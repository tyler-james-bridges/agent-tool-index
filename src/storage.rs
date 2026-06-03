use std::fs;
use std::path::Path;

use anyhow::Result;
use chrono::Utc;
use rusqlite::{Connection, params};

use crate::types::{RegistryEventRecord, Snapshot};

pub fn init_db(path: &str) -> Result<()> {
    ensure_parent(path)?;
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            chain_id INTEGER NOT NULL,
            registry TEXT NOT NULL,
            tool_count INTEGER NOT NULL,
            synced_at TEXT NOT NULL,
            snapshot_json TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS tools (
            chain_id INTEGER NOT NULL,
            registry TEXT NOT NULL,
            tool_id INTEGER NOT NULL,
            status TEXT NOT NULL,
            name TEXT,
            metadata_uri TEXT,
            record_json TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (chain_id, registry, tool_id)
        );
        CREATE TABLE IF NOT EXISTS registry_events (
            chain_id INTEGER NOT NULL,
            registry TEXT NOT NULL,
            block_number INTEGER NOT NULL,
            block_timestamp TEXT,
            tx_hash TEXT NOT NULL,
            log_index INTEGER NOT NULL,
            kind TEXT NOT NULL,
            tool_id INTEGER,
            decoded_json TEXT NOT NULL,
            raw_json TEXT NOT NULL,
            PRIMARY KEY (chain_id, registry, tx_hash, log_index)
        );",
    )?;
    Ok(())
}

pub fn save_snapshot_db(path: &str, snapshot: &Snapshot) -> Result<()> {
    init_db(path)?;
    let mut conn = Connection::open(path)?;
    let tx = conn.transaction()?;
    let snapshot_json = serde_json::to_string(snapshot)?;
    tx.execute(
        "INSERT INTO snapshots (chain_id, registry, tool_count, synced_at, snapshot_json)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            snapshot.chain_id,
            snapshot.registry,
            snapshot.tool_count,
            snapshot.synced_at.to_rfc3339(),
            snapshot_json
        ],
    )?;

    let now = Utc::now().to_rfc3339();
    for tool in &snapshot.tools {
        tx.execute(
            "INSERT INTO tools (chain_id, registry, tool_id, status, name, metadata_uri, record_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(chain_id, registry, tool_id) DO UPDATE SET
             status=excluded.status,
             name=excluded.name,
             metadata_uri=excluded.metadata_uri,
             record_json=excluded.record_json,
             updated_at=excluded.updated_at",
            params![
                tool.chain_id,
                tool.registry,
                tool.tool_id,
                serde_json::to_value(&tool.status)?.as_str().unwrap_or("unknown"),
                tool.name,
                tool.metadata_uri,
                serde_json::to_string(tool)?,
                now
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn load_snapshot_db(path: &str) -> Result<Option<Snapshot>> {
    if !Path::new(path).exists() {
        return Ok(None);
    }
    let conn = Connection::open(path)?;
    let mut stmt = conn.prepare("SELECT snapshot_json FROM snapshots ORDER BY id DESC LIMIT 1")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        let json: String = row.get(0)?;
        Ok(Some(serde_json::from_str(&json)?))
    } else {
        Ok(None)
    }
}

pub fn save_events_db(path: &str, events: &[RegistryEventRecord]) -> Result<usize> {
    init_db(path)?;
    let mut conn = Connection::open(path)?;
    let tx = conn.transaction()?;
    let mut inserted = 0;
    for event in events {
        let changed = tx.execute(
            "INSERT OR IGNORE INTO registry_events
             (chain_id, registry, block_number, block_timestamp, tx_hash, log_index, kind, tool_id, decoded_json, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                event.chain_id,
                event.registry,
                event.block_number,
                event.block_timestamp,
                event.tx_hash,
                event.log_index,
                serde_json::to_value(&event.kind)?.as_str().unwrap_or("unknown"),
                event.tool_id,
                serde_json::to_string(event)?,
                serde_json::to_string(&event.raw)?,
            ],
        )?;
        inserted += changed;
    }
    tx.commit()?;
    Ok(inserted)
}

pub fn event_count(path: &str) -> Result<u64> {
    if !Path::new(path).exists() {
        return Ok(0);
    }
    let conn = Connection::open(path)?;
    let count: u64 =
        conn.query_row("SELECT COUNT(*) FROM registry_events", [], |row| row.get(0))?;
    Ok(count)
}

fn ensure_parent(path: &str) -> Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

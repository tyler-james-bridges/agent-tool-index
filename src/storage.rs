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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        BASE_CHAIN_ID, BASE_REGISTRY, ManifestStatus, RegistryEventKind, RegistryEventRecord,
        Snapshot, ToolRecord, ToolStatus,
    };
    use serde_json::json;
    use tempfile::tempdir;

    fn db_path(dir: &tempfile::TempDir) -> String {
        dir.path().join("index.sqlite").to_str().unwrap().to_string()
    }

    fn make_tool(tool_id: u64) -> ToolRecord {
        ToolRecord {
            chain_id: BASE_CHAIN_ID,
            registry: BASE_REGISTRY.to_string(),
            tool_id,
            status: ToolStatus::Active,
            creator: None,
            metadata_uri: Some(format!("ipfs://tool-{tool_id}")),
            manifest_hash: None,
            access_predicate: None,
            predicate_type: "unknown".to_string(),
            manifest_status: ManifestStatus::Unchecked,
            computed_manifest_hash: None,
            name: Some(format!("Tool {tool_id}")),
            description: None,
            endpoint: None,
            tags: Vec::new(),
            has_x402: false,
            has_auth: false,
            error: None,
            manifest: None,
            checked_at: Utc::now(),
        }
    }

    fn make_snapshot(tools: Vec<ToolRecord>) -> Snapshot {
        Snapshot {
            chain_id: BASE_CHAIN_ID,
            registry: BASE_REGISTRY.to_string(),
            tool_count: tools.len() as u64,
            synced_at: Utc::now(),
            tools,
        }
    }

    fn make_event(log_index: u64) -> RegistryEventRecord {
        RegistryEventRecord {
            chain_id: BASE_CHAIN_ID,
            registry: BASE_REGISTRY.to_string(),
            block_number: 100,
            block_timestamp: None,
            tx_hash: "0xabc".to_string(),
            log_index,
            kind: RegistryEventKind::ToolRegistered,
            tool_id: Some(1),
            creator: None,
            metadata_uri: None,
            manifest_hash: None,
            access_predicate: None,
            raw: json!({}),
        }
    }

    fn table_names(path: &str) -> Vec<String> {
        let conn = Connection::open(path).unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        rows
    }

    #[test]
    fn init_db_creates_expected_tables() {
        let dir = tempdir().unwrap();
        let path = db_path(&dir);
        init_db(&path).unwrap();
        let tables = table_names(&path);
        assert!(tables.contains(&"snapshots".to_string()));
        assert!(tables.contains(&"tools".to_string()));
        assert!(tables.contains(&"registry_events".to_string()));
    }

    #[test]
    fn event_count_missing_file_is_zero() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.sqlite");
        assert_eq!(event_count(path.to_str().unwrap()).unwrap(), 0);
    }

    #[test]
    fn event_count_fresh_db_is_zero() {
        let dir = tempdir().unwrap();
        let path = db_path(&dir);
        init_db(&path).unwrap();
        assert_eq!(event_count(&path).unwrap(), 0);
    }

    #[test]
    fn snapshot_db_round_trips() {
        let dir = tempdir().unwrap();
        let path = db_path(&dir);
        let snapshot = make_snapshot(vec![make_tool(1), make_tool(2)]);
        save_snapshot_db(&path, &snapshot).unwrap();

        let loaded = load_snapshot_db(&path).unwrap().expect("snapshot present");
        assert_eq!(loaded.tool_count, 2);
        assert_eq!(loaded.tools.len(), 2);
        assert_eq!(loaded.chain_id, BASE_CHAIN_ID);
    }

    #[test]
    fn load_snapshot_db_missing_file_is_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing.sqlite");
        assert!(load_snapshot_db(path.to_str().unwrap()).unwrap().is_none());
    }

    #[test]
    fn load_snapshot_db_returns_latest_snapshot() {
        let dir = tempdir().unwrap();
        let path = db_path(&dir);
        save_snapshot_db(&path, &make_snapshot(vec![make_tool(1)])).unwrap();
        save_snapshot_db(&path, &make_snapshot(vec![make_tool(1), make_tool(2), make_tool(3)]))
            .unwrap();

        let loaded = load_snapshot_db(&path).unwrap().unwrap();
        assert_eq!(loaded.tool_count, 3);
    }

    #[test]
    fn save_snapshot_db_upserts_tools_by_primary_key() {
        let dir = tempdir().unwrap();
        let path = db_path(&dir);
        save_snapshot_db(&path, &make_snapshot(vec![make_tool(1)])).unwrap();

        let mut updated = make_tool(1);
        updated.name = Some("Renamed".to_string());
        save_snapshot_db(&path, &make_snapshot(vec![updated])).unwrap();

        let conn = Connection::open(&path).unwrap();
        let tool_rows: u64 = conn
            .query_row("SELECT COUNT(*) FROM tools", [], |row| row.get(0))
            .unwrap();
        assert_eq!(tool_rows, 1, "same tool_id must upsert, not duplicate");

        let name: String = conn
            .query_row("SELECT name FROM tools WHERE tool_id = 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(name, "Renamed");
    }

    #[test]
    fn save_events_db_inserts_and_deduplicates() {
        let dir = tempdir().unwrap();
        let path = db_path(&dir);

        let inserted = save_events_db(&path, &[make_event(0), make_event(1)]).unwrap();
        assert_eq!(inserted, 2);
        assert_eq!(event_count(&path).unwrap(), 2);

        // Re-inserting the same (tx_hash, log_index) pairs is ignored.
        let reinserted = save_events_db(&path, &[make_event(0), make_event(1)]).unwrap();
        assert_eq!(reinserted, 0);
        assert_eq!(event_count(&path).unwrap(), 2);

        // A new log_index is a distinct row.
        let added = save_events_db(&path, &[make_event(2)]).unwrap();
        assert_eq!(added, 1);
        assert_eq!(event_count(&path).unwrap(), 3);
    }
}

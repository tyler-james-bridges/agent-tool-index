mod cache;
mod events;
mod indexer;
mod storage;
mod types;
mod web;

use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio::sync::RwLock;
use tracing_subscriber::EnvFilter;

use crate::cache::{load_snapshot, save_snapshot};
use crate::events::{apply_event_history, backfill_events};
use crate::indexer::sync_registry;
use crate::storage::{event_count, init_db, load_snapshot_db, save_events_db, save_snapshot_db};
use crate::types::{CACHE_PATH, DB_PATH, DEFAULT_RPC_URL};
use crate::web::{AppState, serve};

#[derive(Debug, Parser)]
#[command(
    name = "agent-tool-index",
    about = "Agent-first ERC-8257 registry index demo"
)]
struct Cli {
    #[arg(long, env = "BASE_RPC_URL", default_value = DEFAULT_RPC_URL)]
    rpc_url: String,

    #[arg(long, env = "ERC8257_CACHE", default_value = CACHE_PATH)]
    cache_path: String,

    #[arg(long, env = "ERC8257_DB", default_value = DB_PATH)]
    db_path: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Sync,
    BackfillEvents,
    Serve {
        #[arg(long, default_value = "127.0.0.1:8787")]
        addr: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Sync => {
            let mut snapshot = sync_registry(&cli.rpc_url).await?;
            let events = backfill_events().await.unwrap_or_default();
            apply_event_history(&mut snapshot, &events).await?;
            let stats = snapshot.stats();
            save_snapshot(&cli.cache_path, &snapshot)?;
            save_snapshot_db(&cli.db_path, &snapshot)?;
            if !events.is_empty() {
                save_events_db(&cli.db_path, &events)?;
            }
            println!(
                "synced {} ids: {} active, {} deregistered, {} verified manifests, {} events stored",
                stats.total_ids,
                stats.active,
                stats.deregistered,
                stats.verified_manifests,
                event_count(&cli.db_path)?
            );
        }
        Command::BackfillEvents => {
            init_db(&cli.db_path)?;
            let events = backfill_events().await?;
            let inserted = save_events_db(&cli.db_path, &events)?;
            println!(
                "fetched {} events, inserted {} new rows",
                events.len(),
                inserted
            );
        }
        Command::Serve { addr } => {
            init_db(&cli.db_path)?;
            let snapshot =
                load_snapshot_db(&cli.db_path)?.unwrap_or(load_snapshot(&cli.cache_path)?);
            let state = AppState {
                snapshot: Arc::new(RwLock::new(snapshot)),
                rpc_url: cli.rpc_url,
                cache_path: cli.cache_path,
                db_path: cli.db_path,
            };
            tracing::info!("serving http://{}", addr);
            serve(&addr, state).await?;
        }
    }
    Ok(())
}

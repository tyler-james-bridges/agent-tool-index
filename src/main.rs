mod cache;
mod events;
mod indexer;
mod storage;
mod types;
mod verify;
mod web;

use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio::sync::RwLock;
use tracing_subscriber::EnvFilter;

use crate::cache::{load_snapshot, save_snapshot};
use crate::events::{apply_event_history, apply_event_history_multi_chain, backfill_all_events, backfill_events, backfill_events_legacy};
use crate::indexer::{sync_all_chains, sync_registry, sync_registry_legacy};
use crate::storage::{event_count, init_db, load_snapshot_db, save_events_db, save_snapshot_db};
use crate::types::{CACHE_PATH, CHAINS, DB_PATH, DEFAULT_RPC_URL, MultiChainSnapshot, Snapshot};
use crate::web::{AppState, fallback_snapshot, serve};

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
    Sync {
        #[arg(long, help = "Comma-separated chain IDs to sync (default: all)")]
        chains: Option<String>,
    },
    BackfillEvents {
        #[arg(long, help = "Comma-separated chain IDs to backfill (default: all)")]
        chains: Option<String>,
    },
    ExportStatic,
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
        Command::Sync { chains } => {
            if let Some(chain_filter) = chains {
                let chain_ids: Vec<u64> = chain_filter
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                
                let filtered_chains: Vec<_> = CHAINS
                    .iter()
                    .filter(|config| chain_ids.contains(&config.chain_id))
                    .collect();
                
                if filtered_chains.is_empty() {
                    eprintln!("No valid chains found for IDs: {}", chain_filter);
                    return Ok(());
                }
                
                let mut all_tools = Vec::new();
                let mut all_events = Vec::new();
                
                for chain_config in filtered_chains {
                    if let Ok(mut snapshot) = sync_registry(chain_config).await {
                        if let Ok(events) = backfill_events(chain_config).await {
                            apply_event_history(&mut snapshot, &events).await?;
                            all_events.extend(events);
                        }
                        all_tools.extend(snapshot.tools);
                    }
                }
                
                let multi_snapshot = MultiChainSnapshot {
                    synced_at: chrono::Utc::now(),
                    tools: all_tools,
                };
                
                let stats = multi_snapshot.stats();
                save_snapshot_db(&cli.db_path, &multi_snapshot.into())?;
                if !all_events.is_empty() {
                    save_events_db(&cli.db_path, &all_events)?;
                }
                
                println!(
                    "synced {} tools from {} chains: {} active, {} deregistered, {} verified manifests, {} events stored",
                    stats.total_ids,
                    chain_ids.len(),
                    stats.active,
                    stats.deregistered,
                    stats.verified_manifests,
                    event_count(&cli.db_path)?
                );
            } else if cli.rpc_url != DEFAULT_RPC_URL {
                // Legacy mode: use provided RPC URL
                let mut snapshot = sync_registry_legacy(&cli.rpc_url).await?;
                let events = backfill_events_legacy().await.unwrap_or_default();
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
            } else {
                // Multi-chain mode: sync all chains
                let mut multi_snapshot = sync_all_chains().await?;
                let events = backfill_all_events().await.unwrap_or_default();
                apply_event_history_multi_chain(&mut multi_snapshot, &events).await?;
                let stats = multi_snapshot.stats();
                let snapshot: Snapshot = multi_snapshot.clone().into();
                let chains_summary = snapshot.chains_summary();
                save_snapshot(&cli.cache_path, &snapshot)?;
                save_snapshot_db(&cli.db_path, &snapshot)?;
                if !events.is_empty() {
                    save_events_db(&cli.db_path, &events)?;
                }
                
                println!(
                    "synced {} tools across {} chains: {} active, {} deregistered, {} verified manifests, {} events stored",
                    stats.total_ids,
                    chains_summary.len(),
                    stats.active,
                    stats.deregistered,
                    stats.verified_manifests,
                    event_count(&cli.db_path)?
                );
                
                for (chain_id, name, count) in chains_summary {
                    println!("  {} ({}): {} tools", name, chain_id, count);
                }
            }
        }
        Command::BackfillEvents { chains } => {
            init_db(&cli.db_path)?;
            
            if let Some(chain_filter) = chains {
                let chain_ids: Vec<u64> = chain_filter
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                
                let filtered_chains: Vec<_> = CHAINS
                    .iter()
                    .filter(|config| chain_ids.contains(&config.chain_id))
                    .collect();
                
                if filtered_chains.is_empty() {
                    eprintln!("No valid chains found for IDs: {}", chain_filter);
                    return Ok(());
                }
                
                let mut all_events = Vec::new();
                for chain_config in filtered_chains {
                    if let Ok(events) = backfill_events(chain_config).await {
                        all_events.extend(events);
                    }
                }
                
                let inserted = save_events_db(&cli.db_path, &all_events)?;
                println!(
                    "fetched {} events from {} chains, inserted {} new rows",
                    all_events.len(),
                    chain_ids.len(),
                    inserted
                );
            } else {
                let events = backfill_all_events().await?;
                let inserted = save_events_db(&cli.db_path, &events)?;
                println!(
                    "fetched {} events from all chains, inserted {} new rows",
                    events.len(),
                    inserted
                );
            }
        }
        Command::ExportStatic => {
            init_db(&cli.db_path)?;
            let snapshot = load_snapshot_db(&cli.db_path)?
                .unwrap_or_else(|| load_snapshot(&cli.cache_path).unwrap_or_else(|_| fallback_snapshot().unwrap()));
            
            let registry = web::frontend_registry(&snapshot);
            let js_content = format!("window.REGISTRY = {};", serde_json::to_string(&registry)?);
            
            std::fs::write("web/registry-data.js", js_content)?;
            
            let stats = snapshot.stats();
            let chains_summary = snapshot.chains_summary();
            println!(
                "exported static registry: {} tools across {} chains ({} active, {} verified manifests)",
                stats.total_ids,
                chains_summary.len(),
                stats.active,
                stats.verified_manifests
            );
        }
        Command::Serve { addr } => {
            init_db(&cli.db_path)?;
            let mut snapshot =
                load_snapshot_db(&cli.db_path)?.unwrap_or(load_snapshot(&cli.cache_path)?);
            if snapshot.tools.is_empty() {
                snapshot = fallback_snapshot()?;
            }
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

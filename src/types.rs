use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const BASE_CHAIN_ID: u64 = 8453;
pub const BASE_REGISTRY: &str = "0x265BB2DBFC0A8165C9A1941Eb1372F349baD2cf1";
pub const DEFAULT_RPC_URL: &str = "https://mainnet.base.org";
pub const CACHE_PATH: &str = "data/tools.json";
pub const DB_PATH: &str = "data/index.sqlite";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    Active,
    Deregistered,
    ReadError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestStatus {
    Unchecked,
    Verified,
    HashMismatch,
    FetchError,
    ParseError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRecord {
    pub chain_id: u64,
    pub registry: String,
    pub tool_id: u64,
    pub status: ToolStatus,
    pub creator: Option<String>,
    pub metadata_uri: Option<String>,
    pub manifest_hash: Option<String>,
    pub access_predicate: Option<String>,
    pub manifest_status: ManifestStatus,
    pub computed_manifest_hash: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub endpoint: Option<String>,
    pub tags: Vec<String>,
    pub has_x402: bool,
    pub has_auth: bool,
    pub error: Option<String>,
    pub manifest: Option<Value>,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub total_ids: usize,
    pub active: usize,
    pub deregistered: usize,
    pub read_errors: usize,
    pub verified_manifests: usize,
    pub hash_mismatches: usize,
    pub fetch_errors: usize,
    pub x402_tools: usize,
    pub gated_tools: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub chain_id: u64,
    pub registry: String,
    pub tool_count: u64,
    pub synced_at: DateTime<Utc>,
    pub tools: Vec<ToolRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryEventKind {
    ToolRegistered,
    ToolDeregistered,
    ToolMetadataUpdated,
    AccessPredicateUpdated,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEventRecord {
    pub chain_id: u64,
    pub registry: String,
    pub block_number: u64,
    pub block_timestamp: Option<String>,
    pub tx_hash: String,
    pub log_index: u64,
    pub kind: RegistryEventKind,
    pub tool_id: Option<u64>,
    pub creator: Option<String>,
    pub metadata_uri: Option<String>,
    pub manifest_hash: Option<String>,
    pub access_predicate: Option<String>,
    pub raw: Value,
}

impl Snapshot {
    pub fn empty() -> Self {
        Self {
            chain_id: BASE_CHAIN_ID,
            registry: BASE_REGISTRY.to_string(),
            tool_count: 0,
            synced_at: Utc::now(),
            tools: Vec::new(),
        }
    }

    pub fn stats(&self) -> Stats {
        let mut stats = Stats {
            total_ids: self.tools.len(),
            ..Stats::default()
        };
        for tool in &self.tools {
            match tool.status {
                ToolStatus::Active => stats.active += 1,
                ToolStatus::Deregistered => stats.deregistered += 1,
                ToolStatus::ReadError => stats.read_errors += 1,
            }
            match tool.manifest_status {
                ManifestStatus::Verified => stats.verified_manifests += 1,
                ManifestStatus::HashMismatch => stats.hash_mismatches += 1,
                ManifestStatus::FetchError => stats.fetch_errors += 1,
                ManifestStatus::Unchecked | ManifestStatus::ParseError => {}
            }
            if tool.has_x402 {
                stats.x402_tools += 1;
            }
            if tool.access_predicate.as_deref()
                != Some("0x0000000000000000000000000000000000000000")
                && tool.access_predicate.is_some()
            {
                stats.gated_tools += 1;
            }
        }
        stats
    }
}

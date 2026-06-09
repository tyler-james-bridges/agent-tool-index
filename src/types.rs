use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const BASE_CHAIN_ID: u64 = 8453;
pub const BASE_REGISTRY: &str = "0x265BB2DBFC0A8165C9A1941Eb1372F349baD2cf1";
pub const DEFAULT_RPC_URL: &str = "https://mainnet.base.org";
pub const CACHE_PATH: &str = "data/tools.json";
pub const DB_PATH: &str = "data/index.sqlite";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    pub chain_id: u64,
    pub name: &'static str,
    pub registry: &'static str,
    pub rpc_url: &'static str,
    pub blockscout_url: &'static str,
}

pub const CHAINS: &[ChainConfig] = &[
    ChainConfig {
        chain_id: 1,
        name: "Ethereum",
        registry: "0x265BB2DBFC0A8165C9A1941Eb1372F349baD2cf1",
        rpc_url: "https://ethereum-rpc.publicnode.com",
        blockscout_url: "https://eth.blockscout.com",
    },
    ChainConfig {
        chain_id: 8453,
        name: "Base",
        registry: "0x265BB2DBFC0A8165C9A1941Eb1372F349baD2cf1",
        rpc_url: "https://mainnet.base.org",
        blockscout_url: "https://base.blockscout.com",
    },
    ChainConfig {
        chain_id: 2741,
        name: "Abstract",
        registry: "0x265BB2DBFC0A8165C9A1941Eb1372F349baD2cf1",
        rpc_url: "https://api.mainnet.abs.xyz",
        blockscout_url: "https://explorer.abs.xyz",
    },
];

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
    pub predicate_type: String,
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
pub struct MultiChainSnapshot {
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

    pub fn chains_summary(&self) -> Vec<(u64, &str, usize)> {
        use std::collections::HashMap;
        let mut chains: HashMap<u64, usize> = HashMap::new();
        
        for tool in &self.tools {
            *chains.entry(tool.chain_id).or_insert(0) += 1;
        }
        
        let mut result = Vec::new();
        for config in CHAINS {
            let count = chains.get(&config.chain_id).copied().unwrap_or(0);
            result.push((config.chain_id, config.name, count));
        }
        
        result
    }
}

impl MultiChainSnapshot {
    pub fn empty() -> Self {
        Self {
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

    pub fn chains_summary(&self) -> Vec<(u64, &str, usize)> {
        use std::collections::HashMap;
        let mut chains: HashMap<u64, usize> = HashMap::new();
        
        for tool in &self.tools {
            *chains.entry(tool.chain_id).or_insert(0) += 1;
        }
        
        let mut result = Vec::new();
        for config in CHAINS {
            let count = chains.get(&config.chain_id).copied().unwrap_or(0);
            result.push((config.chain_id, config.name, count));
        }
        
        result
    }
}


impl From<MultiChainSnapshot> for Snapshot {
    fn from(multi: MultiChainSnapshot) -> Self {
        // Find the chain with the most tools or default to Base
        let chain_config = multi.tools
            .iter()
            .map(|tool| tool.chain_id)
            .max_by_key(|&chain_id| {
                multi.tools.iter().filter(|tool| tool.chain_id == chain_id).count()
            })
            .and_then(|chain_id| CHAINS.iter().find(|config| config.chain_id == chain_id))
            .unwrap_or(&CHAINS[1]); // Default to Base

        Self {
            chain_id: chain_config.chain_id,
            registry: chain_config.registry.to_string(),
            tool_count: multi.tools.len() as u64,
            synced_at: multi.synced_at,
            tools: multi.tools,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO_ADDR: &str = "0x0000000000000000000000000000000000000000";

    fn make_tool(chain_id: u64, tool_id: u64) -> ToolRecord {
        ToolRecord {
            chain_id,
            registry: BASE_REGISTRY.to_string(),
            tool_id,
            status: ToolStatus::Active,
            creator: None,
            metadata_uri: None,
            manifest_hash: None,
            access_predicate: None,
            predicate_type: "unknown".to_string(),
            manifest_status: ManifestStatus::Unchecked,
            computed_manifest_hash: None,
            name: None,
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

    #[test]
    fn stats_default_is_all_zero() {
        let stats = Stats::default();
        assert_eq!(stats.total_ids, 0);
        assert_eq!(stats.active, 0);
        assert_eq!(stats.deregistered, 0);
        assert_eq!(stats.read_errors, 0);
        assert_eq!(stats.verified_manifests, 0);
        assert_eq!(stats.hash_mismatches, 0);
        assert_eq!(stats.fetch_errors, 0);
        assert_eq!(stats.x402_tools, 0);
        assert_eq!(stats.gated_tools, 0);
    }

    #[test]
    fn stats_empty_snapshot_is_zero() {
        let stats = make_snapshot(Vec::new()).stats();
        assert_eq!(stats.total_ids, 0);
        assert_eq!(stats.active, 0);
        assert_eq!(stats.gated_tools, 0);
    }

    #[test]
    fn stats_counts_statuses() {
        let mut active = make_tool(8453, 1);
        active.status = ToolStatus::Active;
        let mut dereg = make_tool(8453, 2);
        dereg.status = ToolStatus::Deregistered;
        let mut read_err = make_tool(8453, 3);
        read_err.status = ToolStatus::ReadError;

        let stats = make_snapshot(vec![active, dereg, read_err]).stats();
        assert_eq!(stats.total_ids, 3);
        assert_eq!(stats.active, 1);
        assert_eq!(stats.deregistered, 1);
        assert_eq!(stats.read_errors, 1);
    }

    #[test]
    fn stats_counts_manifest_statuses() {
        let mut verified = make_tool(8453, 1);
        verified.manifest_status = ManifestStatus::Verified;
        let mut mismatch = make_tool(8453, 2);
        mismatch.manifest_status = ManifestStatus::HashMismatch;
        let mut fetch_err = make_tool(8453, 3);
        fetch_err.manifest_status = ManifestStatus::FetchError;
        let mut unchecked = make_tool(8453, 4);
        unchecked.manifest_status = ManifestStatus::Unchecked;
        let mut parse_err = make_tool(8453, 5);
        parse_err.manifest_status = ManifestStatus::ParseError;

        let stats = make_snapshot(vec![verified, mismatch, fetch_err, unchecked, parse_err]).stats();
        assert_eq!(stats.verified_manifests, 1);
        assert_eq!(stats.hash_mismatches, 1);
        assert_eq!(stats.fetch_errors, 1);
    }

    #[test]
    fn stats_counts_x402() {
        let mut paid = make_tool(8453, 1);
        paid.has_x402 = true;
        let free = make_tool(8453, 2);
        let stats = make_snapshot(vec![paid, free]).stats();
        assert_eq!(stats.x402_tools, 1);
    }

    #[test]
    fn stats_gated_counts_only_nonzero_predicate() {
        let mut gated = make_tool(8453, 1);
        gated.access_predicate = Some("0x000000000000000000000000000000000000dEaD".to_string());
        let mut open = make_tool(8453, 2);
        open.access_predicate = Some(ZERO_ADDR.to_string());
        let none = make_tool(8453, 3); // access_predicate None

        let stats = make_snapshot(vec![gated, open, none]).stats();
        assert_eq!(stats.gated_tools, 1);
    }

    #[test]
    fn chains_summary_lists_all_chains_in_config_order() {
        let snapshot = make_snapshot(vec![
            make_tool(1, 1),
            make_tool(8453, 2),
            make_tool(8453, 3),
        ]);
        let summary = snapshot.chains_summary();

        assert_eq!(summary.len(), 3);
        assert_eq!(summary[0], (1, "Ethereum", 1));
        assert_eq!(summary[1], (8453, "Base", 2));
        assert_eq!(summary[2], (2741, "Abstract", 0));
    }

    #[test]
    fn chains_summary_empty_snapshot_has_zero_counts() {
        let snapshot = make_snapshot(Vec::new());
        let summary = snapshot.chains_summary();
        assert_eq!(summary.len(), 3);
        for (_, _, count) in summary {
            assert_eq!(count, 0);
        }
    }

    #[test]
    fn multichain_into_snapshot_picks_majority_chain() {
        let multi = MultiChainSnapshot {
            synced_at: Utc::now(),
            tools: vec![make_tool(1, 1), make_tool(1, 2), make_tool(8453, 3)],
        };
        let snapshot: Snapshot = multi.into();
        assert_eq!(snapshot.chain_id, 1);
        assert_eq!(snapshot.registry, BASE_REGISTRY);
        assert_eq!(snapshot.tool_count, 3);
        assert_eq!(snapshot.tools.len(), 3);
    }

    #[test]
    fn multichain_empty_into_snapshot_defaults_to_base() {
        let snapshot: Snapshot = MultiChainSnapshot::empty().into();
        assert_eq!(snapshot.chain_id, 8453);
        assert_eq!(snapshot.tool_count, 0);
        assert!(snapshot.tools.is_empty());
    }

    #[test]
    fn multichain_stats_matches_snapshot_stats() {
        let mut active = make_tool(1, 1);
        active.has_x402 = true;
        let multi = MultiChainSnapshot {
            synced_at: Utc::now(),
            tools: vec![active],
        };
        let stats = multi.stats();
        assert_eq!(stats.total_ids, 1);
        assert_eq!(stats.active, 1);
        assert_eq!(stats.x402_tools, 1);
    }
}

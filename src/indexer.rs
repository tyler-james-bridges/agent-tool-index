use alloy::primitives::{Address, U256, keccak256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol;
use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::Value;
use tokio::time::{Duration, sleep};

use crate::types::{
    ChainConfig, CHAINS, ManifestStatus, MultiChainSnapshot, Snapshot, ToolRecord, ToolStatus,
};

const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";

pub fn predicate_label(address: &str) -> &'static str {
    let addr = address.to_ascii_lowercase();
    match addr.as_str() {
        "0x0000000000000000000000000000000000000000" => "open",
        "0xc8721c9a776958fffeb602da1b708bf1d318379" => "erc721",
        "0x77373dc3c1ae9a1e937ef3e5e08f4807d47c7c11" => "erc1155",
        "0xcbe0cd9b1d99d95baa9c58f2767246c52e461f25" => "subscription",
        "0x10abf07cfa34bf22372c57f27e8bd9c2dcf93fa1" => "trait_gated",
        "0x1a834fc48b5f6e119c62c12a98b32137bcfa77cd" => "erc20_balance",
        _ => if addr == "0x0000000000000000000000000000000000000000" { "open" } else { "custom" }
    }
}

sol! {
    struct ToolConfig {
        address creator;
        string metadataURI;
        bytes32 manifestHash;
        address accessPredicate;
    }

    #[sol(rpc)]
    contract ToolRegistry {
        function toolCount() external view returns (uint256 count);
        function getToolConfig(uint256 toolId) external view returns (ToolConfig config);
        error ToolIsDeregistered(uint256 toolId);
        error ToolNotFound(uint256 toolId);
    }
}

pub async fn sync_registry(chain_config: &ChainConfig) -> Result<Snapshot> {
    let provider = ProviderBuilder::new().connect_http(chain_config.rpc_url.parse()?);
    let registry_addr: Address = chain_config.registry.parse()?;
    let registry = ToolRegistry::new(registry_addr, &provider);
    let count = registry.toolCount().call().await?.to::<u64>();
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()?;

    let mut tools = Vec::with_capacity(count as usize);
    for tool_id in 1..=count {
        let record = match read_config_with_retry(&registry, tool_id).await {
            Ok(config) => build_active_record(tool_id, config, &http, chain_config).await,
            Err(err) => build_error_record(tool_id, err, chain_config),
        };
        tools.push(record);
        sleep(Duration::from_millis(850)).await;
    }

    Ok(Snapshot {
        chain_id: chain_config.chain_id,
        registry: chain_config.registry.to_string(),
        tool_count: count,
        synced_at: Utc::now(),
        tools,
    })
}

pub async fn sync_all_chains() -> Result<MultiChainSnapshot> {
    let mut all_tools = Vec::new();
    
    for chain_config in CHAINS {
        match sync_registry(chain_config).await {
            Ok(snapshot) => {
                all_tools.extend(snapshot.tools);
            }
            Err(err) => {
                eprintln!("Failed to sync chain {}: {}", chain_config.name, err);
            }
        }
    }
    
    Ok(MultiChainSnapshot {
        synced_at: Utc::now(),
        tools: all_tools,
    })
}

// Backward compatibility wrapper
pub async fn sync_registry_legacy(rpc_url: &str) -> Result<Snapshot> {
    // Find matching chain config by RPC URL, default to Base
    let chain_config = CHAINS.iter()
        .find(|config| config.rpc_url == rpc_url)
        .unwrap_or(&CHAINS[1]); // Base is index 1
    
    sync_registry(chain_config).await
}

async fn read_config_with_retry<P>(
    registry: &ToolRegistry::ToolRegistryInstance<&P>,
    tool_id: u64,
) -> Result<ToolConfig, String>
where
    P: Provider,
{
    let mut last_error = String::new();
    for attempt in 0..8 {
        match registry.getToolConfig(U256::from(tool_id)).call().await {
            Ok(config) => return Ok(config),
            Err(err) => {
                last_error = err.to_string();
                if !last_error.contains("429") && !last_error.contains("over rate limit") {
                    return Err(last_error);
                }
                sleep(Duration::from_millis(1_200 + attempt * 700)).await;
            }
        }
    }
    Err(last_error)
}

async fn build_active_record(
    tool_id: u64,
    config: ToolConfig,
    http: &reqwest::Client,
    chain_config: &ChainConfig,
) -> ToolRecord {
    let metadata_uri = config.metadataURI.clone();
    let manifest_hash = format!("0x{}", hex::encode(config.manifestHash));
    let access_predicate = format!("{:?}", config.accessPredicate);
    let predicate_type = predicate_label(&access_predicate);
    
    let mut record = ToolRecord {
        chain_id: chain_config.chain_id,
        registry: chain_config.registry.to_string(),
        tool_id,
        status: ToolStatus::Active,
        creator: Some(format!("{:?}", config.creator)),
        metadata_uri: Some(metadata_uri.clone()),
        manifest_hash: Some(manifest_hash.clone()),
        access_predicate: Some(access_predicate),
        predicate_type: predicate_type.to_string(),
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
    };

    if let Err(err) = enrich_tool_record(&mut record, http).await {
        record.manifest_status = ManifestStatus::FetchError;
        record.error = Some(err.to_string());
    }
    record
}

fn build_error_record(tool_id: u64, error: String, chain_config: &ChainConfig) -> ToolRecord {
    let status = if error.contains("ToolIsDeregistered") || error.contains("0x0bf47976") {
        ToolStatus::Deregistered
    } else {
        ToolStatus::ReadError
    };

    ToolRecord {
        chain_id: chain_config.chain_id,
        registry: chain_config.registry.to_string(),
        tool_id,
        status,
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
        error: Some(error),
        manifest: None,
        checked_at: Utc::now(),
    }
}

pub async fn enrich_tool_record(record: &mut ToolRecord, http: &reqwest::Client) -> Result<()> {
    let uri = record
        .metadata_uri
        .as_deref()
        .context("missing metadata URI")?;
    let response = http
        .get(uri)
        .send()
        .await
        .context("manifest fetch failed")?;
    if !response.status().is_success() {
        record.manifest_status = ManifestStatus::FetchError;
        record.error = Some(format!("manifest returned HTTP {}", response.status()));
        return Ok(());
    }

    let manifest: Value = response
        .json()
        .await
        .context("manifest JSON parse failed")?;
    let canonical = serde_jcs::to_vec(&manifest).context("manifest canonicalization failed")?;
    let computed = format!("0x{}", hex::encode(keccak256(canonical)));

    record.computed_manifest_hash = Some(computed.clone());
    record.manifest_status = if record.manifest_hash.as_deref() == Some(computed.as_str()) {
        ManifestStatus::Verified
    } else {
        ManifestStatus::HashMismatch
    };
    record.name = manifest
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string);
    record.description = manifest
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_string);
    record.endpoint = manifest
        .get("endpoint")
        .and_then(Value::as_str)
        .map(str::to_string);
    record.tags = manifest
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    record.has_x402 = declares_x402(&manifest);
    record.has_auth = manifest.get("authentication").is_some();
    record.manifest = Some(manifest);
    Ok(())
}

/// True when the manifest actually *declares* x402 payment, not merely mentions it.
///
/// A bare substring scan flagged tools that only describe x402 in prose -- e.g.
/// the verify-tool, whose description says it "decodes ... x402 payment
/// requirements" and whose output schema has a `has_x402` field. Those tools
/// charge nothing, so "Pay ... & run" was a lie. We require x402 to appear in a
/// real declaration: a priced `pricing` entry, or anywhere outside the free-text
/// `description`/`name` and the `inputs`/`outputs` JSON schemas.
fn declares_x402(manifest: &Value) -> bool {
    if manifest_has_pricing_amount(manifest) {
        return true;
    }
    match manifest {
        Value::Object(map) => map.iter().any(|(key, val)| {
            let k = key.to_ascii_lowercase();
            if k == "description" || k == "name" || k == "inputs" || k == "outputs" {
                return false;
            }
            k.contains("x402") || value_contains(val, "x402")
        }),
        _ => false,
    }
}

/// True when `pricing` carries a positive amount (array of entries or a single
/// object). A priced tool is x402 regardless of where the scheme string lives.
/// An `amount` of 0 (e.g. an "open-access" entry) is free, not a charge -- and
/// genuinely metered tools still match via the x402 scheme/tag text below.
fn manifest_has_pricing_amount(manifest: &Value) -> bool {
    let entry = match manifest.get("pricing") {
        Some(Value::Array(items)) => match items.first() {
            Some(e) => e,
            None => return false,
        },
        Some(obj @ Value::Object(_)) => obj,
        _ => return false,
    };
    match entry.get("amount") {
        Some(Value::Number(n)) => n.as_f64().map(|v| v > 0.0).unwrap_or(false),
        Some(Value::String(s)) => s.trim().parse::<f64>().map(|v| v > 0.0).unwrap_or(false),
        _ => false,
    }
}

fn value_contains(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(s) => s.to_ascii_lowercase().contains(needle),
        Value::Array(items) => items.iter().any(|item| value_contains(item, needle)),
        Value::Object(map) => map.iter().any(|(key, val)| {
            key.to_ascii_lowercase().contains(needle) || value_contains(val, needle)
        }),
        _ => false,
    }
}

pub fn access_label(record: &ToolRecord) -> &'static str {
    match record.access_predicate.as_deref() {
        Some(addr) => predicate_label(addr),
        None => "unknown",
    }
}

#[cfg(test)]
mod x402_tests {
    use super::declares_x402;
    use serde_json::json;

    #[test]
    fn prose_and_schema_mentions_do_not_flag_x402() {
        // Shape of the verify-tool manifest: x402 only in the description and as a
        // `has_x402` output-schema key. It charges nothing, so it must read free.
        let manifest = json!({
            "name": "verify-tool",
            "description": "Decodes access predicate and x402 payment requirements.",
            "endpoint": "https://agenttoolindex.xyz/api/verify",
            "tags": ["erc-8257", "verification", "trust"],
            "outputs": { "properties": { "has_x402": { "type": "boolean" } } },
            "pricing": null
        });
        assert!(!declares_x402(&manifest));
    }

    #[test]
    fn priced_pricing_entry_flags_x402() {
        // nft-appraiser: pricing array carries an amount + x402 protocol.
        let manifest = json!({
            "name": "nft-appraiser",
            "pricing": [{ "amount": "50000", "protocol": "x402" }]
        });
        assert!(declares_x402(&manifest));
    }

    #[test]
    fn pricing_object_with_amount_flags_x402() {
        // clawdmint: pricing is a single object, not an array.
        let manifest = json!({
            "name": "clawdmint",
            "tags": ["nft", "x402"],
            "pricing": { "protocol": "x402", "amount": "2.00" }
        });
        assert!(declares_x402(&manifest));
    }

    #[test]
    fn zero_amount_open_access_is_not_x402() {
        // WANCAI: a free, open-access pricing entry with amount "0" must read free.
        let manifest = json!({
            "name": "wancai-wish",
            "pricing": [{ "amount": "0", "protocol": "open-access", "label": "Free" }]
        });
        assert!(!declares_x402(&manifest));
    }

    #[test]
    fn payment_declaration_flags_x402_without_pricing() {
        // swarm-skill: no `pricing`, x402 declared under a `payment` object.
        let manifest = json!({
            "name": "swarm-skill",
            "payment": { "protocol": "x402" }
        });
        assert!(declares_x402(&manifest));
    }
}

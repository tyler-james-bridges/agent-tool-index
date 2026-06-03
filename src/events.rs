use anyhow::Result;
use serde_json::Value;

use crate::indexer::enrich_tool_record;
use crate::types::{
    BASE_CHAIN_ID, BASE_REGISTRY, ManifestStatus, RegistryEventKind, RegistryEventRecord, Snapshot,
};

const BLOCKSCOUT_LOGS: &str =
    "https://base.blockscout.com/api/v2/addresses/0x265BB2DBFC0A8165C9A1941Eb1372F349baD2cf1/logs";
const TOOL_REGISTERED: &str = "0xe7be7fd3c802f61682f56ba817276b1cc81fbee7cb50705c8ed7952811dac397";
const TOOL_DEREGISTERED: &str =
    "0x9add33e854e243f868ff7cacac076d65b1d56fb593e2bb891e76e2e8d5ddd034";
const TOOL_METADATA_UPDATED: &str =
    "0x14f92d1aaaea2df5f884f2fd8dbb6ea7cad1784ffaac2d8fd594f107d719a414";
const ACCESS_PREDICATE_UPDATED: &str =
    "0x53e2d3a37877f4a367d09cdba706178d3c6414a15d642294300c65a8037dd6ff";

pub async fn backfill_events() -> Result<Vec<RegistryEventRecord>> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let mut query: Vec<(String, String)> = Vec::new();
    let mut events = Vec::new();

    for _ in 0..20 {
        let response = http.get(BLOCKSCOUT_LOGS).query(&query).send().await?;
        if !response.status().is_success() {
            anyhow::bail!("Blockscout logs returned HTTP {}", response.status());
        }
        let page: Value = response.json().await?;
        if let Some(items) = page.get("items").and_then(Value::as_array) {
            for item in items {
                if let Some(event) = decode_log(item.clone()) {
                    events.push(event);
                }
            }
        }
        let Some(next) = page.get("next_page_params").and_then(Value::as_object) else {
            break;
        };
        if next.is_empty() {
            break;
        }
        query = next
            .iter()
            .filter_map(|(key, value)| value_to_query(value).map(|v| (key.clone(), v)))
            .collect();
    }

    events.sort_by_key(|event| (event.block_number, event.log_index));
    Ok(events)
}

pub async fn apply_event_history(
    snapshot: &mut Snapshot,
    events: &[RegistryEventRecord],
) -> Result<()> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()?;
    for tool in &mut snapshot.tools {
        let mut creator = None;
        let mut metadata_uri = None;
        let mut manifest_hash = None;
        let mut access_predicate = None;
        for event in events
            .iter()
            .filter(|event| event.tool_id == Some(tool.tool_id))
        {
            if let Some(value) = &event.creator {
                creator = Some(value.clone());
            }
            if let Some(value) = &event.metadata_uri {
                metadata_uri = Some(value.clone());
            }
            if let Some(value) = &event.manifest_hash {
                manifest_hash = Some(value.clone());
            }
            if let Some(value) = &event.access_predicate {
                access_predicate = Some(value.clone());
            }
        }

        if tool.creator.is_none() {
            tool.creator = creator;
        }
        if tool.metadata_uri.is_none() {
            tool.metadata_uri = metadata_uri;
        }
        if tool.manifest_hash.is_none() {
            tool.manifest_hash = manifest_hash;
        }
        if tool.access_predicate.is_none() {
            tool.access_predicate = access_predicate;
        }
        if tool.metadata_uri.is_some() && matches!(tool.manifest_status, ManifestStatus::Unchecked)
        {
            enrich_tool_record(tool, &http).await?;
        }
    }
    Ok(())
}

fn decode_log(raw: Value) -> Option<RegistryEventRecord> {
    let topics = raw.get("topics")?.as_array()?.clone();
    let topic0 = topics.first()?.as_str()?.to_ascii_lowercase();
    let block_number = raw.get("block_number")?.as_u64()?;
    let tx_hash = raw.get("transaction_hash")?.as_str()?.to_string();
    let log_index = raw.get("index")?.as_u64()?;
    let data = raw
        .get("data")
        .and_then(Value::as_str)
        .unwrap_or("0x")
        .to_string();
    let block_timestamp = raw
        .get("block_timestamp")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut event = RegistryEventRecord {
        chain_id: BASE_CHAIN_ID,
        registry: BASE_REGISTRY.to_string(),
        block_number,
        block_timestamp,
        tx_hash,
        log_index,
        kind: RegistryEventKind::Unknown,
        tool_id: topic_u64(topics.get(1)?),
        creator: None,
        metadata_uri: None,
        manifest_hash: None,
        access_predicate: None,
        raw,
    };

    match topic0.as_str() {
        TOOL_REGISTERED => {
            event.kind = RegistryEventKind::ToolRegistered;
            event.creator = topics.get(2).and_then(topic_address);
            event.access_predicate = topics.get(3).and_then(topic_address);
            if let Some((uri, hash)) = decode_string_hash(&data) {
                event.metadata_uri = Some(uri);
                event.manifest_hash = Some(hash);
            }
        }
        TOOL_DEREGISTERED => event.kind = RegistryEventKind::ToolDeregistered,
        TOOL_METADATA_UPDATED => {
            event.kind = RegistryEventKind::ToolMetadataUpdated;
            if let Some((uri, hash)) = decode_string_hash(&data) {
                event.metadata_uri = Some(uri);
                event.manifest_hash = Some(hash);
            }
        }
        ACCESS_PREDICATE_UPDATED => {
            event.kind = RegistryEventKind::AccessPredicateUpdated;
            event.access_predicate = topics.get(2).and_then(topic_address);
        }
        _ => {}
    }
    Some(event)
}

fn decode_string_hash(data: &str) -> Option<(String, String)> {
    let hex = data.strip_prefix("0x").unwrap_or(data);
    if hex.len() < 192 {
        return None;
    }
    let manifest_hash = format!("0x{}", &hex[64..128]);
    let len = usize::from_str_radix(&hex[128..192], 16).ok()?;
    let start = 192;
    let end = start + len * 2;
    if hex.len() < end {
        return None;
    }
    let bytes = hex::decode(&hex[start..end]).ok()?;
    let uri = String::from_utf8(bytes).ok()?;
    Some((uri, manifest_hash))
}

fn topic_u64(value: &Value) -> Option<u64> {
    let hex = value.as_str()?.strip_prefix("0x")?;
    u64::from_str_radix(hex.trim_start_matches('0'), 16).ok()
}

fn topic_address(value: &Value) -> Option<String> {
    let hex = value.as_str()?.strip_prefix("0x")?;
    if hex.len() < 40 {
        return None;
    }
    Some(format!("0x{}", &hex[hex.len() - 40..]))
}

fn value_to_query(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

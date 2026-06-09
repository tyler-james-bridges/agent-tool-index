use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use crate::indexer::{ToolRegistry, enrich_tool_record, predicate_label};
use crate::types::{CHAINS, ChainConfig, ManifestStatus, ToolRecord, ToolStatus};

#[cfg(test)]
const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";

/// Classification of an endpoint probe result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointLiveness {
    Alive,
    Dead,
    Unknown,
}

/// Structured trust report returned by the on-demand verify endpoint.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
    pub chain_id: u64,
    pub chain_name: &'static str,
    pub registry: String,
    pub tool_id: u64,
    pub onchain_found: bool,
    pub status: String,
    pub manifest_status: String,
    pub onchain_manifest_hash: Option<String>,
    pub computed_manifest_hash: Option<String>,
    pub hash_match: bool,
    pub metadata_uri: Option<String>,
    pub endpoint: Option<String>,
    pub endpoint_alive: Option<bool>,
    pub endpoint_liveness: EndpointLiveness,
    pub endpoint_http_status: Option<u16>,
    pub access_predicate: Option<String>,
    pub predicate_type: String,
    pub has_x402: bool,
    pub has_auth: bool,
    pub can_call: Value,
    pub error: Option<String>,
    pub checked_at: DateTime<Utc>,
}

/// Find the configured chain for a given chain id.
pub fn find_chain(chain_id: u64) -> Option<&'static ChainConfig> {
    CHAINS.iter().find(|config| config.chain_id == chain_id)
}

/// Pure mapping from a ToolRecord status enum to the API status string.
pub fn status_string(status: &ToolStatus) -> &'static str {
    match status {
        ToolStatus::Active => "active",
        ToolStatus::Deregistered => "deregistered",
        ToolStatus::ReadError => "read_error",
    }
}

/// Pure mapping from a ManifestStatus enum to the API string.
pub fn manifest_status_string(status: &ManifestStatus) -> &'static str {
    match status {
        ManifestStatus::Unchecked => "unchecked",
        ManifestStatus::Verified => "verified",
        ManifestStatus::HashMismatch => "hash_mismatch",
        ManifestStatus::FetchError => "fetch_error",
        ManifestStatus::ParseError => "parse_error",
    }
}

/// Classify endpoint liveness from a probe outcome.
/// `status` is the HTTP status code if the request completed; `None` means the
/// request itself failed (DNS/connect/timeout). When the endpoint is absent we
/// return Unknown.
pub fn classify_endpoint(endpoint: Option<&str>, status: Option<u16>) -> EndpointLiveness {
    match endpoint {
        None => EndpointLiveness::Unknown,
        Some(_) => match status {
            // Any HTTP response below 500 means the host answered and is serving.
            // 5xx means the server is up but erroring; we still treat it as alive
            // because the host responded. Network-level failures (None) are dead.
            Some(_) => EndpointLiveness::Alive,
            None => EndpointLiveness::Dead,
        },
    }
}

/// Map EndpointLiveness to the nullable boolean exposed in the report.
pub fn liveness_to_bool(liveness: EndpointLiveness) -> Option<bool> {
    match liveness {
        EndpointLiveness::Alive => Some(true),
        EndpointLiveness::Dead => Some(false),
        EndpointLiveness::Unknown => None,
    }
}

/// Whether the onchain manifest hash and the computed JCS hash agree.
pub fn compute_hash_match(onchain: Option<&str>, computed: Option<&str>) -> bool {
    match (onchain, computed) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    }
}

fn access_label_for(record: &ToolRecord) -> &'static str {
    match record.access_predicate.as_deref() {
        Some(addr) => predicate_label(addr),
        None => "unknown",
    }
}

/// Build a can_call-style summary (callable/conditional/not_callable) from a
/// fully-enriched ToolRecord. Pure: no network access.
pub fn can_call_summary(record: &ToolRecord) -> Value {
    let mut requirements: Vec<String> = Vec::new();
    let mut blockers: Vec<String> = Vec::new();

    if !matches!(record.status, ToolStatus::Active) {
        blockers.push("tool is not active in the registry".to_string());
    }

    let gated = access_label_for(record) != "open" && record.access_predicate.is_some();
    if gated {
        requirements.push("access predicate must approve the caller wallet".to_string());
    }
    if record.has_auth {
        requirements.push("manifest declares authentication requirements".to_string());
    }
    if record.has_x402 {
        requirements.push("x402 payment required or accepted".to_string());
    }

    let status = if !blockers.is_empty() {
        "not_callable"
    } else if gated || record.has_auth {
        "conditional"
    } else {
        "callable"
    };

    json!({
        "status": status,
        "requirements": requirements,
        "blockers": blockers,
    })
}

/// Build the trust report from an enriched ToolRecord plus the endpoint probe
/// outcome. Pure: no network access.
pub fn build_report(
    chain: &ChainConfig,
    tool_id: u64,
    record: &ToolRecord,
    endpoint_status: Option<u16>,
) -> VerifyReport {
    let liveness = classify_endpoint(record.endpoint.as_deref(), endpoint_status);
    let hash_match = compute_hash_match(
        record.manifest_hash.as_deref(),
        record.computed_manifest_hash.as_deref(),
    );

    VerifyReport {
        chain_id: chain.chain_id,
        chain_name: chain.name,
        registry: chain.registry.to_string(),
        tool_id,
        onchain_found: true,
        status: status_string(&record.status).to_string(),
        manifest_status: manifest_status_string(&record.manifest_status).to_string(),
        onchain_manifest_hash: record.manifest_hash.clone(),
        computed_manifest_hash: record.computed_manifest_hash.clone(),
        hash_match,
        metadata_uri: record.metadata_uri.clone(),
        endpoint: record.endpoint.clone(),
        endpoint_alive: liveness_to_bool(liveness),
        endpoint_liveness: liveness,
        endpoint_http_status: endpoint_status,
        access_predicate: record.access_predicate.clone(),
        predicate_type: record.predicate_type.clone(),
        has_x402: record.has_x402,
        has_auth: record.has_auth,
        can_call: can_call_summary(record),
        error: record.error.clone(),
        checked_at: record.checked_at,
    }
}

/// Build a report for a tool that could not be read onchain (deregistered,
/// not found, or RPC read error). Pure.
pub fn build_not_found_report(
    chain: &ChainConfig,
    tool_id: u64,
    status: ToolStatus,
    error: Option<String>,
) -> VerifyReport {
    VerifyReport {
        chain_id: chain.chain_id,
        chain_name: chain.name,
        registry: chain.registry.to_string(),
        tool_id,
        onchain_found: false,
        status: status_string(&status).to_string(),
        manifest_status: "unchecked".to_string(),
        onchain_manifest_hash: None,
        computed_manifest_hash: None,
        hash_match: false,
        metadata_uri: None,
        endpoint: None,
        endpoint_alive: None,
        endpoint_liveness: EndpointLiveness::Unknown,
        endpoint_http_status: None,
        access_predicate: None,
        predicate_type: "unknown".to_string(),
        has_x402: false,
        has_auth: false,
        can_call: json!({
            "status": "not_callable",
            "requirements": [],
            "blockers": ["tool is not active in the registry"],
        }),
        error,
        checked_at: Utc::now(),
    }
}

fn deregistered_or_read_error(error: &str) -> ToolStatus {
    if error.contains("ToolIsDeregistered") || error.contains("0x0bf47976") {
        ToolStatus::Deregistered
    } else if error.contains("ToolNotFound") {
        ToolStatus::ReadError
    } else {
        ToolStatus::ReadError
    }
}

/// Probe an endpoint URL with a short-timeout HEAD (falling back to GET) and
/// return the HTTP status code if the host answered, or None on a transport
/// failure. Network access; not unit tested.
async fn probe_endpoint(http: &reqwest::Client, endpoint: &str) -> Option<u16> {
    if let Ok(response) = http.head(endpoint).send().await {
        return Some(response.status().as_u16());
    }
    match http.get(endpoint).send().await {
        Ok(response) => Some(response.status().as_u16()),
        Err(_) => None,
    }
}

/// Live verification: read the tool config onchain, fetch and hash the manifest,
/// probe the endpoint, and return a structured trust report.
pub async fn verify_tool_live(chain_id: u64, tool_id: u64) -> Result<VerifyReport> {
    let chain = find_chain(chain_id)
        .ok_or_else(|| anyhow!("unknown chain id {chain_id}"))?;

    let provider = ProviderBuilder::new().connect_http(chain.rpc_url.parse()?);
    let registry_addr: Address = chain.registry.parse()?;
    let registry = ToolRegistry::new(registry_addr, &provider);

    let config = match registry.getToolConfig(U256::from(tool_id)).call().await {
        Ok(config) => config,
        Err(err) => {
            let message = err.to_string();
            let status = deregistered_or_read_error(&message);
            return Ok(build_not_found_report(chain, tool_id, status, Some(message)));
        }
    };

    let manifest_hash = format!("0x{}", hex::encode(config.manifestHash));
    let access_predicate = format!("{:?}", config.accessPredicate);
    let predicate_type = predicate_label(&access_predicate);

    let mut record = ToolRecord {
        chain_id: chain.chain_id,
        registry: chain.registry.to_string(),
        tool_id,
        status: ToolStatus::Active,
        creator: Some(format!("{:?}", config.creator)),
        metadata_uri: Some(config.metadataURI.clone()),
        manifest_hash: Some(manifest_hash),
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

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()?;

    if let Err(err) = enrich_tool_record(&mut record, &http).await {
        record.manifest_status = ManifestStatus::FetchError;
        record.error = Some(err.to_string());
    }

    let endpoint_status = match record.endpoint.as_deref() {
        Some(endpoint) if !endpoint.is_empty() => {
            let probe_http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()?;
            probe_endpoint(&probe_http, endpoint).await
        }
        _ => None,
    };

    Ok(build_report(chain, tool_id, &record, endpoint_status))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record() -> ToolRecord {
        ToolRecord {
            chain_id: 8453,
            registry: crate::types::BASE_REGISTRY.to_string(),
            tool_id: 1,
            status: ToolStatus::Active,
            creator: None,
            metadata_uri: Some("ipfs://manifest".to_string()),
            manifest_hash: Some("0xabc".to_string()),
            access_predicate: Some(ZERO_ADDRESS.to_string()),
            predicate_type: "open".to_string(),
            manifest_status: ManifestStatus::Verified,
            computed_manifest_hash: Some("0xabc".to_string()),
            name: Some("Tool".to_string()),
            description: None,
            endpoint: Some("https://api.example.com".to_string()),
            tags: Vec::new(),
            has_x402: false,
            has_auth: false,
            error: None,
            manifest: None,
            checked_at: Utc::now(),
        }
    }

    fn base_chain() -> &'static ChainConfig {
        find_chain(8453).unwrap()
    }

    // ---- find_chain ----

    #[test]
    fn find_chain_known_ids() {
        assert_eq!(find_chain(1).unwrap().name, "Ethereum");
        assert_eq!(find_chain(8453).unwrap().name, "Base");
        assert_eq!(find_chain(2741).unwrap().name, "Abstract");
    }

    #[test]
    fn find_chain_unknown_id_is_none() {
        assert!(find_chain(999_999).is_none());
    }

    // ---- status_string / manifest_status_string ----

    #[test]
    fn status_string_maps_each_variant() {
        assert_eq!(status_string(&ToolStatus::Active), "active");
        assert_eq!(status_string(&ToolStatus::Deregistered), "deregistered");
        assert_eq!(status_string(&ToolStatus::ReadError), "read_error");
    }

    #[test]
    fn manifest_status_string_maps_each_variant() {
        assert_eq!(manifest_status_string(&ManifestStatus::Unchecked), "unchecked");
        assert_eq!(manifest_status_string(&ManifestStatus::Verified), "verified");
        assert_eq!(
            manifest_status_string(&ManifestStatus::HashMismatch),
            "hash_mismatch"
        );
        assert_eq!(
            manifest_status_string(&ManifestStatus::FetchError),
            "fetch_error"
        );
        assert_eq!(
            manifest_status_string(&ManifestStatus::ParseError),
            "parse_error"
        );
    }

    // ---- classify_endpoint ----

    #[test]
    fn classify_endpoint_none_is_unknown() {
        assert_eq!(classify_endpoint(None, None), EndpointLiveness::Unknown);
        assert_eq!(classify_endpoint(None, Some(200)), EndpointLiveness::Unknown);
    }

    #[test]
    fn classify_endpoint_with_status_is_alive() {
        assert_eq!(
            classify_endpoint(Some("https://x.com"), Some(200)),
            EndpointLiveness::Alive
        );
        assert_eq!(
            classify_endpoint(Some("https://x.com"), Some(405)),
            EndpointLiveness::Alive
        );
        assert_eq!(
            classify_endpoint(Some("https://x.com"), Some(503)),
            EndpointLiveness::Alive
        );
    }

    #[test]
    fn classify_endpoint_transport_failure_is_dead() {
        assert_eq!(
            classify_endpoint(Some("https://x.com"), None),
            EndpointLiveness::Dead
        );
    }

    // ---- liveness_to_bool ----

    #[test]
    fn liveness_to_bool_maps_each_variant() {
        assert_eq!(liveness_to_bool(EndpointLiveness::Alive), Some(true));
        assert_eq!(liveness_to_bool(EndpointLiveness::Dead), Some(false));
        assert_eq!(liveness_to_bool(EndpointLiveness::Unknown), None);
    }

    // ---- compute_hash_match ----

    #[test]
    fn hash_match_equal_is_true() {
        assert!(compute_hash_match(Some("0xABC"), Some("0xabc")));
    }

    #[test]
    fn hash_match_different_is_false() {
        assert!(!compute_hash_match(Some("0xabc"), Some("0xdef")));
    }

    #[test]
    fn hash_match_missing_side_is_false() {
        assert!(!compute_hash_match(None, Some("0xabc")));
        assert!(!compute_hash_match(Some("0xabc"), None));
        assert!(!compute_hash_match(None, None));
    }

    // ---- can_call_summary ----

    #[test]
    fn can_call_summary_open_active_is_callable() {
        let summary = can_call_summary(&make_record());
        assert_eq!(summary["status"], "callable");
        assert!(summary["blockers"].as_array().unwrap().is_empty());
    }

    #[test]
    fn can_call_summary_inactive_is_not_callable() {
        let mut record = make_record();
        record.status = ToolStatus::Deregistered;
        let summary = can_call_summary(&record);
        assert_eq!(summary["status"], "not_callable");
        let blockers = summary["blockers"].as_array().unwrap();
        assert!(blockers.iter().any(|b| b == "tool is not active in the registry"));
    }

    #[test]
    fn can_call_summary_auth_is_conditional() {
        let mut record = make_record();
        record.has_auth = true;
        let summary = can_call_summary(&record);
        assert_eq!(summary["status"], "conditional");
        let reqs = summary["requirements"].as_array().unwrap();
        assert!(reqs.iter().any(|r| r == "manifest declares authentication requirements"));
    }

    #[test]
    fn can_call_summary_gated_is_conditional() {
        let mut record = make_record();
        // A non-zero, non-open predicate -> "custom" -> gated.
        record.access_predicate = Some("0x000000000000000000000000000000000000dEaD".to_string());
        let summary = can_call_summary(&record);
        assert_eq!(summary["status"], "conditional");
        let reqs = summary["requirements"].as_array().unwrap();
        assert!(reqs.iter().any(|r| r == "access predicate must approve the caller wallet"));
    }

    #[test]
    fn can_call_summary_x402_adds_requirement() {
        let mut record = make_record();
        record.has_x402 = true;
        let summary = can_call_summary(&record);
        // x402 alone (open access, no auth) is still callable but lists the requirement.
        assert_eq!(summary["status"], "callable");
        let reqs = summary["requirements"].as_array().unwrap();
        assert!(reqs.iter().any(|r| r == "x402 payment required or accepted"));
    }

    // ---- build_report ----

    #[test]
    fn build_report_verified_alive() {
        let record = make_record();
        let report = build_report(base_chain(), 1, &record, Some(200));
        assert_eq!(report.chain_id, 8453);
        assert_eq!(report.chain_name, "Base");
        assert_eq!(report.tool_id, 1);
        assert!(report.onchain_found);
        assert_eq!(report.status, "active");
        assert_eq!(report.manifest_status, "verified");
        assert!(report.hash_match);
        assert_eq!(report.endpoint_alive, Some(true));
        assert_eq!(report.endpoint_liveness, EndpointLiveness::Alive);
        assert_eq!(report.endpoint_http_status, Some(200));
        assert_eq!(report.can_call["status"], "callable");
    }

    #[test]
    fn build_report_dead_endpoint() {
        let record = make_record();
        let report = build_report(base_chain(), 1, &record, None);
        assert_eq!(report.endpoint_alive, Some(false));
        assert_eq!(report.endpoint_liveness, EndpointLiveness::Dead);
        assert_eq!(report.endpoint_http_status, None);
    }

    #[test]
    fn build_report_no_endpoint_is_unknown() {
        let mut record = make_record();
        record.endpoint = None;
        let report = build_report(base_chain(), 1, &record, None);
        assert_eq!(report.endpoint_alive, None);
        assert_eq!(report.endpoint_liveness, EndpointLiveness::Unknown);
    }

    #[test]
    fn build_report_hash_mismatch_sets_false() {
        let mut record = make_record();
        record.manifest_hash = Some("0xaaa".to_string());
        record.computed_manifest_hash = Some("0xbbb".to_string());
        record.manifest_status = ManifestStatus::HashMismatch;
        let report = build_report(base_chain(), 1, &record, Some(200));
        assert!(!report.hash_match);
        assert_eq!(report.manifest_status, "hash_mismatch");
    }

    // ---- build_not_found_report ----

    #[test]
    fn build_not_found_report_deregistered() {
        let report = build_not_found_report(
            base_chain(),
            7,
            ToolStatus::Deregistered,
            Some("ToolIsDeregistered(7)".to_string()),
        );
        assert!(!report.onchain_found);
        assert_eq!(report.status, "deregistered");
        assert_eq!(report.manifest_status, "unchecked");
        assert!(!report.hash_match);
        assert_eq!(report.endpoint_alive, None);
        assert_eq!(report.can_call["status"], "not_callable");
        assert_eq!(report.error.as_deref(), Some("ToolIsDeregistered(7)"));
    }

    // ---- deregistered_or_read_error ----

    #[test]
    fn classify_error_deregistered() {
        assert!(matches!(
            deregistered_or_read_error("execution reverted: ToolIsDeregistered(3)"),
            ToolStatus::Deregistered
        ));
        assert!(matches!(
            deregistered_or_read_error("0x0bf47976"),
            ToolStatus::Deregistered
        ));
    }

    #[test]
    fn classify_error_not_found_is_read_error() {
        assert!(matches!(
            deregistered_or_read_error("ToolNotFound(99)"),
            ToolStatus::ReadError
        ));
    }

    #[test]
    fn classify_error_generic_is_read_error() {
        assert!(matches!(
            deregistered_or_read_error("connection refused"),
            ToolStatus::ReadError
        ));
    }
}

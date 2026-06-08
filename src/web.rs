use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::RwLock;

use crate::cache::save_snapshot;
use crate::events::{apply_event_history, apply_event_history_multi_chain, backfill_all_events, backfill_events_legacy};
use crate::indexer::{access_label, sync_all_chains, sync_registry_legacy};
use crate::storage::{event_count, save_events_db, save_snapshot_db};
use crate::types::{CHAINS, ManifestStatus, Snapshot, ToolRecord, ToolStatus};

const INDEX_HTML: &str = include_str!("../web/index.html");
const TWEAKS_PANEL_JSX: &str = include_str!("../web/tweaks-panel.jsx");
const CON_HELPERS_JSX: &str = include_str!("../web/con-helpers.jsx");
const CAT_DOMAINS_JSX: &str = include_str!("../web/cat-domains.jsx");
const CAT_HELPERS_JSX: &str = include_str!("../web/cat-helpers.jsx");
const CAT_CARD_JSX: &str = include_str!("../web/cat-card.jsx");
const CAT_APP_JSX: &str = include_str!("../web/cat-app.jsx");
const FALLBACK_REGISTRY_DATA_JS: &str = include_str!("../web/registry-data.js");

#[derive(Clone)]
pub struct AppState {
    pub snapshot: Arc<RwLock<Snapshot>>,
    pub rpc_url: String,
    pub cache_path: String,
    pub db_path: String,
}

#[derive(Debug, Deserialize)]
pub struct ResolveRequest {
    pub query: Option<String>,
    pub status: Option<String>,
    pub access: Option<String>,
    pub manifest_status: Option<String>,
    pub x402: Option<bool>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct CanCallRequest {
    pub wallet: Option<String>,
    pub budget_usdc: Option<f64>,
    pub allow_x402: Option<bool>,
    pub has_auth: Option<bool>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/registry-data.js", get(registry_data_js))
        .route("/tweaks-panel.jsx", get(tweaks_panel_jsx))
        .route("/con-helpers.jsx", get(con_helpers_jsx))
        .route("/cat-domains.jsx", get(cat_domains_jsx))
        .route("/cat-helpers.jsx", get(cat_helpers_jsx))
        .route("/cat-card.jsx", get(cat_card_jsx))
        .route("/cat-app.jsx", get(cat_app_jsx))
        .route("/tools/{tool_id}", get(tool_page))
        .route("/api/tools", get(api_tools))
        .route("/api/tools/{tool_id}", get(api_tool))
        .route("/api/tools/{tool_id}/can_call", post(api_can_call))
        .route("/api/resolve", get(resolve_help).post(api_resolve))
        .route("/api/stats", get(api_stats))
        .route("/api/sync", post(api_sync))
        .route("/llms.txt", get(llms_txt))
        .route("/openapi.json", get(openapi_json))
        .with_state(state)
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn registry_data_js(State(state): State<AppState>) -> Response {
    let snapshot = state.snapshot.read().await;
    if snapshot.tools.is_empty() {
        return javascript_response(FALLBACK_REGISTRY_DATA_JS.to_string());
    }
    match serde_json::to_string(&frontend_registry(&snapshot)) {
        Ok(registry) => javascript_response(format!("window.REGISTRY = {registry};")),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn tweaks_panel_jsx() -> Response {
    babel_response(TWEAKS_PANEL_JSX)
}

async fn con_helpers_jsx() -> Response {
    babel_response(CON_HELPERS_JSX)
}

async fn cat_domains_jsx() -> Response {
    babel_response(CAT_DOMAINS_JSX)
}

async fn cat_helpers_jsx() -> Response {
    babel_response(CAT_HELPERS_JSX)
}

async fn cat_card_jsx() -> Response {
    babel_response(CAT_CARD_JSX)
}

async fn cat_app_jsx() -> Response {
    babel_response(CAT_APP_JSX)
}

async fn tool_page(Path(tool_id): Path<u64>, State(state): State<AppState>) -> Response {
    let snapshot = state.snapshot.read().await;
    if snapshot.tools.iter().any(|tool| tool.tool_id == tool_id) {
        Html(INDEX_HTML).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

async fn api_tools(State(state): State<AppState>) -> Json<Vec<ToolRecord>> {
    Json(state.snapshot.read().await.tools.clone())
}

async fn api_tool(Path(tool_id): Path<u64>, State(state): State<AppState>) -> Response {
    let snapshot = state.snapshot.read().await;
    match snapshot.tools.iter().find(|tool| tool.tool_id == tool_id) {
        Some(tool) => Json(tool).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn api_can_call(
    Path(tool_id): Path<u64>,
    State(state): State<AppState>,
    Json(request): Json<CanCallRequest>,
) -> Response {
    let snapshot = state.snapshot.read().await;
    match snapshot.tools.iter().find(|tool| tool.tool_id == tool_id) {
        Some(tool) => Json(can_call_plan(tool, &request)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn resolve_help() -> Json<Value> {
    Json(json!({
        "method": "POST",
        "endpoint": "/api/resolve",
        "body": {
            "query": "wallet risk",
            "status": "active",
            "access": "open",
            "manifest_status": "verified",
            "x402": true,
            "limit": 5
        }
    }))
}

async fn api_resolve(
    State(state): State<AppState>,
    Json(request): Json<ResolveRequest>,
) -> Json<Value> {
    let snapshot = state.snapshot.read().await;
    let mut tools = snapshot
        .tools
        .iter()
        .filter(|tool| resolve_matches(tool, &request))
        .map(|tool| {
            json!({
                "score": resolve_score(tool, request.query.as_deref()),
                "invocation": invocation_hint(tool),
                "tool": tool,
            })
        })
        .collect::<Vec<_>>();

    tools.sort_by(|a, b| b["score"].as_u64().cmp(&a["score"].as_u64()));
    tools.truncate(request.limit.unwrap_or(10).min(50));

    Json(json!({
        "query": request.query,
        "filters": {
            "status": request.status,
            "access": request.access,
            "manifestStatus": request.manifest_status,
            "x402": request.x402,
        },
        "count": tools.len(),
        "tools": tools,
    }))
}

async fn api_stats(State(state): State<AppState>) -> Json<Value> {
    let snapshot = state.snapshot.read().await;
    let events = event_count(&state.db_path).unwrap_or(0);
    let chains_summary = snapshot.chains_summary();
    Json(json!({
        "chainId": snapshot.chain_id,
        "registry": snapshot.registry,
        "toolCount": snapshot.tool_count,
        "syncedAt": snapshot.synced_at,
        "storedEvents": events,
        "chains": chains_summary,
        "stats": snapshot.stats(),
    }))
}

async fn api_sync(State(state): State<AppState>) -> Response {
    // Check if we should use legacy single-chain sync or multi-chain sync
    if state.rpc_url != crate::types::DEFAULT_RPC_URL {
        // Legacy mode: use provided RPC URL
        match sync_registry_legacy(&state.rpc_url).await {
            Ok(mut snapshot) => {
                let events = backfill_events_legacy().await.unwrap_or_default();
                if let Err(err) = apply_event_history(&mut snapshot, &events).await {
                    return (StatusCode::BAD_GATEWAY, err.to_string()).into_response();
                }
                if let Err(err) = save_snapshot(&state.cache_path, &snapshot) {
                    return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
                }
                if let Err(err) = save_snapshot_db(&state.db_path, &snapshot) {
                    return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
                }
                if let Err(err) = save_events_db(&state.db_path, &events) {
                    return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
                }
                let stats = snapshot.stats();
                let events = event_count(&state.db_path).unwrap_or(0);
                *state.snapshot.write().await = snapshot;
                Html(format!(
                    "<strong>Synced.</strong> {} active, {} deregistered, {} verified manifests, {} stored events.<script>setTimeout(()=>location.reload(),700)</script>",
                    stats.active, stats.deregistered, stats.verified_manifests, events
                ))
                .into_response()
            }
            Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
        }
    } else {
        // Multi-chain mode
        match sync_all_chains().await {
            Ok(mut multi_snapshot) => {
                let events = backfill_all_events().await.unwrap_or_default();
                if let Err(err) = apply_event_history_multi_chain(&mut multi_snapshot, &events).await {
                    return (StatusCode::BAD_GATEWAY, err.to_string()).into_response();
                }
                let snapshot: Snapshot = multi_snapshot.into();
                if let Err(err) = save_snapshot(&state.cache_path, &snapshot) {
                    return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
                }
                if let Err(err) = save_snapshot_db(&state.db_path, &snapshot) {
                    return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
                }
                if let Err(err) = save_events_db(&state.db_path, &events) {
                    return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
                }
                let stats = snapshot.stats();
                let chains_summary = snapshot.chains_summary();
                let events = event_count(&state.db_path).unwrap_or(0);
                let snapshot_clone = snapshot.clone();
                *state.snapshot.write().await = snapshot_clone;
                Html(format!(
                    "<strong>Synced.</strong> {} tools from {} chains: {} active, {} deregistered, {} verified manifests, {} stored events.<script>setTimeout(()=>location.reload(),700)</script>",
                    stats.total_ids, chains_summary.len(), stats.active, stats.deregistered, stats.verified_manifests, events
                ))
                .into_response()
            }
            Err(err) => (StatusCode::BAD_GATEWAY, err.to_string()).into_response(),
        }
    }
}

async fn llms_txt(State(state): State<AppState>) -> Response {
    let snapshot = state.snapshot.read().await;
    let chains_line = snapshot.chains_summary().iter()
        .map(|(cid, name, count)| format!("{} ({}, {} tools)", name, cid, count))
        .collect::<Vec<_>>().join(", ");
    let body = format!(
        "# Agent Tool Index\n\nVisual and agent-readable index for ERC-8257 tools across Ethereum, Base, and Abstract.\n\n## Registry\n\n- Registry address: {} (same on all chains)\n- Chains: {}\n- Synced at: {}\n- Tool count: {}\n\n## API\n\n- GET /api/tools - list all indexed tools\n- GET /api/tools/{{tool_id}} - single tool record\n- POST /api/tools/{{tool_id}}/can_call - plan whether a caller can invoke a tool\n- POST /api/resolve - resolve intent/filter criteria to candidate tools\n- GET /api/stats - index statistics\n- GET /openapi.json - OpenAPI 3.1 schema\n\n## Tool Records\n\nEach tool record includes: chain_id, chain_name, status, creator, metadata URI, access predicate,\npredicate_type, manifest verification status (JCS keccak256 hash), x402 detection, endpoint,\ntags, inputs, outputs, pricing, and checked_at timestamps.\n\n## Resolve\n\nPOST /api/resolve accepts: query, status, access, manifest_status, x402, limit.\nReturns scored candidates with invocation hints.\n\n## Call Planning\n\nPOST /api/tools/{{tool_id}}/can_call accepts: wallet, budget_usdc, allow_x402, has_auth.\nReturns callable/conditional/not_callable with requirements, blockers, and steps.\n",
        snapshot.registry, chains_line, snapshot.synced_at, snapshot.tool_count
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    (headers, body).into_response()
}

async fn openapi_json() -> Json<Value> {
    Json(json!({
        "openapi": "3.1.0",
        "info": {
            "title": "ERC-8257 Index",
            "version": "0.1.0",
            "description": "Agent-first index for ERC-8257 ToolRegistry records on Base."
        },
        "paths": {
            "/api/tools": { "get": { "summary": "List indexed ERC-8257 tools", "responses": { "200": json_response("ToolRecordList") } } },
            "/api/tools/{tool_id}": {
                "get": {
                    "summary": "Get one indexed ERC-8257 tool",
                    "parameters": [{ "name": "tool_id", "in": "path", "required": true, "schema": { "type": "integer", "minimum": 1 } }],
                    "responses": { "200": json_response("ToolRecord"), "404": { "description": "Tool not found" } }
                }
            },
            "/api/tools/{tool_id}/can_call": {
                "post": {
                    "summary": "Plan whether a caller can invoke a tool",
                    "parameters": [{ "name": "tool_id", "in": "path", "required": true, "schema": { "type": "integer", "minimum": 1 } }],
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CanCallRequest" } } } },
                    "responses": { "200": json_response("CanCallResponse"), "404": { "description": "Tool not found" } }
                }
            },
            "/api/resolve": {
                "get": {
                    "summary": "Show resolve endpoint usage",
                    "responses": { "200": { "description": "Resolver usage example" } }
                },
                "post": {
                    "summary": "Resolve agent intent to candidate tools",
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ResolveRequest" } } } },
                    "responses": { "200": json_response("ResolveResponse") }
                }
            },
            "/api/stats": { "get": { "summary": "Get index statistics", "responses": { "200": json_response("StatsResponse") } } },
            "/api/sync": { "post": { "summary": "Run a live Base registry sync", "responses": { "200": { "description": "Sync completed" }, "502": { "description": "RPC or sync failure" } } } }
        },
        "components": { "schemas": openapi_schemas() }
    }))
}

fn json_response(schema: &str) -> Value {
    json!({
        "description": "OK",
        "content": { "application/json": { "schema": { "$ref": format!("#/components/schemas/{schema}") } } }
    })
}

fn openapi_schemas() -> Value {
    json!({
        "ToolRecordList": { "type": "array", "items": { "$ref": "#/components/schemas/ToolRecord" } },
        "ToolRecord": {
            "type": "object",
            "required": ["chain_id", "registry", "tool_id", "status", "manifest_status", "tags", "has_x402", "has_auth", "checked_at"],
            "properties": {
                "chain_id": { "type": "integer", "example": 8453 },
                "registry": { "type": "string", "format": "address" },
                "tool_id": { "type": "integer", "minimum": 1 },
                "status": { "type": "string", "enum": ["active", "deregistered", "read_error"] },
                "creator": { "type": ["string", "null"], "format": "address" },
                "metadata_uri": { "type": ["string", "null"], "format": "uri" },
                "manifest_hash": { "type": ["string", "null"] },
                "access_predicate": { "type": ["string", "null"], "format": "address" },
                "manifest_status": { "type": "string", "enum": ["unchecked", "verified", "hash_mismatch", "fetch_error", "parse_error"] },
                "computed_manifest_hash": { "type": ["string", "null"] },
                "name": { "type": ["string", "null"] },
                "description": { "type": ["string", "null"] },
                "endpoint": { "type": ["string", "null"], "format": "uri" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "has_x402": { "type": "boolean" },
                "has_auth": { "type": "boolean" },
                "error": { "type": ["string", "null"] },
                "manifest": { "type": ["object", "null"] },
                "checked_at": { "type": "string", "format": "date-time" }
            }
        },
        "ResolveRequest": {
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Intent or search text." },
                "status": { "type": "string", "enum": ["active", "deregistered", "read_error"] },
                "access": { "type": "string", "enum": ["open", "predicate", "unknown"] },
                "manifest_status": { "type": "string", "enum": ["unchecked", "verified", "hash_mismatch", "fetch_error", "parse_error"] },
                "x402": { "type": "boolean" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 }
            }
        },
        "CanCallRequest": {
            "type": "object",
            "properties": {
                "wallet": { "type": "string", "description": "Caller wallet address used for predicate evaluation planning." },
                "budget_usdc": { "type": "number", "minimum": 0 },
                "allow_x402": { "type": "boolean", "default": true },
                "has_auth": { "type": "boolean", "description": "Whether the caller can provide auth/SIWE if required." }
            }
        },
        "CanCallResponse": {
            "type": "object",
            "properties": {
                "toolId": { "type": "integer" },
                "status": { "type": "string", "enum": ["callable", "conditional", "not_callable"] },
                "endpoint": { "type": ["string", "null"] },
                "method": { "type": "string" },
                "priceUsdc": { "type": ["number", "null"] },
                "requirements": { "type": "array", "items": { "type": "string" } },
                "blockers": { "type": "array", "items": { "type": "string" } },
                "steps": { "type": "array", "items": { "type": "string" } }
            }
        },
        "ResolveResponse": {
            "type": "object",
            "properties": {
                "query": { "type": ["string", "null"] },
                "filters": { "type": "object" },
                "count": { "type": "integer" },
                "tools": { "type": "array", "items": { "type": "object", "properties": { "score": { "type": "integer" }, "invocation": { "type": "string" }, "tool": { "$ref": "#/components/schemas/ToolRecord" } } } }
            }
        },
        "StatsResponse": {
            "type": "object",
            "properties": {
                "chainId": { "type": "integer" },
                "registry": { "type": "string" },
                "toolCount": { "type": "integer" },
                "syncedAt": { "type": "string", "format": "date-time" },
                "storedEvents": { "type": "integer" },
                "stats": { "type": "object" }
            }
        }
    })
}

pub fn frontend_registry(snapshot: &Snapshot) -> Value {
    let chains_summary: Vec<Value> = snapshot.chains_summary().iter().map(|(cid, name, count)| {
        json!({ "chain_id": cid, "name": name, "tool_count": count })
    }).collect();
    json!({
        "chain_id": snapshot.chain_id,
        "registry": snapshot.registry,
        "tool_count": snapshot.tool_count,
        "synced_at": snapshot.synced_at,
        "chains": chains_summary,
        "tools": snapshot.tools.iter().map(frontend_tool).collect::<Vec<_>>(),
    })
}

pub fn chain_name_for(chain_id: u64) -> &'static str {
    CHAINS.iter().find(|c| c.chain_id == chain_id).map(|c| c.name).unwrap_or("Unknown")
}

pub fn frontend_tool(tool: &ToolRecord) -> Value {
    json!({
        "id": tool.tool_id,
        "chain_id": tool.chain_id,
        "chain_name": chain_name_for(tool.chain_id),
        "status": status_text(tool),
        "name": frontend_tool_name(tool),
        "description": tool.description.as_deref().unwrap_or("No description published in the manifest."),
        "endpoint": tool.endpoint,
        "creator": tool.creator,
        "metadata_uri": tool.metadata_uri,
        "manifest_hash": tool.manifest_hash,
        "computed_hash": tool.computed_manifest_hash,
        "manifest_status": manifest_text(tool),
        "access": frontend_access_label(tool),
        "access_predicate": tool.access_predicate,
        "predicate_type": &tool.predicate_type,
        "access_reqs": access_requirements(tool),
        "has_x402": tool.has_x402,
        "has_auth": tool.has_auth,
        "price_usdc": pricing_amount_usdc(tool),
        "tags": tool.tags,
        "inputs": manifest_inputs(tool),
        "outputs": manifest_outputs(tool),
        "erc": erc_label(tool),
        "checked_at": tool.checked_at,
    })
}

pub fn frontend_tool_name(tool: &ToolRecord) -> String {
    tool.name
        .as_deref()
        .or(tool.metadata_uri.as_deref())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Tool #{}", tool.tool_id))
}

pub fn frontend_access_label(tool: &ToolRecord) -> &'static str {
    match access_label(tool) {
        "predicate" => "gated",
        other => other,
    }
}

fn manifest_inputs(tool: &ToolRecord) -> Vec<Value> {
    let Some(manifest) = tool.manifest.as_ref() else {
        return Vec::new();
    };

    for key in ["inputs", "parameters"] {
        if let Some(inputs) = manifest.get(key).and_then(Value::as_array) {
            let normalized = inputs
                .iter()
                .enumerate()
                .filter_map(|(index, field)| normalize_input_field(field, index))
                .collect::<Vec<_>>();
            if !normalized.is_empty() {
                return normalized;
            }
        }
    }

    for key in [
        "inputSchema",
        "input_schema",
        "requestSchema",
        "request_schema",
        "parameters",
    ] {
        if let Some(inputs) = schema_inputs(manifest.get(key)) {
            return inputs;
        }
    }

    Vec::new()
}

fn normalize_input_field(field: &Value, index: usize) -> Option<Value> {
    match field {
        Value::Object(map) => {
            let name = map
                .get("name")
                .or_else(|| map.get("key"))
                .or_else(|| map.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("input{}", index + 1));
            Some(json!({
                "name": name,
                "type": map
                    .get("type")
                    .cloned()
                    .unwrap_or_else(|| Value::String("object".to_string())),
                "required": map
                    .get("required")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                "description": map
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            }))
        }
        Value::String(name) => Some(json!({
            "name": name,
            "type": "string",
            "required": true,
            "description": "",
        })),
        _ => None,
    }
}

fn schema_inputs(schema: Option<&Value>) -> Option<Vec<Value>> {
    let schema = schema?;
    let properties = schema.get("properties")?.as_object()?;
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(
        properties
            .iter()
            .map(|(name, spec)| {
                json!({
                    "name": name,
                    "type": schema_type(spec),
                    "required": required.iter().any(|item| item == name),
                    "description": spec
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                })
            })
            .collect(),
    )
}

fn schema_type(spec: &Value) -> Value {
    if let Some(field_type) = spec.get("type") {
        return field_type.clone();
    }
    if let Some(items) = spec.get("anyOf").and_then(Value::as_array) {
        let types = items
            .iter()
            .filter_map(|item| item.get("type"))
            .cloned()
            .collect::<Vec<_>>();
        if !types.is_empty() {
            return Value::Array(types);
        }
    }
    Value::String("object".to_string())
}

fn manifest_outputs(tool: &ToolRecord) -> Vec<String> {
    let Some(manifest) = tool.manifest.as_ref() else {
        return Vec::new();
    };

    for key in ["outputs", "returns"] {
        if let Some(outputs) = manifest.get(key).and_then(Value::as_array) {
            let normalized = outputs.iter().filter_map(output_name).collect::<Vec<_>>();
            if !normalized.is_empty() {
                return normalized;
            }
        }
    }

    for key in [
        "outputSchema",
        "output_schema",
        "responseSchema",
        "response_schema",
    ] {
        if let Some(outputs) = schema_outputs(manifest.get(key)) {
            return outputs;
        }
    }

    Vec::new()
}

fn output_name(output: &Value) -> Option<String> {
    match output {
        Value::String(name) => Some(name.to_string()),
        Value::Object(map) => map
            .get("name")
            .or_else(|| map.get("key"))
            .or_else(|| map.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn schema_outputs(schema: Option<&Value>) -> Option<Vec<String>> {
    let schema = schema?;
    Some(
        schema
            .get("properties")?
            .as_object()?
            .keys()
            .map(|key| key.to_string())
            .collect(),
    )
}

fn access_requirements(tool: &ToolRecord) -> Vec<Value> {
    let Some(manifest) = tool.manifest.as_ref() else {
        return Vec::new();
    };

    let mut requirements = Vec::new();
    for key in ["accessRequirements", "access_requirements"] {
        collect_access_requirements(manifest.get(key), &mut requirements);
    }
    for key in ["access", "accessPredicate", "access_predicate"] {
        if let Some(access) = manifest.get(key) {
            collect_access_requirements(access.get("requirements"), &mut requirements);
            collect_access_requirements(access.get("rules"), &mut requirements);
            collect_access_requirements(Some(access), &mut requirements);
        }
    }
    requirements
}

fn collect_access_requirements(value: Option<&Value>, requirements: &mut Vec<Value>) {
    match value {
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(requirement) = normalize_access_requirement(item) {
                    requirements.push(requirement);
                }
            }
        }
        Some(value) => {
            if let Some(requirement) = normalize_access_requirement(value) {
                requirements.push(requirement);
            }
        }
        None => {}
    }
}

fn normalize_access_requirement(value: &Value) -> Option<Value> {
    match value {
        Value::String(label) => Some(json!({ "label": label, "kind": "" })),
        Value::Object(map) => {
            let label = map
                .get("label")
                .or_else(|| map.get("name"))
                .or_else(|| map.get("description"))
                .and_then(Value::as_str)?;
            let kind = map
                .get("kind")
                .or_else(|| map.get("type"))
                .or_else(|| map.get("selector"))
                .and_then(Value::as_str)
                .unwrap_or("");
            Some(json!({ "label": label, "kind": kind }))
        }
        _ => None,
    }
}

fn erc_label(tool: &ToolRecord) -> String {
    let Some(manifest) = tool.manifest.as_ref() else {
        return "draft".to_string();
    };
    for key in ["erc", "standard", "spec", "version"] {
        if let Some(label) = manifest.get(key).and_then(Value::as_str) {
            return label
                .trim_start_matches("ERC-")
                .trim_start_matches("erc-")
                .to_string();
        }
    }
    if tool
        .tags
        .iter()
        .any(|tag| tag.eq_ignore_ascii_case("erc-8257"))
    {
        "8257".to_string()
    } else {
        "draft".to_string()
    }
}

fn javascript_response(body: String) -> Response {
    text_response("application/javascript; charset=utf-8", body)
}

fn babel_response(body: &'static str) -> Response {
    text_response("text/babel; charset=utf-8", body.to_string())
}

fn text_response(content_type: &'static str, body: String) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static(content_type));
    (headers, body).into_response()
}

pub fn fallback_snapshot() -> Result<Snapshot> {
    let registry_json = FALLBACK_REGISTRY_DATA_JS
        .trim()
        .strip_prefix("window.REGISTRY = ")
        .unwrap_or(FALLBACK_REGISTRY_DATA_JS.trim())
        .trim_end_matches(';');
    let registry: Value = serde_json::from_str(registry_json)?;
    let tools = registry
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .map(fallback_tool_record)
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();

    Ok(Snapshot {
        chain_id: registry
            .get("chain_id")
            .and_then(Value::as_u64)
            .unwrap_or(crate::types::BASE_CHAIN_ID),
        registry: registry
            .get("registry")
            .and_then(Value::as_str)
            .unwrap_or(crate::types::BASE_REGISTRY)
            .to_string(),
        tool_count: registry
            .get("tool_count")
            .and_then(Value::as_u64)
            .unwrap_or(tools.len() as u64),
        synced_at: date_time_field(&registry, "synced_at").unwrap_or_else(Utc::now),
        tools,
    })
}

fn fallback_tool_record(tool: &Value) -> Result<ToolRecord> {
    let tool_id = tool.get("id").and_then(Value::as_u64).unwrap_or_default();
    let checked_at = date_time_field(tool, "checked_at").unwrap_or_else(Utc::now);
    Ok(ToolRecord {
        chain_id: tool
            .get("chain_id")
            .and_then(Value::as_u64)
            .unwrap_or(crate::types::BASE_CHAIN_ID),
        registry: tool
            .get("registry")
            .and_then(Value::as_str)
            .unwrap_or(crate::types::BASE_REGISTRY)
            .to_string(),
        tool_id,
        status: parse_tool_status(tool.get("status").and_then(Value::as_str)),
        creator: string_field(tool, "creator"),
        metadata_uri: string_field(tool, "metadata_uri"),
        manifest_hash: string_field(tool, "manifest_hash"),
        access_predicate: string_field(tool, "access_predicate"),
        predicate_type: string_field(tool, "predicate_type").unwrap_or("unknown".to_string()),
        manifest_status: parse_manifest_status(tool.get("manifest_status").and_then(Value::as_str)),
        computed_manifest_hash: string_field(tool, "computed_hash"),
        name: string_field(tool, "name"),
        description: string_field(tool, "description"),
        endpoint: string_field(tool, "endpoint"),
        tags: string_array_field(tool, "tags"),
        has_x402: tool
            .get("has_x402")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        has_auth: tool
            .get("has_auth")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        error: None,
        manifest: Some(fallback_manifest(tool)),
        checked_at,
    })
}

fn fallback_manifest(tool: &Value) -> Value {
    let mut manifest = serde_json::Map::new();
    for key in [
        "name",
        "description",
        "endpoint",
        "tags",
        "inputs",
        "outputs",
        "erc",
    ] {
        if let Some(value) = tool.get(key) {
            manifest.insert(key.to_string(), value.clone());
        }
    }
    if let Some(access_reqs) = tool.get("access_reqs") {
        manifest.insert("accessRequirements".to_string(), access_reqs.clone());
    }
    if tool
        .get("has_auth")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        manifest.insert("authentication".to_string(), json!({ "type": "siwe" }));
    }
    if let Some(price) = tool.get("price_usdc").and_then(Value::as_f64) {
        manifest.insert("pricing".to_string(), json!([{ "amount": price }]));
    }
    Value::Object(manifest)
}

fn parse_tool_status(status: Option<&str>) -> ToolStatus {
    match status {
        Some("active") => ToolStatus::Active,
        Some("deregistered") => ToolStatus::Deregistered,
        Some("read_error") => ToolStatus::ReadError,
        _ => ToolStatus::ReadError,
    }
}

fn parse_manifest_status(status: Option<&str>) -> ManifestStatus {
    match status {
        Some("verified") => ManifestStatus::Verified,
        Some("hash_mismatch") => ManifestStatus::HashMismatch,
        Some("fetch_error") => ManifestStatus::FetchError,
        Some("parse_error") => ManifestStatus::ParseError,
        _ => ManifestStatus::Unchecked,
    }
}

fn date_time_field(value: &Value, key: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.get(key)?.as_str()?)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

fn string_array_field(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub async fn serve(addr: &str, state: AppState) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}


fn resolve_matches(tool: &ToolRecord, request: &ResolveRequest) -> bool {
    if let Some(status) = request.status.as_deref() {
        if status != status_text(tool) {
            return false;
        }
    }
    if let Some(access) = request.access.as_deref() {
        if access != access_label(tool) {
            return false;
        }
    }
    if let Some(manifest_status) = request.manifest_status.as_deref() {
        if manifest_status != manifest_text(tool) {
            return false;
        }
    }
    if let Some(x402) = request.x402 {
        if x402 != tool.has_x402 {
            return false;
        }
    }
    match request.query.as_deref() {
        Some(query) if !query.trim().is_empty() => query
            .split_whitespace()
            .all(|term| searchable_text(tool).contains(&term.to_ascii_lowercase())),
        _ => true,
    }
}

fn resolve_score(tool: &ToolRecord, query: Option<&str>) -> u64 {
    let text = searchable_text(tool);
    let mut score = if matches!(tool.status, ToolStatus::Active) {
        10
    } else {
        0
    };
    if matches!(tool.manifest_status, ManifestStatus::Verified) {
        score += 5;
    }
    if let Some(query) = query {
        score += query
            .split_whitespace()
            .filter(|term| text.contains(&term.to_ascii_lowercase()))
            .count() as u64;
    }
    score
}

fn searchable_text(tool: &ToolRecord) -> String {
    [
        tool.name.as_deref(),
        tool.description.as_deref(),
        tool.endpoint.as_deref(),
        tool.metadata_uri.as_deref(),
        tool.creator.as_deref(),
        Some(&tool.tags.join(" ")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase()
}

fn invocation_hint(tool: &ToolRecord) -> String {
    if !matches!(tool.status, ToolStatus::Active) {
        return "Not callable: this tool ID is not active in the registry.".to_string();
    }
    let endpoint = tool
        .endpoint
        .as_deref()
        .unwrap_or("manifest has no endpoint");
    match (access_label(tool), tool.has_x402, tool.has_auth) {
        ("predicate", true, _) => format!(
            "Call {endpoint} after satisfying predicate access and x402 payment requirements from the manifest."
        ),
        ("predicate", false, _) => {
            format!("Call {endpoint} with wallet/auth proof required by the access predicate.")
        }
        (_, true, _) => {
            format!("Call {endpoint}; expect HTTP 402/x402 payment requirements before success.")
        }
        (_, _, true) => format!("Call {endpoint}; manifest declares authentication requirements."),
        _ => format!("Call {endpoint} directly with JSON matching the input schema."),
    }
}

fn can_call_plan(tool: &ToolRecord, request: &CanCallRequest) -> Value {
    let mut requirements = Vec::new();
    let mut blockers = Vec::new();
    let price = pricing_amount_usdc(tool);

    if !matches!(tool.status, ToolStatus::Active) {
        blockers.push("tool is not active in the registry".to_string());
    }
    if access_label(tool) == "predicate" {
        requirements.push("access predicate must approve the caller wallet".to_string());
        if request.wallet.is_none() {
            blockers.push("wallet is required to evaluate predicate access".to_string());
        }
    }
    if tool.has_auth && request.has_auth != Some(true) {
        requirements.push("manifest declares authentication requirements".to_string());
    }
    if tool.has_x402 {
        requirements.push("x402 payment required or accepted".to_string());
        if request.allow_x402 == Some(false) {
            blockers.push("caller does not allow x402 payments".to_string());
        }
        if let (Some(budget), Some(price)) = (request.budget_usdc, price) {
            if budget < price {
                blockers.push(format!(
                    "budget {budget:.6} USDC is below price {price:.6} USDC"
                ));
            }
        }
    }

    let status = if !blockers.is_empty() {
        "not_callable"
    } else if access_label(tool) == "predicate" || tool.has_auth {
        "conditional"
    } else {
        "callable"
    };

    json!({
        "toolId": tool.tool_id,
        "status": status,
        "endpoint": tool.endpoint,
        "method": "POST",
        "priceUsdc": price,
        "requirements": requirements,
        "blockers": blockers,
        "steps": invocation_steps(tool),
    })
}

fn invocation_steps(tool: &ToolRecord) -> Vec<String> {
    let mut steps = Vec::new();
    steps.push("Fetch and validate the tool manifest before calling.".to_string());
    if access_label(tool) == "predicate" || tool.has_auth {
        steps.push(
            "Prepare wallet authentication or predicate proof required by the manifest."
                .to_string(),
        );
    }
    if tool.has_x402 {
        steps.push("Call endpoint, read HTTP 402 requirements, sign x402 payment, then retry with X-Payment.".to_string());
    }
    steps.push("POST JSON matching the manifest input schema to the endpoint.".to_string());
    steps
}

fn pricing_amount_usdc(tool: &ToolRecord) -> Option<f64> {
    let manifest = tool.manifest.as_ref()?;
    let pricing = manifest.get("pricing")?.as_array()?.first()?;
    let amount_value = pricing.get("amount")?;
    let amount = amount_value
        .as_f64()
        .or_else(|| amount_value.as_str()?.parse::<f64>().ok())?;
    Some(if amount > 1_000.0 {
        amount / 1_000_000.0
    } else {
        amount
    })
}

pub fn status_text(tool: &ToolRecord) -> &'static str {
    match tool.status {
        ToolStatus::Active => "active",
        ToolStatus::Deregistered => "deregistered",
        ToolStatus::ReadError => "read_error",
    }
}

fn manifest_text(tool: &ToolRecord) -> &'static str {
    match tool.manifest_status {
        ManifestStatus::Unchecked => "unchecked",
        ManifestStatus::Verified => "verified",
        ManifestStatus::HashMismatch => "hash_mismatch",
        ManifestStatus::FetchError => "fetch_error",
        ManifestStatus::ParseError => "parse_error",
    }
}



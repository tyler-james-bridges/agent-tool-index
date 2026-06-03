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
use crate::events::{apply_event_history, backfill_events};
use crate::indexer::{access_label, sync_registry};
use crate::storage::{event_count, save_events_db, save_snapshot_db};
use crate::types::{ManifestStatus, Snapshot, ToolRecord, ToolStatus};

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
    match snapshot.tools.iter().find(|tool| tool.tool_id == tool_id) {
        Some(tool) => Html(render_detail(tool)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
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
    Json(json!({
        "chainId": snapshot.chain_id,
        "registry": snapshot.registry,
        "toolCount": snapshot.tool_count,
        "syncedAt": snapshot.synced_at,
        "storedEvents": events,
        "stats": snapshot.stats(),
    }))
}

async fn api_sync(State(state): State<AppState>) -> Response {
    match sync_registry(&state.rpc_url).await {
        Ok(mut snapshot) => {
            let events = backfill_events().await.unwrap_or_default();
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
}

async fn llms_txt(State(state): State<AppState>) -> Response {
    let snapshot = state.snapshot.read().await;
    let body = format!(
        "# ERC-8257 Index\n\nThis service indexes ERC-8257 tools from Base ToolRegistry.\n\nRegistry: {}\nChain ID: {}\nSynced At: {}\n\nAgent endpoints:\n- GET /api/tools\n- GET /api/tools/{{tool_id}}\n- POST /api/tools/{{tool_id}}/can_call\n- POST /api/resolve\n- GET /api/stats\n- GET /openapi.json\n\nResolve accepts JSON fields: query, status, access, manifest_status, x402, limit. can_call accepts wallet, budget_usdc, allow_x402, has_auth. Tool records include status, creator, metadata URI, access predicate, manifest verification status, x402 detection, endpoint, tags, and raw manifest JSON when available.\n",
        snapshot.registry, snapshot.chain_id, snapshot.synced_at
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

fn frontend_registry(snapshot: &Snapshot) -> Value {
    json!({
        "chain_id": snapshot.chain_id,
        "registry": snapshot.registry,
        "tool_count": snapshot.tool_count,
        "synced_at": snapshot.synced_at,
        "tools": snapshot.tools.iter().map(frontend_tool).collect::<Vec<_>>(),
    })
}

fn frontend_tool(tool: &ToolRecord) -> Value {
    json!({
        "id": tool.tool_id,
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

fn frontend_tool_name(tool: &ToolRecord) -> String {
    tool.name
        .as_deref()
        .or(tool.metadata_uri.as_deref())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Tool #{}", tool.tool_id))
}

fn frontend_access_label(tool: &ToolRecord) -> &'static str {
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

fn render_detail(tool: &ToolRecord) -> String {
    let title = tool.name.as_deref().unwrap_or("unknown tool");
    let manifest_json = tool
        .manifest
        .as_ref()
        .and_then(|manifest| serde_json::to_string_pretty(manifest).ok())
        .unwrap_or_else(|| "null".to_string());
    format!(
        r##"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Tool #{id} / ERC-8257 Index</title>{style}</head><body><main><a class="back" href="/">Back to index</a><section class="hero detail"><p class="eyebrow">Tool #{id}</p><h1>{title}</h1><p>{description}</p><div class="actions"><a href="/api/tools/{id}">JSON</a>{metadata_link}{endpoint_link}</div></section><section class="detail-grid"><article><h2>Registry</h2><p><b>Status</b> {status_badge}</p><p><b>Access</b> {access_badge}</p><p><b>Creator</b> {creator}</p><p><b>Registry</b> {registry}</p><p><b>Chain</b> {chain}</p></article><article><h2>Manifest</h2><p><b>Status</b> {manifest_badge}</p><p><b>Onchain hash</b> <code>{manifest_hash}</code></p><p><b>Computed hash</b> <code>{computed_hash}</code></p><p><b>Checked</b> {checked}</p></article><article><h2>Agent Invocation</h2><p>{invocation}</p><p><b>Planner</b> <code>POST /api/tools/{id}/can_call</code></p><p><b>Tags</b> {tags}</p><p><b>Error</b> {error}</p></article></section><section class="raw"><h2>Raw Manifest</h2><pre>{manifest}</pre></section></main></body></html>"##,
        style = STYLE,
        id = tool.tool_id,
        title = html_escape::encode_text(title),
        description = html_escape::encode_text(
            tool.description
                .as_deref()
                .unwrap_or("No manifest description is available.")
        ),
        metadata_link = optional_link(tool.metadata_uri.as_deref(), "Metadata URI"),
        endpoint_link = optional_link(tool.endpoint.as_deref(), "Endpoint"),
        status_badge = badge(status_text(tool), status_text(tool)),
        access_badge = badge(access_label(tool), access_label(tool)),
        creator = html_escape::encode_text(tool.creator.as_deref().unwrap_or("unknown")),
        registry = html_escape::encode_text(&tool.registry),
        chain = tool.chain_id,
        manifest_badge = badge(manifest_text(tool), manifest_text(tool)),
        manifest_hash =
            html_escape::encode_text(tool.manifest_hash.as_deref().unwrap_or("unknown")),
        computed_hash =
            html_escape::encode_text(tool.computed_manifest_hash.as_deref().unwrap_or("unknown")),
        checked = tool.checked_at,
        invocation = html_escape::encode_text(&invocation_hint(tool)),
        tags = html_escape::encode_text(&tool.tags.join(", ")),
        error = html_escape::encode_text(tool.error.as_deref().unwrap_or("none")),
        manifest = html_escape::encode_text(&manifest_json),
    )
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

fn status_text(tool: &ToolRecord) -> &'static str {
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

fn badge(label: &str, class_name: &str) -> String {
    format!(
        r#"<span class="badge {}">{}</span>"#,
        html_escape::encode_double_quoted_attribute(class_name),
        html_escape::encode_text(label)
    )
}

fn optional_link(url: Option<&str>, label: &str) -> String {
    url.map(|url| {
        format!(
            r#"<a href="{}">{}</a>"#,
            html_escape::encode_double_quoted_attribute(url),
            html_escape::encode_text(label)
        )
    })
    .unwrap_or_default()
}

const STYLE: &str = r#"<style>:root{color-scheme:dark;--bg:#080a0f;--panel:#101520;--line:#1f2b3d;--text:#e5edf7;--muted:#8da0b8;--hot:#85ffd1;--blue:#8ab4ff;--warn:#ffd180;--bad:#ff8a9a}*{box-sizing:border-box}body{margin:0;background:radial-gradient(circle at top left,#172033,#080a0f 48%);color:var(--text);font:14px/1.5 ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace}main{width:min(1180px,92vw);margin:0 auto;padding:28px 0 56px}.hero{padding:38px 0}.detail{padding-bottom:18px}.eyebrow{color:var(--hot);letter-spacing:.08em;text-transform:uppercase}h1{font-size:clamp(34px,8vw,72px);line-height:.95;margin:10px 0 18px;max-width:920px}h2{margin:0 0 12px}.hero p{max-width:780px;color:var(--muted);font-size:16px}.actions{display:flex;gap:10px;flex-wrap:wrap;margin-top:22px}button,.actions a,.back{background:var(--hot);border:0;color:#06110d;padding:10px 14px;border-radius:999px;font-weight:700;text-decoration:none;cursor:pointer}.actions a,.back{background:#172235;color:var(--text);border:1px solid var(--line)}.sync-result{margin-top:14px;color:var(--hot)}.stats{display:grid;grid-template-columns:repeat(6,1fr);gap:10px;margin:8px 0 18px}.stats div,.meta,.tools,.detail-grid article,.raw{background:rgba(16,21,32,.78);border:1px solid var(--line);border-radius:18px}.stats div{padding:16px}.stats strong{display:block;font-size:28px}.stats span,.count{color:var(--muted)}.meta{display:flex;gap:24px;flex-wrap:wrap;padding:14px 16px;color:var(--muted)}.tools,.raw{margin-top:18px;overflow:hidden}.filters{display:grid;grid-template-columns:2fr repeat(3,1fr) auto;gap:10px;padding:12px;border-bottom:1px solid var(--line)}input,select{width:100%;background:#0b0f17;border:1px solid var(--line);border-radius:12px;padding:11px;color:var(--text);font:inherit}label{display:flex;align-items:center;gap:8px;color:var(--muted);white-space:nowrap}.count{padding:0 14px}table{width:100%;border-collapse:collapse}th,td{text-align:left;padding:12px 14px;border-bottom:1px solid var(--line);vertical-align:top}th{color:var(--muted);font-size:12px;text-transform:uppercase;letter-spacing:.06em}td small{display:block;color:var(--muted);max-width:420px;overflow-wrap:anywhere}.badge{display:inline-block;border:1px solid var(--line);border-radius:999px;padding:3px 8px;margin:2px;color:var(--blue);font-size:12px}.active,.verified,.open{color:var(--hot)}.deregistered,.hash_mismatch,.predicate{color:var(--warn)}.read_error,.fetch_error{color:var(--bad)}.x402{color:#c7a5ff}a{color:var(--hot)}.detail-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:14px}.detail-grid article{padding:18px;overflow-wrap:anywhere}code,pre{background:#070a10;border:1px solid var(--line);border-radius:12px}code{padding:2px 5px}pre{padding:16px;overflow:auto;max-height:560px}.raw h2{padding:16px 16px 0}@media(max-width:920px){.stats,.detail-grid{grid-template-columns:repeat(2,1fr)}.filters{grid-template-columns:1fr 1fr}}@media(max-width:720px){.stats,.detail-grid,.filters{grid-template-columns:1fr}table,thead,tbody,tr,td{display:block}thead{display:none}tr{padding:10px;border-bottom:1px solid var(--line)}td{border:0;padding:6px 12px}}</style>"#;

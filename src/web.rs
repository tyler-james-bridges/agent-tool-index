use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::RwLock;

use crate::cache::save_snapshot;
use crate::events::{apply_event_history, backfill_events};
use crate::indexer::{access_label, sync_registry};
use crate::storage::{event_count, save_events_db, save_snapshot_db};
use crate::types::{ManifestStatus, Snapshot, ToolRecord, ToolStatus};

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

async fn index(State(state): State<AppState>) -> Html<String> {
    let snapshot = state.snapshot.read().await.clone();
    Html(render_index(&snapshot))
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

pub async fn serve(addr: &str, state: AppState) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router(state)).await?;
    Ok(())
}

fn render_index(snapshot: &Snapshot) -> String {
    let stats = snapshot.stats();
    let rows = snapshot
        .tools
        .iter()
        .map(render_tool_row)
        .collect::<String>();
    format!(
        r##"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>ERC-8257 Index</title>{style}<script src="https://unpkg.com/htmx.org@2.0.4"></script></head><body><main><section class="hero"><p class="eyebrow">opencode demo / Base / ERC-8257</p><h1>Agent Tool Registry Index</h1><p>Live explorer for every known tool ID in the Base ERC-8257 ToolRegistry. Built as agent-readable infra first, human UI second.</p><div class="actions"><button hx-post="/api/sync" hx-target="#sync-result">Sync Registry</button><a href="/api/tools">Agent JSON</a><a href="/api/resolve">Resolver</a><a href="/llms.txt">llms.txt</a></div><div id="sync-result" class="sync-result"></div></section><section class="stats"><div><strong>{total}</strong><span>Total IDs</span></div><div><strong>{active}</strong><span>Active</span></div><div><strong>{deregistered}</strong><span>Deregistered</span></div><div><strong>{verified}</strong><span>Verified</span></div><div><strong>{x402}</strong><span>x402</span></div><div><strong>{gated}</strong><span>Gated</span></div></section><section class="meta"><p><b>Registry</b> {registry}</p><p><b>Synced</b> {synced}</p></section><section class="tools"><div class="filters"><input id="search" placeholder="Search name, endpoint, creator, tag..." oninput="filterRows()"><select id="status-filter" onchange="filterRows()"><option value="">Any status</option><option value="active">Active</option><option value="deregistered">Deregistered</option><option value="read_error">Read error</option></select><select id="manifest-filter" onchange="filterRows()"><option value="">Any manifest</option><option value="verified">Verified</option><option value="hash_mismatch">Hash mismatch</option><option value="fetch_error">Fetch error</option><option value="unchecked">Unchecked</option></select><select id="access-filter" onchange="filterRows()"><option value="">Any access</option><option value="open">Open</option><option value="predicate">Predicate</option><option value="unknown">Unknown</option></select><label><input id="x402-filter" type="checkbox" onchange="filterRows()"> x402 only</label></div><p id="visible-count" class="count"></p><table><thead><tr><th>ID</th><th>Status</th><th>Tool</th><th>Access</th><th>Manifest</th><th>Endpoint</th></tr></thead><tbody>{rows}</tbody></table></section></main>{script}</body></html>"##,
        style = STYLE,
        total = stats.total_ids,
        active = stats.active,
        deregistered = stats.deregistered,
        verified = stats.verified_manifests,
        x402 = stats.x402_tools,
        gated = stats.gated_tools,
        registry = snapshot.registry,
        synced = snapshot.synced_at,
        rows = rows,
        script = FILTER_SCRIPT
    )
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

fn render_tool_row(tool: &ToolRecord) -> String {
    let title = tool
        .name
        .as_deref()
        .or(tool.metadata_uri.as_deref())
        .unwrap_or("unknown");
    let endpoint = tool.endpoint.as_deref().unwrap_or("");
    let uri = tool.metadata_uri.as_deref().unwrap_or("");
    let search_text = searchable_text(tool);
    let haystack = html_escape::encode_double_quoted_attribute(&search_text);
    format!(
        r#"<tr data-search="{haystack}" data-status="{status}" data-manifest="{manifest}" data-access="{access}" data-x402="{has_x402}"><td><a href="/tools/{id}">#{id}</a></td><td>{status_badge}</td><td><b>{title}</b><small>{uri}</small></td><td>{access_badge}{x402_badge}</td><td>{manifest_badge}</td><td>{endpoint}</td></tr>"#,
        haystack = haystack,
        status = status_text(tool),
        manifest = manifest_text(tool),
        access = access_label(tool),
        has_x402 = tool.has_x402,
        id = tool.tool_id,
        status_badge = badge(status_text(tool), status_text(tool)),
        title = html_escape::encode_text(title),
        uri = html_escape::encode_text(uri),
        access_badge = badge(access_label(tool), access_label(tool)),
        x402_badge = if tool.has_x402 {
            badge("x402", "x402")
        } else {
            String::new()
        },
        manifest_badge = badge(manifest_text(tool), manifest_text(tool)),
        endpoint = html_escape::encode_text(endpoint),
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
    let amount = pricing.get("amount")?.as_str()?.parse::<f64>().ok()?;
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

const FILTER_SCRIPT: &str = r#"<script>function filterRows(){const q=document.getElementById('search').value.toLowerCase();const status=document.getElementById('status-filter').value;const manifest=document.getElementById('manifest-filter').value;const access=document.getElementById('access-filter').value;const x402=document.getElementById('x402-filter').checked;let visible=0;document.querySelectorAll('tbody tr').forEach(r=>{const ok=(!q||r.dataset.search.includes(q))&&(!status||r.dataset.status===status)&&(!manifest||r.dataset.manifest===manifest)&&(!access||r.dataset.access===access)&&(!x402||r.dataset.x402==='true');r.style.display=ok?'':'none';if(ok)visible++});document.getElementById('visible-count').textContent=visible+' visible tools'}filterRows()</script>"#;

const STYLE: &str = r#"<style>:root{color-scheme:dark;--bg:#080a0f;--panel:#101520;--line:#1f2b3d;--text:#e5edf7;--muted:#8da0b8;--hot:#85ffd1;--blue:#8ab4ff;--warn:#ffd180;--bad:#ff8a9a}*{box-sizing:border-box}body{margin:0;background:radial-gradient(circle at top left,#172033,#080a0f 48%);color:var(--text);font:14px/1.5 ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace}main{width:min(1180px,92vw);margin:0 auto;padding:28px 0 56px}.hero{padding:38px 0}.detail{padding-bottom:18px}.eyebrow{color:var(--hot);letter-spacing:.08em;text-transform:uppercase}h1{font-size:clamp(34px,8vw,72px);line-height:.95;margin:10px 0 18px;max-width:920px}h2{margin:0 0 12px}.hero p{max-width:780px;color:var(--muted);font-size:16px}.actions{display:flex;gap:10px;flex-wrap:wrap;margin-top:22px}button,.actions a,.back{background:var(--hot);border:0;color:#06110d;padding:10px 14px;border-radius:999px;font-weight:700;text-decoration:none;cursor:pointer}.actions a,.back{background:#172235;color:var(--text);border:1px solid var(--line)}.sync-result{margin-top:14px;color:var(--hot)}.stats{display:grid;grid-template-columns:repeat(6,1fr);gap:10px;margin:8px 0 18px}.stats div,.meta,.tools,.detail-grid article,.raw{background:rgba(16,21,32,.78);border:1px solid var(--line);border-radius:18px}.stats div{padding:16px}.stats strong{display:block;font-size:28px}.stats span,.count{color:var(--muted)}.meta{display:flex;gap:24px;flex-wrap:wrap;padding:14px 16px;color:var(--muted)}.tools,.raw{margin-top:18px;overflow:hidden}.filters{display:grid;grid-template-columns:2fr repeat(3,1fr) auto;gap:10px;padding:12px;border-bottom:1px solid var(--line)}input,select{width:100%;background:#0b0f17;border:1px solid var(--line);border-radius:12px;padding:11px;color:var(--text);font:inherit}label{display:flex;align-items:center;gap:8px;color:var(--muted);white-space:nowrap}.count{padding:0 14px}table{width:100%;border-collapse:collapse}th,td{text-align:left;padding:12px 14px;border-bottom:1px solid var(--line);vertical-align:top}th{color:var(--muted);font-size:12px;text-transform:uppercase;letter-spacing:.06em}td small{display:block;color:var(--muted);max-width:420px;overflow-wrap:anywhere}.badge{display:inline-block;border:1px solid var(--line);border-radius:999px;padding:3px 8px;margin:2px;color:var(--blue);font-size:12px}.active,.verified,.open{color:var(--hot)}.deregistered,.hash_mismatch,.predicate{color:var(--warn)}.read_error,.fetch_error{color:var(--bad)}.x402{color:#c7a5ff}a{color:var(--hot)}.detail-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:14px}.detail-grid article{padding:18px;overflow-wrap:anywhere}code,pre{background:#070a10;border:1px solid var(--line);border-radius:12px}code{padding:2px 5px}pre{padding:16px;overflow:auto;max-height:560px}.raw h2{padding:16px 16px 0}@media(max-width:920px){.stats,.detail-grid{grid-template-columns:repeat(2,1fr)}.filters{grid-template-columns:1fr 1fr}}@media(max-width:720px){.stats,.detail-grid,.filters{grid-template-columns:1fr}table,thead,tbody,tr,td{display:block}thead{display:none}tr{padding:10px;border-bottom:1px solid var(--line)}td{border:0;padding:6px 12px}}</style>"#;

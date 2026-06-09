// GET /api/verify/:chain_id/:tool_id — live trust/verification report for an
// ERC-8257 tool. Combines the baked (keccak-verified) registry snapshot with two
// live probes: an HTTP liveness check of the tool endpoint and a raw JSON-RPC
// eth_call to the onchain registry's getToolConfig(uint256).
//
// Dependency-free: Node built-ins + global fetch only.
const lib = require("../../_lib");

// chain_id -> { name, rpc }. Single source of truth for supported chains here.
const CHAINS = {
  1: { name: "Ethereum", rpc: "https://ethereum-rpc.publicnode.com" },
  8453: { name: "Base", rpc: "https://mainnet.base.org" },
  2741: { name: "Abstract", rpc: "https://api.mainnet.abs.xyz" },
};

// Registry contract is the same address on every supported chain.
const REGISTRY_ADDRESS = "0x265BB2DBFC0A8165C9A1941Eb1372F349baD2cf1";

// 4-byte function selector for getToolConfig(uint256), i.e. the first 4 bytes of
// keccak256("getToolConfig(uint256)"). Hardcoded so we stay dependency-free (no
// keccak lib). Verified offline with `cast sig "getToolConfig(uint256)"`.
const GET_TOOL_CONFIG_SELECTOR = "0xa0178453";

// Known revert selector for ToolIsDeregistered(uint256) — surfaced in eth_call
// error payloads as the leading bytes of error.data. Matches src/verify.rs.
const TOOL_DEREGISTERED_SELECTOR = "0x0bf47976";

const ENDPOINT_TIMEOUT_MS = 8000;
const RPC_TIMEOUT_MS = 8000;

// ---- pure helpers (unit-testable, no network) ----

// Resolve a chain_id (string or number) to { chain_id, name, rpc } or null.
function resolveChain(chainId) {
  const n = Number(chainId);
  if (!Number.isInteger(n) || !CHAINS[n]) return null;
  return { chain_id: n, name: CHAINS[n].name, rpc: CHAINS[n].rpc };
}

// Validate a tool_id is a positive integer. Returns the number or null.
function parseToolId(toolId) {
  if (toolId == null || toolId === "") return null;
  if (!/^\d+$/.test(String(toolId))) return null;
  const n = Number(toolId);
  if (!Number.isInteger(n) || n < 1) return null;
  return n;
}

// Encode a uint256 tool_id into the 64-hex-char (32-byte) ABI calldata tail.
function encodeUint256(n) {
  return BigInt(n).toString(16).padStart(64, "0");
}

// Build the full eth_call data field for getToolConfig(tool_id).
function buildCallData(toolId) {
  return GET_TOOL_CONFIG_SELECTOR + encodeUint256(toolId);
}

// Classify an HTTP status code into an endpoint_liveness label. Any HTTP answer
// (incl 4xx/405) means the host is reachable, so it is "alive".
function classifyLiveness(httpStatus) {
  if (httpStatus == null) return "dead";
  if (Number.isInteger(httpStatus) && httpStatus >= 100 && httpStatus < 600) return "alive";
  return "dead";
}

// Map the keccak-verified snapshot record into the report's static trust fields.
function snapshotFields(t) {
  if (!t) {
    return {
      status: null,
      manifest_status: null,
      manifest_hash: null,
      computed_hash: null,
      hash_match: false,
      metadata_uri: null,
      endpoint: null,
      access: null,
      predicate_type: null,
      has_x402: null,
      has_auth: null,
    };
  }
  return {
    status: t.status || null,
    manifest_status: t.manifest_status || null,
    manifest_hash: t.manifest_hash || null,
    computed_hash: t.computed_hash || null,
    hash_match: t.manifest_status === "verified",
    metadata_uri: t.metadata_uri || null,
    endpoint: t.endpoint || null,
    access: t.access || null,
    predicate_type: t.predicate_type || null,
    has_x402: !!t.has_x402,
    has_auth: !!t.has_auth,
  };
}

// Compose a short human-readable summary from the assembled report parts.
function buildSummary(r) {
  if (!r.snapshot_found && r.onchain_live === "unknown") {
    return `Tool ${r.tool_id} not found in the ${r.chain_name} snapshot and onchain status could not be determined.`;
  }
  const parts = [];
  parts.push(`Tool ${r.tool_id} on ${r.chain_name}`);
  if (r.snapshot_found) {
    parts.push(r.status === "active" ? "active in registry" : `${r.status || "unknown"} in registry`);
    parts.push(r.hash_match ? "manifest hash verified" : `manifest ${r.manifest_status || "unverified"}`);
  } else {
    parts.push("absent from snapshot");
  }
  if (r.onchain_live === "active") parts.push("onchain config present");
  else if (r.onchain_live === "deregistered_or_missing") parts.push("onchain deregistered or missing");
  else parts.push("onchain status unknown");
  if (r.endpoint_liveness === "alive") parts.push(`endpoint alive (HTTP ${r.endpoint_http_status})`);
  else if (r.endpoint_liveness === "dead") parts.push("endpoint unreachable");
  else parts.push("no endpoint to probe");
  if (r.can_call && r.can_call.status) parts.push(`call verdict: ${r.can_call.status}`);
  return parts.join("; ") + ".";
}

// Interpret a JSON-RPC response object into an onchain_live label.
function classifyOnchain(rpcResult) {
  // rpcResult: { ok: bool, json?: object, networkError?: bool }
  if (rpcResult.networkError) return { onchain_live: "unknown", note: "rpc network failure" };
  const j = rpcResult.json;
  if (!j || typeof j !== "object") return { onchain_live: "unknown", note: "malformed rpc response" };
  if (j.error) {
    const data = (j.error.data && String(j.error.data)) || "";
    if (data.toLowerCase().startsWith(TOOL_DEREGISTERED_SELECTOR)) {
      return { onchain_live: "deregistered_or_missing", note: "ToolIsDeregistered revert" };
    }
    // Any revert/execution error means the registry rejected the read.
    return { onchain_live: "deregistered_or_missing", note: j.error.message || "eth_call reverted" };
  }
  if (typeof j.result === "string" && j.result !== "0x" && j.result.length > 2) {
    return { onchain_live: "active", note: null };
  }
  // Empty result (0x) from a non-reverting call: treat as missing data.
  return { onchain_live: "deregistered_or_missing", note: "empty eth_call result" };
}

// ---- network helpers (not unit tested) ----

// Probe an endpoint: HEAD first, GET fallback. Returns { alive, status }.
async function probeEndpoint(endpoint) {
  if (!endpoint) return { alive: null, status: null };
  for (const method of ["HEAD", "GET"]) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), ENDPOINT_TIMEOUT_MS);
    try {
      const resp = await fetch(endpoint, { method, redirect: "manual", signal: controller.signal });
      clearTimeout(timer);
      return { alive: true, status: resp.status };
    } catch (e) {
      clearTimeout(timer);
      // HEAD may be rejected at the transport layer by some hosts; try GET next.
      if (method === "GET") return { alive: false, status: null };
    }
  }
  return { alive: false, status: null };
}

// Raw JSON-RPC eth_call to the registry's getToolConfig. Returns the shape that
// classifyOnchain expects: { ok, json?, networkError? }.
async function onchainCall(rpc, toolId) {
  const payload = {
    jsonrpc: "2.0",
    id: 1,
    method: "eth_call",
    params: [{ to: REGISTRY_ADDRESS, data: buildCallData(toolId) }, "latest"],
  };
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), RPC_TIMEOUT_MS);
  try {
    const resp = await fetch(rpc, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload),
      signal: controller.signal,
    });
    clearTimeout(timer);
    let json;
    try {
      json = await resp.json();
    } catch (e) {
      return { ok: false, networkError: false, json: null };
    }
    return { ok: resp.ok, json };
  } catch (e) {
    clearTimeout(timer);
    return { ok: false, networkError: true };
  }
}

// ---- handler ----

module.exports = async function handler(req, res) {
  if (lib.preflight(req, res)) return;
  if (req.method !== "GET") return lib.send(res, 405, { error: "GET only" });

  const q = req.query || {};
  const chain = resolveChain(q.chain_id);
  if (!chain) {
    return lib.send(res, 400, {
      error: "unknown or unsupported chain_id",
      chain_id: q.chain_id != null ? String(q.chain_id) : null,
      supported: Object.keys(CHAINS).map(Number),
    });
  }
  const toolId = parseToolId(q.tool_id);
  if (toolId == null) {
    return lib.send(res, 400, { error: "tool_id must be a positive integer", tool_id: q.tool_id != null ? String(q.tool_id) : null });
  }

  const reg = lib.registry();
  const snap = lib.findTool(toolId, chain.chain_id);
  const fields = snapshotFields(snap);

  // Live endpoint probe.
  const endpointProbedAt = new Date().toISOString();
  const probe = await probeEndpoint(fields.endpoint);
  const endpointLiveness = fields.endpoint ? classifyLiveness(probe.status) : "unknown";

  // Live onchain status check.
  const onchainCheckedAt = new Date().toISOString();
  const rpcResult = await onchainCall(chain.rpc, toolId);
  const onchain = classifyOnchain(rpcResult);

  // Call verdict from the shared planner (snapshot-derived).
  const canCall = snap ? lib.planCall(snap, {}) : null;

  const report = {
    chain_id: chain.chain_id,
    chain_name: chain.name,
    tool_id: toolId,
    snapshot_found: !!snap,
    status: fields.status,
    manifest_status: fields.manifest_status,
    manifest_hash: fields.manifest_hash,
    computed_hash: fields.computed_hash,
    hash_match: fields.hash_match,
    hash_verified_as_of: reg.synced_at || null,
    metadata_uri: fields.metadata_uri,
    endpoint: fields.endpoint,
    endpoint_alive: fields.endpoint ? probe.alive : null,
    endpoint_liveness: endpointLiveness,
    endpoint_http_status: probe.status,
    endpoint_probed_at: endpointProbedAt,
    access: fields.access,
    predicate_type: fields.predicate_type,
    has_x402: fields.has_x402,
    has_auth: fields.has_auth,
    onchain_live: onchain.onchain_live,
    onchain_note: onchain.note,
    onchain_checked_at: onchainCheckedAt,
    can_call: canCall,
    summary: "",
  };
  report.summary = buildSummary(report);

  lib.send(res, 200, report);
};

// Exported for unit tests. The handler is the default export above.
module.exports.resolveChain = resolveChain;
module.exports.parseToolId = parseToolId;
module.exports.encodeUint256 = encodeUint256;
module.exports.buildCallData = buildCallData;
module.exports.classifyLiveness = classifyLiveness;
module.exports.snapshotFields = snapshotFields;
module.exports.buildSummary = buildSummary;
module.exports.classifyOnchain = classifyOnchain;
module.exports.CHAINS = CHAINS;
module.exports.GET_TOOL_CONFIG_SELECTOR = GET_TOOL_CONFIG_SELECTOR;
module.exports.REGISTRY_ADDRESS = REGISTRY_ADDRESS;

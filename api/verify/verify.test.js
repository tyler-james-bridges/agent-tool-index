// Unit tests for the dependency-free verify endpoint pure helpers.
// Run with: node --test api/verify/verify.test.js
// No network, no dependencies (node:test + node:assert only).
const test = require("node:test");
const assert = require("node:assert/strict");

const v = require("./[chain_id]/[tool_id].js");

test("resolveChain maps supported chains to rpc/name", () => {
  assert.deepEqual(v.resolveChain(1), { chain_id: 1, name: "Ethereum", rpc: "https://ethereum-rpc.publicnode.com" });
  assert.deepEqual(v.resolveChain("8453"), { chain_id: 8453, name: "Base", rpc: "https://mainnet.base.org" });
  assert.deepEqual(v.resolveChain(2741), { chain_id: 2741, name: "Abstract", rpc: "https://api.mainnet.abs.xyz" });
});

test("resolveChain rejects unknown or non-numeric chains", () => {
  assert.equal(v.resolveChain(137), null);
  assert.equal(v.resolveChain("abc"), null);
  assert.equal(v.resolveChain(""), null);
  assert.equal(v.resolveChain(null), null);
});

test("parseToolId accepts positive integers only", () => {
  assert.equal(v.parseToolId("1"), 1);
  assert.equal(v.parseToolId(42), 42);
  assert.equal(v.parseToolId("0"), null);
  assert.equal(v.parseToolId("-3"), null);
  assert.equal(v.parseToolId("1.5"), null);
  assert.equal(v.parseToolId("abc"), null);
  assert.equal(v.parseToolId(""), null);
  assert.equal(v.parseToolId(null), null);
});

test("encodeUint256 produces 32-byte hex tail", () => {
  assert.equal(v.encodeUint256(1).length, 64);
  assert.equal(v.encodeUint256(1), "0".repeat(63) + "1");
  assert.equal(v.encodeUint256(255), "0".repeat(62) + "ff");
});

test("buildCallData prefixes the getToolConfig selector", () => {
  const data = v.buildCallData(1);
  assert.equal(data, v.GET_TOOL_CONFIG_SELECTOR + "0".repeat(63) + "1");
  assert.equal(v.GET_TOOL_CONFIG_SELECTOR, "0xa0178453");
  assert.equal(data.length, 10 + 64); // 0x + 8 selector chars + 64 arg chars
});

test("classifyLiveness treats any HTTP status as alive", () => {
  assert.equal(v.classifyLiveness(200), "alive");
  assert.equal(v.classifyLiveness(405), "alive");
  assert.equal(v.classifyLiveness(404), "alive");
  assert.equal(v.classifyLiveness(500), "alive");
  assert.equal(v.classifyLiveness(null), "dead");
  assert.equal(v.classifyLiveness(700), "dead");
});

test("snapshotFields maps a verified snapshot record", () => {
  const t = {
    status: "active",
    manifest_status: "verified",
    manifest_hash: "0xaaa",
    computed_hash: "0xaaa",
    metadata_uri: "https://example.com/.well-known/ai-tool/x.json",
    endpoint: "https://example.com/tool",
    access: "open",
    predicate_type: "open",
    has_x402: true,
    has_auth: false,
  };
  const f = v.snapshotFields(t);
  assert.equal(f.status, "active");
  assert.equal(f.hash_match, true);
  assert.equal(f.has_x402, true);
  assert.equal(f.has_auth, false);
  assert.equal(f.endpoint, "https://example.com/tool");
});

test("snapshotFields handles a mismatched manifest and missing record", () => {
  const mismatch = v.snapshotFields({ manifest_status: "hash_mismatch", status: "active" });
  assert.equal(mismatch.hash_match, false);

  const missing = v.snapshotFields(null);
  assert.equal(missing.status, null);
  assert.equal(missing.hash_match, false);
  assert.equal(missing.endpoint, null);
});

test("classifyOnchain returns active for a non-empty result", () => {
  const r = v.classifyOnchain({ ok: true, json: { jsonrpc: "2.0", id: 1, result: "0x" + "ab".repeat(32) } });
  assert.equal(r.onchain_live, "active");
});

test("classifyOnchain returns deregistered_or_missing for the known revert selector", () => {
  const r = v.classifyOnchain({ ok: false, json: { error: { code: 3, message: "execution reverted", data: "0x0bf47976" + "0".repeat(64) } } });
  assert.equal(r.onchain_live, "deregistered_or_missing");
  assert.equal(r.note, "ToolIsDeregistered revert");
});

test("classifyOnchain returns deregistered_or_missing for a generic revert", () => {
  const r = v.classifyOnchain({ ok: false, json: { error: { code: 3, message: "execution reverted" } } });
  assert.equal(r.onchain_live, "deregistered_or_missing");
});

test("classifyOnchain returns unknown on network failure or malformed payload", () => {
  assert.equal(v.classifyOnchain({ networkError: true }).onchain_live, "unknown");
  assert.equal(v.classifyOnchain({ ok: true, json: null }).onchain_live, "unknown");
});

test("classifyOnchain treats an empty 0x result as missing", () => {
  const r = v.classifyOnchain({ ok: true, json: { result: "0x" } });
  assert.equal(r.onchain_live, "deregistered_or_missing");
});

test("buildSummary composes a verified-active summary", () => {
  const summary = v.buildSummary({
    tool_id: 1,
    chain_name: "Ethereum",
    snapshot_found: true,
    status: "active",
    hash_match: true,
    manifest_status: "verified",
    onchain_live: "active",
    endpoint_liveness: "alive",
    endpoint_http_status: 200,
    can_call: { status: "callable" },
  });
  assert.match(summary, /Tool 1 on Ethereum/);
  assert.match(summary, /active in registry/);
  assert.match(summary, /manifest hash verified/);
  assert.match(summary, /onchain config present/);
  assert.match(summary, /endpoint alive \(HTTP 200\)/);
  assert.match(summary, /call verdict: callable/);
});

test("buildSummary handles a tool absent from the snapshot and unknown onchain", () => {
  const summary = v.buildSummary({
    tool_id: 999,
    chain_name: "Base",
    snapshot_found: false,
    onchain_live: "unknown",
    endpoint_liveness: "unknown",
  });
  assert.match(summary, /not found in the Base snapshot/);
});

test("buildSummary reports a dead endpoint and deregistered onchain", () => {
  const summary = v.buildSummary({
    tool_id: 5,
    chain_name: "Base",
    snapshot_found: true,
    status: "deregistered",
    hash_match: false,
    manifest_status: "hash_mismatch",
    onchain_live: "deregistered_or_missing",
    endpoint_liveness: "dead",
    can_call: { status: "blocked" },
  });
  assert.match(summary, /deregistered in registry/);
  assert.match(summary, /onchain deregistered or missing/);
  assert.match(summary, /endpoint unreachable/);
  assert.match(summary, /call verdict: blocked/);
});

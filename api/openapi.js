// GET /api/openapi (also served at /openapi.json via rewrite).
const lib = require("./_lib");

module.exports = function handler(req, res) {
  if (lib.preflight(req, res)) return;
  const r = lib.registry();
  lib.send(res, 200, {
    openapi: "3.1.0",
    info: {
      title: "Agent Tool Index API",
      version: "1.0.0",
      description:
        "Live agent surface for the Agent Tool Index. Resolve and plan ERC-8257 tools, then invoke any registered tool through the same-origin /api/call proxy, which transparently handles the x402 payment challenge/retry loop.",
    },
    servers: [{ url: "https://agenttoolindex.xyz" }],
    "x-registry": { chain_id: r.chain_id, registry: r.registry, synced_at: r.synced_at, tool_count: r.tool_count },
    paths: {
      "/api/tools": {
        get: {
          summary: "List tools",
          parameters: [
            { name: "q", in: "query", schema: { type: "string" } },
            { name: "status", in: "query", schema: { type: "string", enum: ["active", "deregistered"] } },
            { name: "access", in: "query", schema: { type: "string", enum: ["open", "gated"] } },
            { name: "x402", in: "query", schema: { type: "boolean" } },
            { name: "limit", in: "query", schema: { type: "integer" } },
          ],
        },
      },
      "/api/tools/{id}": { get: { summary: "Get one tool", parameters: [{ name: "id", in: "path", required: true, schema: { type: "integer" } }] } },
      "/api/tools/{id}/can_call": {
        post: {
          summary: "Plan callability for a caller context",
          requestBody: { content: { "application/json": { schema: { type: "object", properties: { wallet: { type: "string" }, budget_usdc: { type: "number" }, allow_x402: { type: "boolean" }, has_auth: { type: "boolean" } } } } } },
        },
      },
      "/api/verify/{chain_id}/{tool_id}": {
        get: {
          summary: "Live trust/verification report for a tool",
          description:
            "Combines the keccak-verified registry snapshot with two live probes: an HTTP liveness check of the tool endpoint and a raw eth_call to the onchain registry getToolConfig(uint256).",
          parameters: [
            { name: "chain_id", in: "path", required: true, schema: { type: "integer" } },
            { name: "tool_id", in: "path", required: true, schema: { type: "integer" } },
          ],
        },
      },
      "/api/resolve": {
        get: { summary: "Resolve usage help" },
        post: { summary: "Resolve intent to candidate tools", requestBody: { content: { "application/json": { schema: { type: "object", properties: { query: { type: "string" }, status: { type: "string" }, access: { type: "string" }, manifest_status: { type: "string" }, x402: { type: "boolean" }, limit: { type: "integer" } } } } } } },
      },
      "/api/stats": { get: { summary: "Index statistics" } },
      "/api/call": {
        post: {
          summary: "Invoke a registered tool endpoint (handles x402)",
          description:
            "Proxies a request to a tool endpoint whose host is in the registry. On a 402 the envelope returns the x402 `accepts` requirements; resend with an X-PAYMENT header (or xPayment in the body) to settle and receive the result.",
          requestBody: {
            content: {
              "application/json": {
                schema: {
                  type: "object",
                  required: ["url"],
                  properties: {
                    url: { type: "string", description: "tool endpoint (host must be in the registry)" },
                    method: { type: "string", default: "POST" },
                    body: { type: "object", description: "tool inputs" },
                    xPayment: { type: "string", description: "base64 x402 payment payload" },
                  },
                },
              },
            },
          },
        },
      },
    },
  });
};

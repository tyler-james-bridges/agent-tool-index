// GET /api/resolve — usage help. POST /api/resolve — resolve intent to candidate tools.
const lib = require("./_lib");

module.exports = async function handler(req, res) {
  if (lib.preflight(req, res)) return;

  if (req.method !== "POST") {
    return lib.send(res, 200, {
      method: "POST",
      endpoint: "/api/resolve",
      body: { query: "wallet risk", status: "active", access: "open", manifest_status: "verified", x402: true, limit: 5 },
    });
  }

  const body = await lib.readBody(req);
  const f = {
    query: body.query || null,
    status: body.status || null,
    access: body.access || null,
    manifest_status: body.manifest_status || null,
    x402: typeof body.x402 === "boolean" ? body.x402 : null,
  };
  let list = lib.tools().filter((t) => lib.resolveMatches(t, f));
  list = list
    .map((t) => ({ score: lib.resolveScore(t, f.query), tool: t }))
    .sort((a, b) => b.score - a.score);
  const limit = Math.min(body.limit || 10, 50);
  list = list.slice(0, limit);

  lib.send(res, 200, {
    query: f.query,
    filters: { status: f.status, access: f.access, manifest_status: f.manifest_status, x402: f.x402 },
    count: list.length,
    tools: list.map((x) => ({
      score: x.score,
      tool_id: x.tool.id,
      name: x.tool.name,
      endpoint: x.tool.endpoint,
      price_usdc: x.tool.price_usdc,
      has_x402: x.tool.has_x402,
      access: x.tool.access,
      manifest_status: x.tool.manifest_status,
    })),
  });
};

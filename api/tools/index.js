// GET /api/tools — agent-readable tool list with optional filters.
const lib = require("../_lib");

module.exports = function handler(req, res) {
  if (lib.preflight(req, res)) return;
  const q = req.query || {};
  const f = {
    status: q.status || null,
    access: q.access || null,
    manifest_status: q.manifest_status || null,
    x402: q.x402 === "true" ? true : q.x402 === "false" ? false : null,
    query: q.q || q.query || null,
  };
  let list = lib.tools().filter((t) => lib.resolveMatches(t, f));
  if (f.query) {
    list = [...list].sort((a, b) => lib.resolveScore(b, f.query) - lib.resolveScore(a, f.query));
  }
  const limit = Math.min(parseInt(q.limit, 10) || list.length, 500);
  lib.send(res, 200, { count: Math.min(list.length, limit), total: lib.tools().length, tools: list.slice(0, limit) });
};

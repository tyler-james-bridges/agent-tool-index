// GET /api/stats — index statistics.
const lib = require("./_lib");

module.exports = function handler(req, res) {
  if (lib.preflight(req, res)) return;
  const r = lib.registry();
  lib.send(res, 200, {
    chain_id: r.chain_id,
    registry: r.registry,
    chains: r.chains,
    tool_count: r.tool_count,
    synced_at: r.synced_at,
    stats: lib.stats(),
  });
};

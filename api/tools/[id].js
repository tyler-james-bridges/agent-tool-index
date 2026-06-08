// GET /api/tools/:id — single tool record.
const lib = require("../_lib");

module.exports = function handler(req, res) {
  if (lib.preflight(req, res)) return;
  const t = lib.findTool(req.query.id, req.query.chain_id);
  if (!t) return lib.send(res, 404, { error: "tool not found", id: req.query.id });
  lib.send(res, 200, t);
};

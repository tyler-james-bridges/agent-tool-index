// POST /api/tools/:id/can_call — plan whether a caller can invoke a tool.
const lib = require("../../_lib");

module.exports = async function handler(req, res) {
  if (lib.preflight(req, res)) return;
  const t = lib.findTool(req.query.id, req.query.chain_id);
  if (!t) return lib.send(res, 404, { error: "tool not found", id: req.query.id });
  const ctx = await lib.readBody(req);
  const plan = lib.planCall(t, ctx);
  lib.send(res, 200, {
    tool_id: t.id,
    name: t.name,
    status: plan.status,
    caller: {
      wallet: ctx.wallet || null,
      budget_usdc: ctx.budget_usdc != null ? ctx.budget_usdc : null,
      allow_x402: ctx.allow_x402 !== false,
      has_auth: !!ctx.has_auth,
    },
    requirements: plan.requirements,
    blockers: plan.blockers,
    invocation: plan.invocation,
    price_usdc: plan.price_usdc,
  });
};

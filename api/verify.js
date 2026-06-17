// GET|POST /api/verify — flat trust-report endpoint for an ERC-8257 tool.
//
// The verify-tool manifest publishes its endpoint as `/api/verify` with inputs
// { chain_id, tool_id }, but the only implementation Vercel exposed was the
// path-param route /api/verify/:chain_id/:tool_id — so the manifest's own URL
// 404'd. This is that missing flat endpoint. It accepts the params from the
// query string (manifest declares method GET) or a JSON body (the site's
// execution proxy POSTs them) and delegates to the shared report handler.
const lib = require("./_lib");
const report = require("./verify/[chain_id]/[tool_id].js");

module.exports = async function handler(req, res) {
  if (lib.preflight(req, res)) return;

  const q = req.query && typeof req.query === "object" ? req.query : {};
  let chain_id = q.chain_id;
  let tool_id = q.tool_id;

  // Proxy / direct POSTs carry the params in the JSON body.
  if (chain_id == null || tool_id == null) {
    const body = await lib.readBody(req);
    if (chain_id == null) chain_id = body.chain_id;
    if (tool_id == null) tool_id = body.tool_id;
  }

  // The shared report handler is GET-only and reads from req.query; hand it a
  // normalized query so one implementation serves both call shapes.
  req.query = Object.assign({}, q, { chain_id, tool_id });
  req.method = "GET";
  return report(req, res);
};

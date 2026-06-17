// GET|POST /api/verify — flat trust-report endpoint for an ERC-8257 tool,
// gated behind an x402 micropayment (this is the paid resource for tool 136).
//
// The verify-tool manifest publishes its endpoint as `/api/verify` with inputs
// { chain_id, tool_id }. It accepts the params from the query string (manifest
// declares method GET) or a JSON body (the site's execution proxy POSTs them).
//
// Payment flow (x402 v1, "exact" scheme, USDC on Base):
//   1. No X-PAYMENT header        -> 402 with the `accepts` requirements.
//   2. X-PAYMENT present           -> facilitator /verify; if invalid, 402 again.
//   3. valid                       -> build the report, facilitator /settle.
//   4. settled                     -> 200 + report + X-PAYMENT-RESPONSE receipt.
// Settlement runs through PayAI's hosted facilitator (keyless, Base mainnet), so
// no relayer key or API secret lives in this deployment. Dependency-free.
const lib = require("./_lib");
const report = require("./verify/[chain_id]/[tool_id].js");

const FACILITATOR = process.env.X402_FACILITATOR || "https://facilitator.payai.network";
// Base mainnet USDC (6 decimals). payTo + price are overridable via env.
const USDC_BASE = "0x833589fcD6eDb6E08f4c7C32D4f71b54bdA02913";
const PAY_TO = process.env.X402_PAY_TO || "0xa102a2cb8aac6c7d2c477412ebb7d41d0ce53495";
const PRICE_ATOMIC = process.env.X402_PRICE_ATOMIC || "1000"; // 0.001 USDC

// The canonical x402 payment requirements advertised on a 402 and sent to the
// facilitator. `extra` carries the EIP-3009 domain the client signs against.
function paymentRequirements(host) {
  return {
    scheme: "exact",
    network: "base",
    maxAmountRequired: String(PRICE_ATOMIC),
    resource: "https://" + (host || "agenttoolindex.xyz") + "/api/verify",
    description: "ERC-8257 tool verification report",
    mimeType: "application/json",
    payTo: PAY_TO,
    maxTimeoutSeconds: 120,
    asset: USDC_BASE,
    extra: { name: "USD Coin", version: "2" },
  };
}

function decodePayment(header) {
  try { return JSON.parse(Buffer.from(String(header), "base64").toString("utf8")); }
  catch (e) { return null; }
}
function encodeReceipt(obj) {
  return Buffer.from(JSON.stringify(obj), "utf8").toString("base64");
}

async function facilitate(path, body) {
  let r;
  try {
    r = await fetch(FACILITATOR + path, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
  } catch (e) {
    return { ok: false, status: 0, body: { error: String(e && e.message || e) } };
  }
  let j = null;
  try { j = await r.json(); } catch (e) {}
  return { ok: r.ok, status: r.status, body: j };
}

// Run the GET-only report handler but capture its JSON output (instead of
// flushing to the socket) so payment can be settled before the response is sent.
function runReport(req) {
  return new Promise((resolve) => {
    const cap = {
      statusCode: 200,
      setHeader() {},
      end(body) { resolve({ status: this.statusCode, body }); },
    };
    Promise.resolve(report(req, cap)).catch(() =>
      resolve({ status: 500, body: JSON.stringify({ error: "report failed" }) }));
  });
}

module.exports = async function handler(req, res) {
  if (lib.preflight(req, res)) return;

  // Normalize inputs from the query string or a JSON body before doing anything.
  const q = req.query && typeof req.query === "object" ? req.query : {};
  let chain_id = q.chain_id;
  let tool_id = q.tool_id;
  if (chain_id == null || tool_id == null) {
    const body = await lib.readBody(req);
    if (chain_id == null) chain_id = body.chain_id;
    if (tool_id == null) tool_id = body.tool_id;
  }
  req.query = Object.assign({}, q, { chain_id, tool_id });
  req.method = "GET";

  const reqs = paymentRequirements(req.headers && req.headers.host);
  const xPayment = req.headers && req.headers["x-payment"];

  // 1. No payment -> challenge.
  if (!xPayment) {
    return lib.send(res, 402, { x402Version: 1, accepts: [reqs], error: "X-PAYMENT header is required" });
  }
  const paymentPayload = decodePayment(xPayment);
  if (!paymentPayload) {
    return lib.send(res, 402, { x402Version: 1, accepts: [reqs], error: "malformed X-PAYMENT header" });
  }

  // 2. Verify the signed authorization.
  const verify = await facilitate("/verify", { x402Version: 1, paymentPayload, paymentRequirements: reqs });
  if (!verify.ok || !verify.body || verify.body.isValid !== true) {
    const reason = (verify.body && (verify.body.invalidReason || verify.body.error)) || "payment verification failed";
    return lib.send(res, 402, { x402Version: 1, accepts: [reqs], error: reason });
  }

  // 3. Produce the report, then settle the payment onchain via the facilitator.
  const out = await runReport(req);
  const settle = await facilitate("/settle", { x402Version: 1, paymentPayload, paymentRequirements: reqs });
  if (!settle.ok || !settle.body || settle.body.success !== true) {
    const reason = (settle.body && (settle.body.errorReason || settle.body.error)) || "settlement failed";
    return lib.send(res, 402, { x402Version: 1, accepts: [reqs], error: reason });
  }

  // 4. Settled -> return the report with the settlement receipt header.
  lib.cors(res);
  res.setHeader("content-type", "application/json");
  res.setHeader("x-payment-response", encodeReceipt(settle.body));
  res.statusCode = out.status || 200;
  res.end(out.body);
};

// Exported for unit tests (pure, no network).
module.exports.paymentRequirements = paymentRequirements;
module.exports.decodePayment = decodePayment;
module.exports.encodeReceipt = encodeReceipt;

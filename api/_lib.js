// Shared helpers for the Agent Tool Index serverless API.
// Dependency-free (Node built-ins + global fetch) so it runs with installCommand:null.
const fs = require("fs");
const path = require("path");

let REG = null;
function registry() {
  if (!REG) {
    const raw = fs.readFileSync(path.join(__dirname, "registry.json"), "utf8");
    REG = JSON.parse(raw);
  }
  return REG;
}

function tools() {
  return registry().tools;
}

function findTool(id, chainId) {
  const n = Number(id);
  const matches = tools().filter((t) => t.id === n);
  if (chainId != null && chainId !== "") {
    const c = Number(chainId);
    return matches.find((t) => (t.chain_id || 8453) === c) || null;
  }
  return matches[0] || null;
}

// Hosts we are willing to proxy to — derived from the registry so /api/call
// can never be turned into an open proxy / SSRF vector.
let HOSTS = null;
function allowedHosts() {
  if (!HOSTS) {
    HOSTS = new Set();
    for (const t of tools()) {
      try {
        HOSTS.add(new URL(t.endpoint).host.toLowerCase());
      } catch (e) {
        /* skip malformed endpoints */
      }
    }
  }
  return HOSTS;
}

// Port of the frontend planCall — keep verdicts identical across UI and API.
function planCall(t, ctx) {
  ctx = ctx || {};
  const req = [];
  const block = [];
  const price = t.price_usdc;
  if (t.status !== "active") block.push("tool id is not active in the registry");
  if (t.access === "gated") {
    req.push("access predicate must approve caller wallet");
    if (!ctx.wallet) block.push("wallet required to evaluate predicate access");
  }
  if (t.has_auth && !ctx.has_auth) req.push("manifest declares SIWE authentication");
  if (t.has_x402) {
    req.push("x402 payment settled before 200 response");
    if (ctx.allow_x402 === false) block.push("caller ctx disallows x402 payments");
    if (price != null && ctx.budget_usdc != null && ctx.budget_usdc < price) {
      block.push(`budget ${ctx.budget_usdc} < price ${price} USDC`);
    }
  }
  let status = "callable";
  if (block.length) status = "blocked";
  else if (t.access === "gated" || t.has_auth || t.has_x402) status = "conditional";

  const steps = [];
  if (status !== "blocked") {
    if (t.has_auth) steps.push("sign SIWE challenge");
    if (t.access === "gated") steps.push("present predicate proof");
    if (t.has_x402) steps.push("attach x-payment (x402)");
    steps.push("POST inputs -> 200");
  }
  return { status, requirements: req, blockers: block, invocation: steps, price_usdc: price };
}

function resolveScore(t, query) {
  if (!query) return 0;
  const q = query.toLowerCase();
  const hay = [t.name, t.description, (t.tags || []).join(" ")].join(" ").toLowerCase();
  let score = 0;
  for (const term of q.split(/\s+/).filter(Boolean)) {
    if ((t.name || "").toLowerCase().includes(term)) score += 3;
    if (hay.includes(term)) score += 1;
  }
  return score;
}

function resolveMatches(t, f) {
  if (f.status && t.status !== f.status) return false;
  if (f.access && t.access !== f.access) return false;
  if (f.manifest_status && t.manifest_status !== f.manifest_status) return false;
  if (f.x402 === true && !t.has_x402) return false;
  if (f.x402 === false && t.has_x402) return false;
  if (f.query) {
    const hay = [t.name, t.description, t.endpoint, (t.tags || []).join(" ")].join(" ").toLowerCase();
    for (const term of f.query.toLowerCase().split(/\s+/).filter(Boolean)) {
      if (!hay.includes(term)) return false;
    }
  }
  return true;
}

function stats() {
  const r = registry();
  const s = { total: r.tools.length, active: 0, deregistered: 0, verified: 0, mismatch: 0, x402: 0, free: 0, gated: 0, auth: 0 };
  for (const t of r.tools) {
    if (t.status === "active") s.active++; else s.deregistered++;
    if (t.manifest_status === "verified") s.verified++; else s.mismatch++;
    if (t.has_x402) s.x402++; else s.free++;
    if (t.access === "gated") s.gated++;
    if (t.has_auth) s.auth++;
  }
  return s;
}

// ---- HTTP helpers ----
function cors(res) {
  res.setHeader("access-control-allow-origin", "*");
  res.setHeader("access-control-allow-methods", "GET,POST,OPTIONS");
  res.setHeader("access-control-allow-headers", "content-type,x-payment");
  res.setHeader("access-control-expose-headers", "x-payment-response,x-accept-payment");
}

function send(res, status, obj) {
  cors(res);
  res.setHeader("content-type", "application/json");
  res.statusCode = status;
  res.end(JSON.stringify(obj));
}

function preflight(req, res) {
  if (req.method === "OPTIONS") {
    cors(res);
    res.statusCode = 204;
    res.end();
    return true;
  }
  return false;
}

async function readBody(req) {
  if (req.body !== undefined) {
    if (typeof req.body === "string") {
      try { return JSON.parse(req.body || "{}"); } catch (e) { return {}; }
    }
    return req.body || {};
  }
  return await new Promise((resolve) => {
    let data = "";
    req.on("data", (c) => { data += c; });
    req.on("end", () => { try { resolve(JSON.parse(data || "{}")); } catch (e) { resolve({}); } });
    req.on("error", () => resolve({}));
  });
}

module.exports = {
  registry, tools, findTool, allowedHosts, planCall,
  resolveScore, resolveMatches, stats,
  cors, send, preflight, readBody,
};

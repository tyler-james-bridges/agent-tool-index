// POST /api/call — same-origin execution proxy for registered tool endpoints.
//
// Body: { url, method?, body?, xPayment? }  (xPayment may also arrive as the X-PAYMENT header)
// Returns an envelope: { ok, status, headers, body } so the browser can run the
// full x402 challenge/retry loop without ever hitting CORS on the tool endpoint.
//
// Hardened against open-proxy/SSRF: the target host must appear in the registry.
const lib = require("./_lib");

module.exports = async function handler(req, res) {
  if (lib.preflight(req, res)) return;
  if (req.method !== "POST") return lib.send(res, 405, { error: "POST only" });

  const body = await lib.readBody(req);
  const target = body.url;
  if (!target || typeof target !== "string") return lib.send(res, 400, { error: "missing url" });

  let parsed;
  try { parsed = new URL(target); } catch (e) { return lib.send(res, 400, { error: "invalid url" }); }
  if (parsed.protocol !== "https:") return lib.send(res, 400, { error: "https only" });
  // Same-origin tools (endpoints hosted on this deployment, e.g. verify-tool at
  // /api/verify) are always trusted — they can't be an SSRF vector since the
  // caller could hit them directly. Registry-derived hosts cover everyone else.
  const selfHost = String(req.headers.host || "").toLowerCase();
  const host = parsed.host.toLowerCase();
  if (host !== selfHost && !lib.allowedHosts().has(host)) {
    return lib.send(res, 403, { error: "host not in registry allowlist", host: parsed.host });
  }

  const xPayment = req.headers["x-payment"] || body.xPayment || null;
  const method = (body.method || "POST").toUpperCase();
  const headers = { accept: "application/json" };
  let payload;
  if (method !== "GET" && method !== "HEAD") {
    headers["content-type"] = "application/json";
    payload = JSON.stringify(body.body != null ? body.body : {});
  }
  if (xPayment) headers["x-payment"] = xPayment;

  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 30000);
  let upstream;
  try {
    upstream = await fetch(target, { method, headers, body: payload, signal: controller.signal });
  } catch (e) {
    clearTimeout(timeout);
    return lib.send(res, 502, { error: "upstream fetch failed", detail: String(e && e.message || e) });
  }
  clearTimeout(timeout);

  const text = await upstream.text();
  let parsedBody;
  const ct = upstream.headers.get("content-type") || "";
  if (ct.includes("application/json")) {
    try { parsedBody = JSON.parse(text); } catch (e) { parsedBody = text; }
  } else {
    try { parsedBody = JSON.parse(text); } catch (e) { parsedBody = text; }
  }

  lib.send(res, 200, {
    ok: upstream.ok,
    status: upstream.status,
    headers: {
      "x-payment-response": upstream.headers.get("x-payment-response") || null,
      "x-accept-payment": upstream.headers.get("x-accept-payment") || null,
      "content-type": ct || null,
    },
    body: parsedBody,
  });
};

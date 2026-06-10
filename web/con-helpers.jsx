// con-helpers.jsx - glyphs, formatters, query parser, signatures, can_call
const { useState, useEffect, useRef, useCallback, createContext, useContext, useMemo } = React;

const REG = window.REGISTRY;
const ZERO = "0x0000000000000000000000000000000000000000";

// Tool ids are only unique per chain, so derive a globally-unique uid for React
// keys, open-state, and DOM hooks. Deep-link URLs stay numeric (/tools/{id}).
REG.tools.forEach((t) => { t.uid = (t.chain_id || 8453) + "-" + t.id; });

const CHAIN_NAMES = {1: "Ethereum", 8453: "Base", 2741: "Abstract"};
function chainName(id) { return CHAIN_NAMES[id] || "Chain " + id; }

/* ---- tiny svg set (used sparingly; UI is glyph-forward) ---- */
const Svg = {
  copy: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><rect x="9" y="9" width="11" height="11"/><path d="M5 15V5h10"/></svg>,
  term: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><path d="m5 8 4 4-4 4M12 16h6"/></svg>,
  ext: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d="M14 4h6v6M20 4l-9 9M18 13v6H5V6h6"/></svg>,
  check: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="M20 6 9 17l-5-5"/></svg>,
};
const VG = { callable: "\u25B6", conditional: "\u25D0", blocked: "\u25A0" }; // ▶ ◐ ■

/* ---- formatters ---- */
function short(a, n = 4) { if (!a) return "\u2014"; return a.length <= 2 + n * 2 ? a : a.slice(0, 2 + n) + "\u2026" + a.slice(-n); }
function fmtPrice(u) {
  if (u == null) return null;
  if (u === 0) return "0";
  if (u < 0.01) return u.toFixed(4);
  if (u < 1) return u.toFixed(3);
  return u.toFixed(2);
}
function relTime(iso) {
  if (!iso) return "\u2014";
  const m = Math.round((Date.now() - new Date(iso)) / 60000);
  if (m < 1) return "now"; if (m < 60) return m + "m"; const h = Math.round(m / 60);
  if (h < 24) return h + "h"; return Math.round(h / 24) + "d";
}
function accessOf(t) { return t.access; } // open | gated | unknown

/* ---- function signature from manifest ---- */
function signature(t) {
  const args = (t.inputs || []).map((f) => ({ name: f.name, opt: !f.required }));
  const outs = (t.outputs || []).slice(0, 4);
  return { fn: t.name || ("tool_" + t.id), args, outs, moreOut: (t.outputs || []).length - outs.length };
}

/* ---- can_call planner ---- */
function planCall(t, ctx) {
  const req = [], block = [];
  const price = t.price_usdc;
  if (t.status !== "active") block.push("tool id is not active in the registry");
  if (t.access === "gated") {
    req.push("access predicate must approve caller wallet");
    if (!ctx.wallet) block.push("wallet required to evaluate predicate access");
  }
  if (t.has_auth && !ctx.auth) req.push("manifest declares SIWE authentication");
  if (t.has_x402) {
    req.push("x402 payment settled before 200 response");
    if (!ctx.x402) block.push("caller ctx disallows x402 payments");
    if (price != null && ctx.budget < price) block.push(`budget ${ctx.budget.toFixed(2)} < price ${fmtPrice(price)} USDC`);
  }
  let status = "callable";
  if (block.length) status = "blocked";
  else if (t.access === "gated" || t.has_auth || t.has_x402) status = "conditional";
  return { status, req, block, price };
}

/* ---- curl + resolve json ---- */
function buildCurl(t) {
  const body = {};
  (t.inputs || []).forEach((f) => {
    body[f.name] = f.type === "string" ? "<" + f.name + ">" : f.type === "array" ? [] : f.type === "number" ? 0 : f.type === "boolean" ? false : null;
  });
  const L = [`curl -X POST ${t.endpoint || "https://endpoint.invalid"} \\`, `  -H 'content-type: application/json' \\`];
  if (t.has_x402) L.push(`  -H 'x-payment: <x402-signed-token>' \\`);
  L.push(`  -d '${JSON.stringify(body)}'`);
  return L.join("\n");
}
function buildResolveJSON(t) {
  return JSON.stringify({
    chain_id: t.chain_id || REG.chain_id, chain_name: chainName(t.chain_id || REG.chain_id),
    registry: REG.registry, tool_id: t.id, name: t.name, endpoint: t.endpoint,
    method: t.method || "POST", access: t.access, predicate_type: t.predicate_type || "unknown",
    requires_x402: t.has_x402, requires_auth: t.has_auth,
    price_usdc: t.price_usdc, manifest_verified: t.manifest_status === "verified",
  }, null, 2);
}

/* ---- query parser ---- */
const KNOWN_STATUS = { active: "active", deregistered: "deregistered", dereg: "deregistered" };
const KNOWN_ACCESS = { open: "open", gated: "gated", keyed: "gated" };
function parseQuery(str) {
  const toks = (str || "").trim().split(/\s+/).filter(Boolean);
  const f = { status: null, access: null, verify: null, x402: null, free: null, price: null };
  const chips = [], terms = [];
  for (const raw of toks) {
    const low = raw.toLowerCase();
    let m;
    if (low.startsWith("#") && low.length > 1) { chips.push({ kind: "tag", key: "#", val: low.slice(1), raw }); continue; }
    if ((m = low.match(/^status:(\w+)$/)) && KNOWN_STATUS[m[1]]) { f.status = KNOWN_STATUS[m[1]]; chips.push({ kind: "status", key: "status", val: KNOWN_STATUS[m[1]], raw }); continue; }
    if ((m = low.match(/^access:(\w+)$/)) && KNOWN_ACCESS[m[1]]) { f.access = KNOWN_ACCESS[m[1]]; chips.push({ kind: "access", key: "access", val: KNOWN_ACCESS[m[1]], raw }); continue; }
    if (low === "verified") { f.verify = "verified"; chips.push({ kind: "verify", key: "manifest", val: "verified", raw }); continue; }
    if (low === "mismatch") { f.verify = "hash_mismatch"; chips.push({ kind: "verify", key: "manifest", val: "mismatch", raw }); continue; }
    if (low === "x402") { f.x402 = true; chips.push({ kind: "x402", key: "settle", val: "x402", raw }); continue; }
    if (low === "free") { f.free = true; chips.push({ kind: "verify", key: "settle", val: "free", raw }); continue; }
    if ((m = low.match(/^price([<>]=?|=)(\d*\.?\d+)$/))) { f.price = { op: m[1], n: parseFloat(m[2]) }; chips.push({ kind: "x402", key: "price", val: m[1] + m[2], raw }); continue; }
    terms.push(low);
  }
  return { f, chips, terms };
}
function matchQuery(t, parsed) {
  const { f, terms } = parsed;
  if (f.status && t.status !== f.status) return false;
  if (f.access && t.access !== f.access) return false;
  if (f.verify && t.manifest_status !== f.verify) return false;
  if (f.x402 && !t.has_x402) return false;
  if (f.free && t.has_x402) return false;
  if (f.price) {
    const p = t.price_usdc == null ? 0 : t.price_usdc;
    const { op, n } = f.price;
    if (op === "<" && !(p < n)) return false;
    if (op === "<=" && !(p <= n)) return false;
    if (op === ">" && !(p > n)) return false;
    if (op === ">=" && !(p >= n)) return false;
    if (op === "=" && !(p === n)) return false;
  }
  if (terms.length) {
    const hay = [t.name, t.description, t.endpoint, t.creator, (t.tags || []).join(" ")].join(" ").toLowerCase();
    if (!terms.every((tm) => hay.includes(tm))) return false;
  }
  return true;
}
/* add/remove a whole token from the query string */
function toggleToken(str, token) {
  const toks = (str || "").trim().split(/\s+/).filter(Boolean);
  const i = toks.findIndex((x) => x.toLowerCase() === token.toLowerCase());
  if (i >= 0) toks.splice(i, 1); else toks.push(token);
  return toks.join(" ");
}
function removeRaw(str, raw) {
  const toks = (str || "").trim().split(/\s+/).filter(Boolean);
  const i = toks.indexOf(raw);
  if (i >= 0) toks.splice(i, 1);
  return toks.join(" ");
}

/* ---- json syntax highlight ---- */
function hljson(obj) {
  const j = JSON.stringify(obj, null, 2).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return j.replace(/("(\\u[a-zA-Z0-9]{4}|\\[^u]|[^\\"])*"(\s*:)?|\b(true|false|null)\b|-?\d+(?:\.\d+)?)/g,
    (m) => { let c = "n"; if (/^"/.test(m)) c = /:$/.test(m) ? "k" : "s"; else if (/true|false|null/.test(m)) c = "b"; return `<span class="${c}">${m}</span>`; });
}

/* ---- toast + copy ---- */
const ToastCtx = createContext(() => {});
function ToastProvider({ children }) {
  const [msg, setMsg] = useState(null); const tr = useRef(null);
  const push = useCallback((m) => { setMsg(m); clearTimeout(tr.current); tr.current = setTimeout(() => setMsg(null), 1700); }, []);
  return <ToastCtx.Provider value={push}>{children}<div className="toast" data-show={!!msg}><span className="g">{Svg.check}</span><span>{msg}</span></div></ToastCtx.Provider>;
}
const useToast = () => useContext(ToastCtx);
function copyText(text) {
  try { if (navigator.clipboard?.writeText) return navigator.clipboard.writeText(text); } catch (e) {}
  const ta = document.createElement("textarea"); ta.value = text; ta.style.position = "fixed"; ta.style.opacity = "0";
  document.body.appendChild(ta); ta.select(); try { document.execCommand("copy"); } catch (e) {} document.body.removeChild(ta);
  return Promise.resolve();
}

Object.assign(window, {
  Svg, VG, short, fmtPrice, relTime, accessOf, signature, planCall, buildCurl, buildResolveJSON,
  parseQuery, matchQuery, toggleToken, removeRaw, hljson, ToastProvider, useToast, copyText, REG,
  CHAIN_NAMES, chainName,
});

// cat-helpers.jsx - plain-language ("human") + machine-payload ("agent") layers
const { useState: useCS, useMemo: useCM, useRef: useCR, useEffect: useCE } = React;

/* ---------- icons (line, inherit currentColor) ---------- */
const Ico = {
  human: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="8" r="3.4"/><path d="M5.5 20a6.5 6.5 0 0 1 13 0"/></svg>,
  agent: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><rect x="4" y="6" width="16" height="13" rx="2.2"/><path d="M9 3v3M15 3v3M9.5 12.5h0M14.5 12.5h0M9 16h6"/></svg>,
  chev: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m6 9 6 6 6-6"/></svg>,
  check: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M20 6 9 17l-5-5"/></svg>,
  dot: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M5 12h.01M12 12h.01M19 12h.01"/></svg>,
  block: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="9"/><path d="m6 6 12 12"/></svg>,
  copy: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a1 1 0 0 1 1-1h9"/></svg>,
  ext: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><path d="M14 4h6v6M20 4l-9 9M18 13v6H5V6h6"/></svg>,
  verified: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><path d="m9 12 2 2 4-4"/><path d="M12 3l2.3 1.7 2.8-.2.9 2.7 2.3 1.6-.9 2.7.9 2.7-2.3 1.6-.9 2.7-2.8-.2L12 21l-2.3-1.7-2.8.2-.9-2.7L3.7 15l.9-2.7-.9-2.7 2.3-1.6.9-2.7 2.8.2z"/></svg>,
  mismatch: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><path d="M12 8v5M12 16h.01"/><circle cx="12" cy="12" r="9"/></svg>,
  gate: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><rect x="4.5" y="10.5" width="15" height="9.5" rx="1.6"/><path d="M8 10.5V8a4 4 0 0 1 8 0v2.5"/></svg>,
  bolt: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><path d="M13 2 4 14h7l-1 8 9-12h-7z"/></svg>,
  arrow: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>,
  sun: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>,
  moon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg>,
};

/* ---------- plain-language helpers ---------- */
const TYPE_WORD = { string: "text", number: "number", boolean: "true / false", array: "list", object: "object" };

// One-line "what it costs / how you reach it" plain summary line.
function priceLine(t) {
  if (!t.has_x402) return { label: "Free", per: "no payment", free: true };
  const p = fmtPrice(t.price_usdc);
  if (p == null) return { label: "x402", per: "metered", free: false };
  return { label: p, per: "per call · USDC", free: false };
}

// Human-readable requirement checklist derived from the same planCall logic.
function humanChecklist(t, ctx) {
  const plan = planCall(t, ctx);
  const items = [];

  // status
  if (t.status !== "active") {
    items.push({ state: "block", text: <>This capability was <b>deregistered</b> from the index and can no longer be called.</> });
  } else {
    items.push({ state: "met", text: <>Active in the registry on {chainName(t.chain_id || 8453)}.</> });
  }

  // manifest integrity
  if (t.manifest_status === "verified") {
    items.push({ state: "met", text: <>Manifest <b>verified</b> - the published spec matches its on-chain hash.</> });
  } else {
    items.push({ state: "block", text: <>Manifest <b>hash mismatch</b> - the live spec doesn't match what was registered. Treat results with caution.</> });
  }

  // access predicate
  if (t.access === "gated") {
    const reqLabels = (t.access_reqs || []).map((r) => r.label).filter(Boolean);
    const cond = reqLabels.length ? reqLabels.join(", ") : "an on-chain predicate";
    if (!ctx.wallet) {
      items.push({ state: "req", text: <>Gated access - your wallet must satisfy <b>{cond}</b>. Connect a wallet to check.</> });
    } else {
      items.push({ state: "req", text: <>Gated access - caller wallet must satisfy <b>{cond}</b>.</> });
    }
  }

  // auth
  if (t.has_auth) {
    items.push({ state: ctx.auth ? "met" : "req", text: ctx.auth
      ? <>Wallet sign-in (SIWE) available.</>
      : <>Requires wallet sign-in (<b>SIWE</b>) before the call.</> });
  }

  // payment
  if (t.has_x402) {
    const p = fmtPrice(t.price_usdc);
    if (!ctx.x402) {
      items.push({ state: "block", text: <>Paid via <b>x402</b>{p != null ? <> ({p} USDC/call)</> : null} - but your caller settings disallow payments.</> });
    } else if (p != null && ctx.budget < t.price_usdc) {
      items.push({ state: "block", text: <>Costs <b>{p} USDC</b> per call - above your {ctx.budget.toFixed(2)} USDC budget.</> });
    } else {
      items.push({ state: ctx.x402 ? "met" : "req", text: <>Settles <b>{p != null ? p + " USDC" : "payment"}</b> per call over x402 before responding.</> });
    }
  } else {
    items.push({ state: "met", text: <>No payment required.</> });
  }

  return { plan, items };
}

const VERDICT_COPY = {
  callable:   { label: "Ready to call",            line: "Every requirement is satisfied under your current caller context." },
  conditional:{ label: "Callable with conditions", line: "Reachable once the conditions below are met." },
  blocked:    { label: "Not callable",             line: "Blocked under your current caller context." },
};

/* ---------- agent-payload builders (legible JSON) ---------- */
function resolveRecord(t) {
  return {
    chain_id: t.chain_id || REG.chain_id,
    chain_name: chainName(t.chain_id || REG.chain_id),
    registry: REG.registry,
    tool_id: t.id,
    name: t.name,
    endpoint: t.endpoint,
    method: "POST",
    access: t.access,
    predicate_type: t.predicate_type || "unknown",
    requires_auth: t.has_auth,
    settlement: t.has_x402 ? "x402" : "none",
    price_usdc: t.price_usdc,
    manifest_verified: t.manifest_status === "verified",
    tags: t.tags || [],
  };
}
function canCallRecord(t, ctx) {
  const plan = planCall(t, ctx);
  const steps = [];
  if (plan.status !== "blocked") {
    if (t.has_auth) steps.push("sign SIWE challenge");
    if (t.access === "gated") steps.push("present predicate proof");
    if (t.has_x402) steps.push("attach x-payment (x402)");
    steps.push("POST inputs -> 200");
  }
  return {
    tool_id: t.id,
    status: plan.status,
    caller: { wallet: ctx.wallet ? "0x...connected" : null, budget_usdc: ctx.budget, allow_x402: ctx.x402, has_auth: ctx.auth },
    requirements: plan.req,
    blockers: plan.block,
    invocation: steps,
  };
}
function invokeSnippet(t) {
  const body = {};
  (t.inputs || []).forEach((f) => {
    body[f.name] = f.type === "string" ? "<" + f.name + ">" : f.type === "array" ? [] : f.type === "number" ? 0 : f.type === "boolean" ? false : null;
  });
  const lines = [];
  lines.push(`POST ${t.endpoint || "https://endpoint.invalid"}`);
  lines.push(`content-type: application/json`);
  if (t.has_auth) lines.push(`authorization: Bearer <siwe-session>`);
  if (t.has_x402) lines.push(`x-payment: <x402-signed-token>`);
  lines.push("");
  lines.push(JSON.stringify(body, null, 2));
  return lines.join("\n");
}

// Highlight a header+JSON invocation block.
function hlInvoke(text) {
  const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return esc(text).split("\n").map((ln) => {
    if (/^POST /.test(ln)) return `<span class="b">POST</span> <span class="k">${ln.slice(5)}</span>`;
    if (/^[a-z-]+:/.test(ln)) { const i = ln.indexOf(":"); return `<span class="cm">${ln.slice(0, i)}</span>:${ln.slice(i + 1)}`; }
    return ln
      .replace(/("(?:[^"\\]|\\.)*")(\s*:)/g, '<span class="k">$1</span>$2')
      .replace(/:\s*("(?:[^"\\]|\\.)*")/g, ': <span class="s">$1</span>')
      .replace(/\b(true|false|null)\b/g, '<span class="b">$1</span>')
      .replace(/:\s*(-?\d+(?:\.\d+)?)/g, ': <span class="n">$1</span>');
  }).join("\n");
}

/* ---------- small shared components ---------- */
function CodeBlock({ verb, endpoint, html, raw, push }) {
  return (
    <div className="codeblock">
      <div className="cb-head">
        <span className="verb">{verb}</span>
        {endpoint && <span className="ep">{endpoint}</span>}
        <button className="copy" onClick={() => { copyText(raw); push("Copied " + verb.toLowerCase()); }}>{Ico.copy} copy</button>
      </div>
      <pre dangerouslySetInnerHTML={{ __html: html }} />
    </div>
  );
}

Object.assign(window, {
  Ico, TYPE_WORD, priceLine, humanChecklist, VERDICT_COPY,
  resolveRecord, canCallRecord, invokeSnippet, hlInvoke, CodeBlock,
});

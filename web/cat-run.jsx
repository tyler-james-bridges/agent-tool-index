// cat-run.jsx - live execution: wallet hook, WalletButton, and RunPanel.
// Tool calls run for real on the site (free + x402) via window.ATI (wallet.js).
const { useState: useRS, useEffect: useRE, useRef: useRR } = React;

// Subscribe to the vanilla window.ATI wallet state.
function useWallet() {
  const [w, setW] = useRS(() => (window.ATI ? window.ATI.getState() : { hasProvider: false, address: null, chainId: null, connecting: false }));
  useRE(() => {
    if (!window.ATI) return;
    setW(window.ATI.getState());
    const off = window.ATI.subscribe(setW);
    return off;
  }, []);
  return w;
}

async function connectWallet(push) {
  if (!window.ATI) return;
  try { await window.ATI.connect(); }
  catch (e) { if (push) push(e && e.message ? e.message : "Wallet connection failed"); }
}

async function connectKey(key, push) {
  if (!window.ATI) return;
  try { await window.ATI.connect(key); }
  catch (e) { if (push) push(e && e.message ? e.message : "Wallet connection failed"); }
}

function WalletButton() {
  const w = useWallet();
  const push = useToast();
  const [picking, setPicking] = useRS(false);
  const net = w.chainIdNum && window.ATI ? Object.values(window.ATI.NETWORKS).find((n) => n.id === w.chainIdNum) : null;
  const wallets = window.ATI ? window.ATI.listWallets() : [];

  useRE(() => {
    if (!picking) return;
    const close = (e) => { if (!e.target.closest || !e.target.closest(".walletwrap")) setPicking(false); };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [picking]);

  if (w.address) {
    return (
      <div className="walletwrap">
        <button className="walletbtn" data-on="true" onClick={() => copyText(w.address).then(() => push("Address copied"))}
          title={w.address + (net ? " · " + net.name : "")}>
          <span className="wdot" />
          <span className="waddr">{short(w.address, 4)}</span>
          {net && <span className="wnet">{net.name}</span>}
        </button>
      </div>
    );
  }

  function onClick() {
    if (!wallets.length) {
      if (window.ATI && window.ATI.isMobile()) {
        setPicking((v) => !v);                                   // mobile → show wallet app links
        return;
      }
      connectWallet(push);                                       // desktop → shows the "no wallet" message
      return;
    }
    if (wallets.length === 1) { connectKey(wallets[0].key, push); return; }
    setPicking((v) => !v);                                       // multiple → let the user choose
  }

  const isMobile = window.ATI && window.ATI.isMobile();
  const noWalletTitle = isMobile ? "Open in a wallet app" : "No browser wallet detected";
  const noWalletText = isMobile ? "Connect" : "No wallet";

  return (
    <div className="walletwrap">
      <button className="walletbtn" onClick={onClick} disabled={w.connecting}
        title={wallets.length ? "Connect your wallet" : noWalletTitle}>
        <span className="wdot off" />
        <span className="wlabel">{w.connecting ? "Connecting…" : wallets.length ? "Connect wallet" : noWalletText}</span>
        <span className="wlabel-short" aria-hidden="true">{w.connecting ? "…" : wallets.length ? "Connect" : noWalletText}</span>
      </button>
      {picking && (
        <div className="walletmenu">
          <div className="wm-head">{!wallets.length && isMobile ? "Open in a wallet app" : "Choose a wallet"}</div>
          {!wallets.length && isMobile ? (
            window.ATI.mobileWalletLinks().map((link) => (
              <a className="wm-item" key={link.name} href={link.url}>
                <span className="wm-ico ph" />
                <span className="wm-name">{link.name}</span>
              </a>
            ))
          ) : (
            wallets.map((wal) => (
              <button className="wm-item" key={wal.key} onClick={() => { setPicking(false); connectKey(wal.key, push); }}>
                {wal.icon ? <img className="wm-ico" src={wal.icon} alt="" /> : <span className="wm-ico ph" />}
                <span className="wm-name">{wal.name}</span>
              </button>
            ))
          )}
        </div>
      )}
    </div>
  );
}

function defaultInputs(t) {
  const o = {};
  (t.inputs || []).forEach((f) => { o[f.name] = f.default != null ? String(f.default) : ""; });
  return o;
}

// A JSON-schema type may be a string ("integer") or an array (["integer","null"]).
function typeIs(ft, name) { return ft === name || (Array.isArray(ft) && ft.includes(name)); }

function coerce(field, val) {
  if (val === "" || val == null) return undefined;
  const ft = field.type;
  if (typeIs(ft, "integer") || typeIs(ft, "number")) { const n = Number(val); return Number.isNaN(n) ? val : n; }
  if (typeIs(ft, "boolean")) return val === true || val === "true";
  if (typeIs(ft, "array") || typeIs(ft, "object")) { try { return JSON.parse(val); } catch (e) { return val; } }
  return val;
}

// Which control a field gets. enum wins so we can always offer the choices.
function fieldKind(f) {
  if (Array.isArray(f.enum) && f.enum.length) return "enum";
  if (typeIs(f.type, "boolean")) return "bool";
  if (typeIs(f.type, "array") || typeIs(f.type, "object")) return "json";
  if (typeIs(f.type, "integer") || typeIs(f.type, "number")) return "number";
  return "text";
}

// Per-field validation -> short error string, or null when acceptable.
// enum values are suggestions, not a hard constraint — humans may type freeform.
function fieldError(f, raw) {
  const v = (raw == null ? "" : String(raw)).trim();
  if (!v) return f.required ? "required" : null;
  const kind = fieldKind(f);
  if (kind === "number") {
    const n = Number(v);
    if (Number.isNaN(n)) return "must be a number";
    if (f.minimum != null && n < f.minimum) return "min " + f.minimum;
    if (f.maximum != null && n > f.maximum) return "max " + f.maximum;
  }
  if (kind === "json") { try { JSON.parse(v); } catch (e) { return "must be valid JSON"; } }
  if (f.pattern) { try { if (!new RegExp(f.pattern).test(v)) return "doesn't match expected format"; } catch (e) { /* bad regex in manifest */ } }
  return null;
}

function exampleOf(f) {
  if (f.examples && f.examples.length) return f.examples[0];
  if (Array.isArray(f.enum) && f.enum.length) return f.enum[0];
  return null;
}

// One schema-aware field: visible options to pick + a box to type freeform.
function SmartField({ f, idp, value, onChange }) {
  const kind = fieldKind(f);
  const err = fieldError(f, value);
  const ex = exampleOf(f);
  const ph = ex != null ? "e.g. " + (typeof ex === "string" ? ex : JSON.stringify(ex))
    : (TYPE_WORD[f.type] || (Array.isArray(f.type) ? f.type.join(" | ") : f.type));
  const listId = "dl-" + idp + "-" + f.name;

  return (
    <label className="run-field">
      <span className="rf-name">
        {f.name}
        {f.required ? <span className="rf-req">*</span> : <span className="rf-opt"> opt</span>}
        {f.default != null && <span className="rf-def">default: {String(f.default)}</span>}
        {(f.minimum != null || f.maximum != null) && (
          <span className="rf-range">{f.minimum != null ? f.minimum : "−∞"}…{f.maximum != null ? f.maximum : "∞"}</span>
        )}
      </span>

      {kind === "bool" ? (
        <div className="rf-chips">
          {["true", "false"].map((opt) => (
            <button type="button" key={opt} className={"rf-chip" + (String(value) === opt ? " on" : "")}
              onClick={() => onChange(value === opt ? "" : opt)}>{opt}</button>
          ))}
        </div>
      ) : kind === "json" ? (
        <textarea className={"rf-input rf-json" + (err ? " bad" : "")} spellCheck="false" rows={3}
          placeholder={ph} value={value} onChange={(e) => onChange(e.target.value)} />
      ) : (
        <input className={"rf-input" + (err ? " bad" : "")} spellCheck="false" autoComplete="off"
          type={kind === "number" ? "number" : "text"}
          min={f.minimum != null ? f.minimum : undefined} max={f.maximum != null ? f.maximum : undefined}
          list={kind === "enum" ? listId : undefined}
          placeholder={ph} value={value} onChange={(e) => onChange(e.target.value)} />
      )}

      {kind === "enum" && (
        <React.Fragment>
          <datalist id={listId}>{f.enum.map((o) => <option key={String(o)} value={String(o)} />)}</datalist>
          <div className="rf-chips">
            {f.enum.map((o) => (
              <button type="button" key={String(o)} className={"rf-chip" + (String(value) === String(o) ? " on" : "")}
                onClick={() => onChange(value === String(o) ? "" : String(o))}>{String(o)}</button>
            ))}
          </div>
        </React.Fragment>
      )}

      <span className="rf-meta">
        {err && <span className="rf-err">{err}</span>}
        {f.description && <span className="rf-hint">{f.description}</span>}
      </span>
    </label>
  );
}

function RunPanel({ t }) {
  const w = useWallet();
  const push = useToast();
  const fields = t.inputs || [];
  const [vals, setVals] = useRS(() => defaultInputs(t));
  const [running, setRunning] = useRS(false);
  const [res, setRes] = useRS(null);
  const [mode, setMode] = useRS("form");   // "form" | "json"
  const [raw, setRaw] = useRS("");          // raw JSON body (json mode / no-schema tools)
  const dereg = t.status !== "active";

  function setField(name, v) { setVals((s) => ({ ...s, [name]: v })); }

  function buildPayload() {
    const out = {};
    fields.forEach((f) => { const c = coerce(f, vals[f.name]); if (c !== undefined) out[f.name] = c; });
    return out;
  }

  // Validation: hard errors block the run in form mode; raw JSON must parse.
  const errs = fields.map((f) => fieldError(f, vals[f.name])).filter(Boolean);
  const rawErr = raw.trim() && (() => { try { JSON.parse(raw); return false; } catch (e) { return true; } })();
  const blocked = mode === "json" ? !!rawErr : errs.length > 0;

  function payload() {
    if (mode === "json") { try { return raw.trim() ? JSON.parse(raw) : {}; } catch (e) { return {}; } }
    return buildPayload();
  }

  // Toggle between the smart form and a raw JSON body, carrying state across.
  function toJson() { setRaw(JSON.stringify(buildPayload(), null, 2)); setMode("json"); }
  function toForm() {
    try {
      const obj = raw.trim() ? JSON.parse(raw) : {};
      setVals((s) => { const n = { ...s }; Object.keys(obj).forEach((k) => { n[k] = typeof obj[k] === "string" ? obj[k] : JSON.stringify(obj[k]); }); return n; });
    } catch (e) { /* keep current form values if raw is unparseable */ }
    setMode("form");
  }

  async function run() {
    if (!window.ATI) { push("Execution unavailable"); return; }
    setRunning(true); setRes(null);
    try {
      const body = payload();
      let r = await window.ATI.runTool(t, body);
      if (r.needWallet) {
        push("Connect a wallet to pay " + r.price_usdc + " USDC");
        await connectWallet(push);
        if (window.ATI.getState().address) r = await window.ATI.runTool(t, body);
      }
      setRes(r);
      if (r.paid) push("Paid " + r.price_usdc + " USDC · tool ran");
      else if (r.ok) push("Tool ran");
    } catch (e) {
      setRes({ ok: false, status: 0, error: e && e.message ? e.message : String(e) });
    } finally {
      setRunning(false);
    }
  }

  const label = dereg ? "Deregistered — cannot run"
    : running ? "Running…"
    : t.has_x402 ? (w.address ? ("Pay " + (fmtPrice(t.price_usdc) || "") + " USDC & run") : "Connect wallet & run")
    : "Run now";

  return (
    <div className="runpanel">
      <div className="run-head">
        <span className="run-dh">Run it here</span>
        {t.has_x402 && <span className="run-cost">x402 · {fmtPrice(t.price_usdc) ? fmtPrice(t.price_usdc) + " USDC" : "metered"} per call</span>}
        {t.has_auth && <span className="run-cost warn">requires sign-in (SIWE)</span>}
        {fields.length > 0 && (
          <button type="button" className="run-modeswap" onClick={() => (mode === "form" ? toJson() : toForm())}>
            {mode === "form" ? "{ } edit as JSON" : "▤ back to form"}
          </button>
        )}
      </div>

      {mode === "json" || fields.length === 0 ? (
        <div className="run-fields">
          {fields.length === 0 && (
            <div className="run-noschema">
              No inputs are declared in this tool's manifest. It runs with an empty body — or send a custom JSON body below if the endpoint accepts one.
            </div>
          )}
          <label className="run-field">
            <span className="rf-name">request body <span className="rf-opt">JSON</span></span>
            <textarea className={"rf-input rf-json" + (rawErr ? " bad" : "")} spellCheck="false" rows={fields.length ? 6 : 4}
              placeholder={"{\n  \n}"} value={raw} onChange={(e) => setRaw(e.target.value)} />
            <span className="rf-meta">{rawErr ? <span className="rf-err">invalid JSON</span> : <span className="rf-hint">sent verbatim as the request body</span>}</span>
          </label>
        </div>
      ) : (
        <div className="run-fields">
          {fields.map((f) => (
            <SmartField key={f.name} f={f} idp={String(t.id)} value={vals[f.name]} onChange={(v) => setField(f.name, v)} />
          ))}
        </div>
      )}

      <div className="run-actions">
        <button className="run-go" disabled={running || dereg || blocked} onClick={run}>{Ico.bolt}{label}</button>
        {blocked && !running && <span className="run-hint">{mode === "json" ? "Fix the JSON to run." : "Fill required fields to run."}</span>}
        {!w.hasProvider && t.has_x402 && <span className="run-hint">No browser wallet detected — install one to settle x402.</span>}
      </div>

      {res && (
        <div className={"run-result " + (res.ok ? "ok" : "err")}>
          <div className="rr-head">
            <span className={"rr-status " + (res.ok ? "ok" : "err")}>{res.ok ? "200 OK" : (res.status ? res.status + " ·" : "") + " " + (res.error || "error")}</span>
            {res.paid && res.payment && res.payment.transaction && (
              <a className="rr-tx" href={"https://basescan.org/tx/" + res.payment.transaction} target="_blank" rel="noreferrer">
                {Ico.ext}settled {short(res.payment.transaction, 6)}
              </a>
            )}
          </div>
          {res.data != null && (
            <pre className="rr-body" dangerouslySetInnerHTML={{
              __html: hljson(typeof res.data === "string" ? res.data : truncateData(res.data)),
            }} />
          )}
          {res.needWallet && <div className="rr-note">Connect a wallet (top right) to pay and run.</div>}
        </div>
      )}
    </div>
  );
}

// Keep huge responses legible in the panel.
function truncateData(obj) {
  try {
    const s = JSON.stringify(obj);
    if (s.length <= 6000) return obj;
    return { _truncated: true, bytes: s.length, preview: s.slice(0, 6000) + "…" };
  } catch (e) { return obj; }
}

Object.assign(window, { useWallet, WalletButton, RunPanel, connectWallet });

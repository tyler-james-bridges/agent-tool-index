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
  (t.inputs || []).forEach((f) => { o[f.name] = ""; });
  return o;
}

function coerce(field, val) {
  if (val === "" || val == null) return undefined;
  if (field.type === "number") { const n = Number(val); return Number.isNaN(n) ? val : n; }
  if (field.type === "boolean") return val === true || val === "true";
  if (field.type === "array" || field.type === "object") { try { return JSON.parse(val); } catch (e) { return val; } }
  return val;
}

function RunPanel({ t }) {
  const w = useWallet();
  const push = useToast();
  const [vals, setVals] = useRS(() => defaultInputs(t));
  const [running, setRunning] = useRS(false);
  const [res, setRes] = useRS(null);
  const dereg = t.status !== "active";

  function setField(name, v) { setVals((s) => ({ ...s, [name]: v })); }

  function buildPayload() {
    const out = {};
    (t.inputs || []).forEach((f) => { const c = coerce(f, vals[f.name]); if (c !== undefined) out[f.name] = c; });
    return out;
  }

  async function run() {
    if (!window.ATI) { push("Execution unavailable"); return; }
    setRunning(true); setRes(null);
    try {
      let r = await window.ATI.runTool(t, buildPayload());
      if (r.needWallet) {
        push("Connect a wallet to pay " + r.price_usdc + " USDC");
        await connectWallet(push);
        if (window.ATI.getState().address) r = await window.ATI.runTool(t, buildPayload());
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
      </div>

      {(t.inputs || []).length > 0 && (
        <div className="run-fields">
          {(t.inputs || []).map((f) => (
            <label className="run-field" key={f.name}>
              <span className="rf-name">{f.name}{f.required ? <span className="rf-req">*</span> : <span className="rf-opt"> opt</span>}</span>
              <input className="rf-input" spellCheck="false" autoComplete="off"
                type={f.type === "number" ? "number" : "text"}
                placeholder={(TYPE_WORD[f.type] || f.type) + (f.description ? " · " + f.description : "")}
                value={vals[f.name]} onChange={(e) => setField(f.name, e.target.value)} />
            </label>
          ))}
        </div>
      )}

      <div className="run-actions">
        <button className="run-go" disabled={running || dereg} onClick={run}>{Ico.bolt}{label}</button>
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

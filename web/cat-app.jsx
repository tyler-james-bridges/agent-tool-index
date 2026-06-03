// cat-app.jsx - Capability Index: masthead, ask bar, Human/Agent lens, filters, caller ctx
const { useState: useS, useMemo: useM, useEffect: useE, useRef: useR } = React;

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "theme": "index",
  "accent": "#0000FF",
  "startLens": "human"
}/*EDITMODE-END*/;

const STAT = (() => {
  const s = { total: REG.tools.length, active: 0, dereg: 0, verified: 0, mismatch: 0, x402: 0, free: 0, gated: 0 };
  REG.tools.forEach((t) => {
    if (t.status === "active") s.active++; else s.dereg++;
    if (t.manifest_status === "verified") s.verified++; else s.mismatch++;
    if (t.has_x402) s.x402++; else s.free++;
    if (t.access === "gated") s.gated++;
  });
  return s;
})();

const PILLS = [
  { key: "free",                 lab: "Free",          tone: "ok",     count: STAT.free },
  { key: "x402",                 lab: "Paid · x402",   tone: "accent", count: STAT.x402 },
  { key: "access:gated",         lab: "Gated",         tone: "",       count: STAT.gated },
  { key: "verified",             lab: "Verified",      tone: "ok",     count: STAT.verified },
  { key: "mismatch",             lab: "Manifest issues", tone: "",     count: STAT.mismatch },
  { key: "status:deregistered",  lab: "Deregistered",  tone: "",       count: STAT.dereg },
];

const EXAMPLES = ["appraise an nft", "scan a wallet for risk", "resolve an ens name", "token burn stats"];

function LensSwitch({ lens, setLens }) {
  const wrap = useR(null);
  const refs = { human: useR(null), agent: useR(null) };
  const [glide, setGlide] = useS({ left: 4, width: 0 });
  useE(() => {
    const el = refs[lens].current; if (!el || !wrap.current) return;
    setGlide({ left: el.offsetLeft, width: el.offsetWidth });
  }, [lens]);
  useE(() => {
    function r() { const el = refs[lens].current; if (el) setGlide({ left: el.offsetLeft, width: el.offsetWidth }); }
    window.addEventListener("resize", r); return () => window.removeEventListener("resize", r);
  }, [lens]);
  return (
    <div className="lens" data-lens={lens}>
      <div className="lensswitch" ref={wrap}>
        <span className="lensglide" style={{ left: glide.left, width: glide.width }} />
        <button ref={refs.human} data-on={lens === "human"} onClick={() => setLens("human")}><span className="ico">{Ico.human}</span><span className="lbl">Human</span></button>
        <button ref={refs.agent} data-on={lens === "agent"} onClick={() => setLens("agent")}><span className="ico">{Ico.agent}</span><span className="lbl">Agent</span></button>
      </div>
    </div>
  );
}

function CallerBar({ ctx, setCtx }) {
  const set = (k, v) => setCtx((c) => ({ ...c, [k]: v }));
  return (
    <div className="callerbar">
      <span className="ttl">Calling as</span>
      <div className="ctxgroup">
        <button className="ctxtog" data-on={ctx.wallet} onClick={() => set("wallet", !ctx.wallet)}><span className="sw" />Wallet connected</button>
        <button className="ctxtog" data-on={ctx.auth} onClick={() => set("auth", !ctx.auth)}><span className="sw" />SIWE sign-in</button>
        <button className="ctxtog" data-on={ctx.x402} onClick={() => set("x402", !ctx.x402)}><span className="sw" />Allow x402</button>
        <span className="ctxnum">
          <span className="lab"><span className="cur">$</span></span>
          <input type="number" step="0.01" min="0" value={ctx.budget}
            onChange={(e) => set("budget", e.target.value === "" ? 0 : Math.max(0, parseFloat(e.target.value) || 0))} />
          <span className="lab" style={{ paddingLeft: 0, paddingRight: 12, color: "var(--faint)" }}>budget / call</span>
        </span>
      </div>
      <span className="note">Verdicts recompute live against this context.</span>
    </div>
  );
}

function App() {
  const [tw, setTweak] = useTweaks(TWEAK_DEFAULTS);
  const [q, setQ] = useS("");
  const [lens, setLens] = useS(TWEAK_DEFAULTS.startLens);
  const [callableOnly, setCallableOnly] = useS(false);
  const [sort, setSort] = useS({ field: "id", dir: "asc" });
  const [domain, setDomain] = useS(null);            // null = browse all domains
  const [openId, setOpenId] = useS(null);
  const [ctx, setCtx] = useS({ wallet: false, x402: true, auth: false, budget: 1.0 });
  const inputRef = useR(null);

  useE(() => { document.documentElement.setAttribute("data-theme", tw.theme); }, [tw.theme]);
  // Agent lens flips the whole document to the terminal palette (see index.html vars).
  useE(() => { document.documentElement.setAttribute("data-lens", lens); }, [lens]);
  useE(() => { document.documentElement.style.setProperty("--accent-raw", tw.accent); }, [tw.accent]);
  useE(() => { setLens(tw.startLens); }, [tw.startLens]);

  const parsed = useM(() => parseQuery(q), [q]);
  const domCounts = useM(() => domainCounts(REG.tools), []);
  const filtered = useM(() => {
    let list = REG.tools.filter((t) => matchQuery(t, parsed));
    if (callableOnly) list = list.filter((t) => planCall(t, ctx).status !== "blocked");
    if (domain) list = list.filter((t) => domainOf(t) === domain);
    const dir = sort.dir === "asc" ? 1 : -1;
    const rank = { callable: 0, conditional: 1, blocked: 2 };
    list = [...list].sort((a, b) => {
      if (sort.field === "price") { const pa = a.price_usdc == null ? -1 : a.price_usdc, pb = b.price_usdc == null ? -1 : b.price_usdc; return (pa - pb) * dir; }
      if (sort.field === "verdict") return (rank[planCall(a, ctx).status] - rank[planCall(b, ctx).status]) * dir || (a.id - b.id);
      return (a.id - b.id) * dir;
    });
    return list;
  }, [parsed, callableOnly, sort, ctx, domain]);

  // Browse mode: nothing narrowed shows domain "shelves" instead of one long list.
  const browseMode = !q.trim() && !callableOnly && !domain;
  const shelves = useM(() => {
    if (!browseMode) return [];
    return DOMAINS.map((d) => ({
      d, items: filtered.filter((t) => domainOf(t) === d.key),
    })).filter((s) => s.items.length);
  }, [browseMode, filtered]);

  function toggleCard(id) { setOpenId((cur) => (cur === id ? null : id)); }
  function resetAll() { setQ(""); setCallableOnly(false); setDomain(null); }
  function pickDomain(k) { setDomain(k); setQ(""); setCallableOnly(false); window.scrollTo({ top: 0 }); }
  function cycleSort() {
    const order = ["id", "verdict", "price"];
    if (sort.dir === "asc" && sort.field !== "id") setSort({ field: sort.field, dir: "desc" });
    else { const i = order.indexOf(sort.field); setSort({ field: order[(i + 1) % order.length], dir: "asc" }); }
  }
  const sortLab = { id: "registry order", verdict: "callability", price: "price" }[sort.field];
  const anyFilter = q.trim() || callableOnly || domain;
  const allOn = !anyFilter;

  return (
    <div data-lens={lens}>
      <header className="masthead">
        <div className="wrap">
          <div className="cmdbar">
            <a className="brand" href="/" aria-label="Agent Tool Index home">
              <span className="basemark" aria-hidden="true"></span>
              <span className="mark">Agent Tool <em>Index</em></span>
            </a>
            <div className="ask">
              <span className="q">⌕</span>
              <input ref={inputRef} value={q} onChange={(e) => setQ(e.target.value)} spellCheck="false" autoComplete="off"
                placeholder={"What do you need done?  e.g. " + EXAMPLES[0]} />
              {q.trim() && <button className="clr" onClick={() => setQ("")}>clear</button>}
            </div>
            <LensSwitch lens={lens} setLens={setLens} />
          </div>
        </div>
      </header>

      <main className="wrap">
        <div className="domnav">
          <button className="domchip all" data-on={!domain} onClick={() => pickDomain(null)}>
            <span className="di">{Ico.bolt}</span>
            <span className="dt"><span className="dn">All capabilities</span><span className="dc">{STAT.total} total</span></span>
          </button>
          {DOMAINS.map((d) => (
            <button className="domchip" key={d.key} data-on={domain === d.key} onClick={() => pickDomain(d.key)}>
              <span className="di">{d.icon}</span>
              <span className="dt"><span className="dn">{d.name}</span><span className="dc">{domCounts[d.key]} tools</span></span>
            </button>
          ))}
        </div>

        <div className="filterstrip">
          <div className="pills">
            <button className="pill" data-on={allOn} onClick={resetAll}>All <span className="ct">{STAT.total}</span></button>
            <button className="pill" data-tone="ok" data-on={callableOnly} onClick={() => setCallableOnly((v) => !v)}>
              <span className="dot" style={{ background: "var(--ok)" }} />Callable for me
            </button>
            {PILLS.map((p) => {
              const on = q.split(/\s+/).map((s) => s.toLowerCase()).includes(p.key.toLowerCase());
              return (
                <button className="pill" data-tone={p.tone} data-on={on} key={p.key} onClick={() => setQ(toggleToken(q, p.key))}>
                  {p.lab} <span className="ct">{p.count}</span>
                </button>
              );
            })}
          </div>
          <div className="fstrip-r">
            <span className="count"><b>{filtered.length}</b> of {STAT.total}</span>
            <button className="sortbtn" onClick={cycleSort}>order: <b>{sortLab} {sort.dir === "asc" ? "↑" : "↓"}</b></button>
          </div>
        </div>

        <CallerBar ctx={ctx} setCtx={setCtx} />

        {browseMode ? (
          <div className="shelves">
            {shelves.map(({ d, items }) => {
              const preview = items.slice(0, 4);
              const rest = items.length - preview.length;
              return (
                <section className="shelf" key={d.key}>
                  <div className="shelf-head">
                    <span className="si">{d.icon}</span>
                    <span className="stext">
                      <div className="sn">{d.name}</div>
                      <div className="sb">{d.blurb}</div>
                    </span>
                    <button className="sall" onClick={() => pickDomain(d.key)}>View all {items.length} {Ico.arrow}</button>
                  </div>
                  <div className="shelf-cards">
                    {preview.map((t) => (
                      <CapabilityCard key={t.id} t={t} ctx={ctx} lens={lens}
                        open={openId === t.id} onToggle={() => toggleCard(t.id)} onTag={(tg) => setQ(toggleToken(q, "#" + tg))} />
                    ))}
                  </div>
                  {rest > 0 && (
                    <button className="shelf-more" onClick={() => pickDomain(d.key)}>+ <b>{rest} more</b> in {d.name}</button>
                  )}
                </section>
              );
            })}
          </div>
        ) : (
          <div className="catalog">
            {filtered.length === 0 ? (
              <div className="empty">
                <div className="eh serif">Nothing in the index matches.</div>
                <div className="es">No registered capability satisfies the current ask and filters.</div>
                <button className="reset" onClick={resetAll}>Reset the view</button>
              </div>
            ) : filtered.map((t) => (
              <CapabilityCard key={t.id} t={t} ctx={ctx} lens={lens}
                open={openId === t.id} onToggle={() => toggleCard(t.id)} onTag={(tg) => setQ(toggleToken(q, "#" + tg))} />
            ))}
          </div>
        )}

        <footer className="pagefoot">
          <span className="mono">{short(REG.registry, 6)}</span>
          <span>Base · 8453</span>
          <span>{STAT.active} active · {STAT.verified} verified · synced {relTime(REG.synced_at)} ago</span>
          <a href="/llms.txt">llms.txt</a>
          <span style={{ marginLeft: "auto" }}>Read this index as a human, or as your agent does.</span>
        </footer>
      </main>

      <TweaksPanel>
        <TweakSection label="Surface" />
        <TweakRadio label="Theme" value={tw.theme} options={["index", "ink"]} onChange={(v) => setTweak("theme", v)} />
        <TweakColor label="Signal color" value={tw.accent} options={["#0000FF", "#cf3a16", "#117a4d", "#6a2cc4"]} onChange={(v) => setTweak("accent", v)} />
        <TweakSection label="Reading" />
        <TweakRadio label="Open as" value={tw.startLens} options={["human", "agent"]} onChange={(v) => setTweak("startLens", v)} />
      </TweaksPanel>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")).render(<ToastProvider><App /></ToastProvider>);

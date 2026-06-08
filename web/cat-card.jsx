// cat-card.jsx - CapabilityCard (human lens) + AgentTable (agent lens machine readout)
const { useState: useKS } = React;

function Glance({ t, onTag }) {
  return (
    <span className="glance">
      {(t.tags || []).slice(0, 3).map((tg) => (
        <span className="tagk" role="button" tabIndex={0} key={tg}
          onClick={(e) => { e.stopPropagation(); onTag(tg); }}
          onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); e.stopPropagation(); onTag(tg); } }}>
          <span className="h">#</span>{tg}
        </span>
      ))}
    </span>
  );
}

// The collapsed-card summary is the tool's own one-liner (high signal, unique
// per tool). The verdict still shows as the compact status dot + label.
function cardSummary(t) {
  return (t.description && t.description.trim())
    || VERDICT_COPY[planCall(t, {}).status].line;
}

function HumanLens({ t, ctx, push }) {
  const { items } = humanChecklist(t, ctx);
  const inputs = t.inputs || [];
  const outputs = t.outputs || [];
  const creator = short(t.creator, 5);

  return (
    <div className="dossier">
      {/* what it does */}
      <div className="dsec">
        <div className="dh">What it does</div>
        <div className="db"><p className="dprose">{t.description || "No description published in the manifest."}</p></div>
      </div>

      {/* can you call it */}
      <div className="dsec">
        <div className="dh">Can you call it</div>
        <div className="db">
          <div className="checklist">
            {items.map((it, i) => (
              <div className={"chk " + it.state} key={i}>
                <span className="ci">{it.state === "met" ? Ico.check : it.state === "block" ? Ico.block : Ico.dot}</span>
                <span className="ct">{it.text}</span>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* inputs */}
      {inputs.length > 0 && (
        <div className="dsec">
          <div className="dh">You provide</div>
          <div className="db">
            <div className="params">
              {inputs.map((f) => (
                <div className="param" key={f.name}>
                  <span className="pn">{f.name}</span>
                  <span className={"pt" + (f.required ? "" : " opt")}>{TYPE_WORD[f.type] || f.type}{f.required ? "" : " · optional"}</span>
                  <span className="pd">{f.description || ""}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* outputs */}
      {outputs.length > 0 && (
        <div className="dsec">
          <div className="dh">You get back</div>
          <div className="db"><div className="returns">{outputs.map((o) => <span className="retk" key={o}>{o}</span>)}</div></div>
        </div>
      )}

      {/* provenance */}
      <div className="dsec">
        <div className="dh">Published by</div>
        <div className="db">
          <p className="dprose" style={{ fontSize: 15 }}>
            <span className="mono" style={{ fontSize: 14 }}>{creator}</span>
            {" · ERC-8257 on " + chainName(t.chain_id || 8453)}
            {" · checked " + relTime(t.checked_at) + " ago"}
          </p>
        </div>
      </div>
    </div>
  );
}

function AgentLens({ t, ctx, push }) {
  const ep = (t.endpoint || "").replace(/^https?:\/\//, "");
  const resolveObj = resolveRecord(t);
  const canObj = canCallRecord(t, ctx);
  const inv = invokeSnippet(t);
  return (
    <div className="agent">
      <div className="agentnote">
        <span className="d">readout</span>
        <span>The exact records an agent receives when it resolves, plans, and invokes this capability.</span>
      </div>

      <CodeBlock verb="GET · resolve" endpoint={"/api/tools/" + t.id}
        html={hljson(resolveObj)} raw={JSON.stringify(resolveObj, null, 2)} push={push} />

      <CodeBlock verb={"POST · can_call → " + canObj.status} endpoint={"/api/tools/" + t.id + "/can_call"}
        html={hljson(canObj)} raw={JSON.stringify(canObj, null, 2)} push={push} />

      <CodeBlock verb="invoke" endpoint={ep || "endpoint.invalid"}
        html={hlInvoke(inv)} raw={inv} push={push} />

      <RunPanel t={t} />

      {/* integrity hashes */}
      <div className="agentnote">
        <span className="d">integrity</span>
        <span>{t.manifest_status === "verified"
          ? "Keccak(JCS(manifest)) equals the on-chain manifestHash."
          : "Computed hash differs from the registered manifestHash - verification failed."}</span>
      </div>
      <CodeBlock verb="hash" endpoint={t.manifest_status === "verified" ? "verified" : "mismatch"}
        html={hljson({ on_chain: t.manifest_hash, computed: t.computed_hash, match: t.manifest_status === "verified" })}
        raw={JSON.stringify({ on_chain: t.manifest_hash, computed: t.computed_hash, match: t.manifest_status === "verified" }, null, 2)}
        push={push} />
    </div>
  );
}

function CapabilityCard({ t, ctx, open, onToggle, onTag }) {
  const push = useToast();
  const plan = planCall(t, ctx);
  const pl = priceLine(t);
  const ver = t.manifest_status === "verified";
  const gated = t.access === "gated";
  const dereg = t.status === "deregistered";

  function copyRecord(e) {
    e.stopPropagation();
    copyText(JSON.stringify(resolveRecord(t), null, 2));
    push("Copied tool record");
  }

  return (
    <div className="card" data-open={open} data-dereg={dereg} data-tool-id={t.uid}>
      <button className="chead" onClick={onToggle} aria-expanded={open}>
        <span className="cverdict">
          <span className={"vdot " + plan.status} />
          <span className={"vl " + plan.status}>{plan.status === "callable" ? "ready" : plan.status === "conditional" ? "conditions" : "blocked"}</span>
        </span>

        <span className="ctitle">
          <span className="nm">
            <span className={dereg ? "strike" : ""}>{t.name}</span>
            <span className="idx">#{String(t.id).padStart(2, "0")}</span>
            <span className="chain-badge" data-chain={t.chain_id}>{chainName(t.chain_id || 8453)}</span>
          </span>
          <span className="sum">{cardSummary(t)}</span>
          <Glance t={t} onTag={onTag} />
        </span>

        <span className="cmeta">
          <span className={"price " + (pl.free ? "free" : "")}>
            {!pl.free && <span className="cur">$</span>}{pl.label}
            <span className="per">{pl.per}</span>
          </span>
          <span style={{ display: "flex", gap: 6 }}>
            {gated && <span className="badge gate">{Ico.gate} gated</span>}
            <span className={"badge " + (ver ? "ver" : "mis")}>{ver ? Ico.verified : Ico.mismatch}{ver ? "verified" : "mismatch"}</span>
          </span>
          <span className="chev">{Ico.chev}</span>
        </span>
      </button>

      {open && (
        <div className="cbody">
          <div className="cbody-in">
            <HumanLens t={t} ctx={ctx} push={push} />

            <RunPanel t={t} />

            <div className="cfoot">
              <button className="cact primary" disabled={plan.status === "blocked"} onClick={(e) => { e.stopPropagation(); copyText(invokeSnippet(t)); push("Copied invocation for " + t.name); }}>
                {Ico.bolt}{plan.status === "blocked" ? "Cannot call" : "Copy invocation"}
              </button>
              <button className="cact" onClick={copyRecord}>
                {Ico.copy}Copy record
              </button>
              {t.endpoint && (
                <a className="cact" href={t.endpoint} target="_blank" rel="noreferrer" onClick={(e) => e.stopPropagation()}>
                  {Ico.ext}endpoint
                </a>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/* ---- AGENT lens list: flat machine table, one row per registry record ---- */
function AgentRow({ t, ctx, open, onToggle, rowNo }) {
  const push = useToast();
  const plan = planCall(t, ctx);
  const flags = [
    t.has_x402 ? "x402" : "free",
    t.has_auth ? "siwe" : null,
    t.access === "gated" ? "gated" : null,
    t.manifest_status === "verified" ? "hash:ok" : "hash:mismatch",
    t.status === "deregistered" ? "dereg" : null,
  ].filter(Boolean);
  const p = fmtPrice(t.price_usdc);
  const price = t.has_x402 ? (p != null ? p : "metered") : "0";

  return (
    <div className="arowwrap" data-open={open} data-tool-id={t.uid}>
      <button className="arow" onClick={onToggle} aria-expanded={open}>
        <span className="aid" title={"uid " + t.uid}>{String(rowNo).padStart(3, "0")}</span>
        <span className="aname" data-dereg={t.status === "deregistered"}>{t.name}</span>
        <span className={"averdict " + plan.status}>{plan.status}</span>
        <span className="aprice">{price}</span>
        <span className="aflags">{flags.join("  ")}</span>
      </button>
      {open && <div className="adetail"><AgentLens t={t} ctx={ctx} push={push} /></div>}
    </div>
  );
}

function AgentTable({ tools, ctx, openId, onToggle }) {
  return (
    <div className="agentpane">
      <div className="apane-head">
        <span className="averb">GET</span>
        <span className="aep">/api/tools</span>
        <span className="acount">{tools.length} records</span>
      </div>
      <div className="acols">
        <span>#</span><span>name</span><span>verdict</span><span>usdc</span><span>flags</span>
      </div>
      {tools.length === 0 ? (
        <div className="anone">0 records match the current query and filters</div>
      ) : tools.map((t, i) => (
        <AgentRow key={t.uid} t={t} ctx={ctx} rowNo={i + 1} open={openId === t.uid} onToggle={() => onToggle(t.uid)} />
      ))}
    </div>
  );
}

Object.assign(window, { CapabilityCard, AgentTable });

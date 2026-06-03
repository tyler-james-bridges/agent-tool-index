// cat-card.jsx - CapabilityCard: collapsed glance + expanded dossier (Human / Agent lens)
const { useState: useKS } = React;

function AgentGlance({ t }) {
  const chips = [
    "/api/tools/" + t.id,
    "POST /can_call",
    t.has_x402 ? "x402" : "no payment",
    t.has_auth ? "SIWE" : null,
    t.access === "gated" ? "predicate" : "open",
  ].filter(Boolean);

  return (
    <span className="glance agentglance">
      {chips.map((chip) => <span className="agentk" key={chip}>{chip}</span>)}
    </span>
  );
}

function Glance({ t, lens, onTag }) {
  if (lens === "agent") return <AgentGlance t={t} />;

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

function cardSummary(t, lens, plan) {
  if (lens !== "agent") {
    const v = VERDICT_COPY[plan.status];
    return v.label + " - " + v.line;
  }

  const settlement = t.has_x402 ? "x402 payment" : "no payment";
  const auth = t.has_auth ? "SIWE auth" : "no auth";
  const integrity = t.manifest_status === "verified" ? "hash verified" : "hash mismatch";
  return "resolve record / " + settlement + " / " + auth + " / " + integrity;
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
            {" · ERC-8257 " + (t.erc || "draft")}
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

function CapabilityCard({ t, ctx, lens, open, onToggle, onTag }) {
  const push = useToast();
  const plan = planCall(t, ctx);
  const pl = priceLine(t);
  const ver = t.manifest_status === "verified";
  const gated = t.access === "gated";
  const dereg = t.status === "deregistered";

  function copyRecord(e) {
    e.stopPropagation();
    const rec = lens === "agent" ? canCallRecord(t, ctx) : resolveRecord(t);
    copyText(JSON.stringify(rec, null, 2));
    push("Copied " + (lens === "agent" ? "can_call plan" : "tool record"));
  }

  return (
    <div className="card" data-open={open} data-dereg={dereg} data-lens={lens}>
      <button className="chead" onClick={onToggle} aria-expanded={open}>
        <span className="cverdict">
          <span className={"vdot " + plan.status} />
          <span className={"vl " + plan.status}>{plan.status === "callable" ? "ready" : plan.status === "conditional" ? "conditions" : "blocked"}</span>
        </span>

        <span className="ctitle">
          <span className="nm">
            <span className={dereg ? "strike" : ""}>{t.name}</span>
            <span className="idx">#{String(t.id).padStart(2, "0")}</span>
          </span>
          <span className="sum">{cardSummary(t, lens, plan)}</span>
          <Glance t={t} lens={lens} onTag={onTag} />
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
            {lens === "agent" ? <AgentLens t={t} ctx={ctx} push={push} /> : <HumanLens t={t} ctx={ctx} push={push} />}

            <div className="cfoot">
              <button className="cact primary" disabled={plan.status === "blocked"} onClick={(e) => { e.stopPropagation(); push(plan.status === "conditional" ? "Resolve conditions, then call" : "Invocation ready"); }}>
                {Ico.bolt}{plan.status === "blocked" ? "Cannot call" : plan.status === "conditional" ? "Call with conditions" : "Call now"}
              </button>
              <button className="cact" onClick={copyRecord}>
                {Ico.copy}{lens === "agent" ? "Copy can_call" : "Copy record"}
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

Object.assign(window, { CapabilityCard });

// cat-domains.jsx — domain taxonomy + browse-layer grouping for discoverability
const { useMemo: useDM } = React;

/* Each capability gets ONE primary domain, decided by first matching tag set
   (priority order matters: most identifying domain wins). */
const DOMAINS = [
  {
    key: "security", name: "Security & Trust",
    blurb: "Audits, risk scores, honeypot and forensics checks before you transact.",
    tags: ["security","audit","honeypot","forensics","risk","risk-management","safety","agent-safety","due-diligence","agentcheck","goplus","scoring","wallet-rating","reputation","trust","certification","cert","proxy-detection","pre-trade"],
    icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d="M12 3l7 3v5c0 4.4-3 7.6-7 9-4-1.4-7-4.6-7-9V6z"/><path d="m9 12 2 2 4-4"/></svg>,
  },
  {
    key: "nft", name: "NFT & Collections",
    blurb: "Appraise, sweep, and analyze collections, floors, rarity and holders.",
    tags: ["nft","opensea","floor","floor-price","collection","rarity","traits","trait","sweep","holders","holder-discount","appraisal","chonks","normies","vinyl-figurine","image-generation"],
    icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><rect x="3.5" y="3.5" width="17" height="17" rx="2.5"/><path d="M3.5 16l4.5-4 3.5 3 4-5 5 5.5"/><circle cx="9" cy="9" r="1.4"/></svg>,
  },
  {
    key: "defi", name: "DeFi & Trading",
    blurb: "Token stats, vaults, staking, rebalancing and on-chain market signals.",
    tags: ["defi","token","erc20","trading","traders","staking","erc4626","rebalance","volume","burn","portfolio","holdings","whale","oracle","gas","fees","market-signal","micropayments","airdrop","timing"],
    icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d="M4 16l5-5 3 3 7-7"/><path d="M16 4h4v4"/><path d="M4 20h16"/></svg>,
  },
  {
    key: "identity", name: "Identity & Agents",
    blurb: "Resolve names and addresses, agent personas, reputation and discovery.",
    tags: ["identity","ens","resolver","address","agent","persona","personality","profile","erc-8004","agent-binding","attribution","community","crypto-twitter","discovery","leaderboard","normies-persona","continuity"],
    icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="8" r="3.2"/><path d="M5.5 20a6.5 6.5 0 0 1 13 0"/><path d="M12 11.2V14"/></svg>,
  },
  {
    key: "data", name: "Data & Tooling",
    blurb: "Decode transactions, inspect contracts, and other on-chain utilities.",
    tags: [], // catch-all
    icon: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"><path d="M8 6 3 12l5 6M16 6l5 6-5 6"/><path d="M13.5 4l-3 16"/></svg>,
  },
];
const DOMAIN_BY_KEY = Object.fromEntries(DOMAINS.map((d) => [d.key, d]));

function domainOf(t) {
  const tset = new Set(t.tags || []);
  for (const d of DOMAINS) {
    if (d.tags.length && d.tags.some((tg) => tset.has(tg))) return d.key;
  }
  return "data";
}

// counts per domain across the full registry (active + dereg)
function domainCounts(tools) {
  const c = {};
  DOMAINS.forEach((d) => (c[d.key] = 0));
  tools.forEach((t) => { c[domainOf(t)]++; });
  return c;
}

Object.assign(window, { DOMAINS, DOMAIN_BY_KEY, domainOf, domainCounts });

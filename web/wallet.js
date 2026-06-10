// wallet.js — vanilla browser wallet + x402 client for Agent Tool Index.
// No build step, no external libs: talks to the injected EIP-1193 provider
// (window.ethereum) and runs the full x402 "exact" flow (EIP-3009
// transferWithAuthorization signed via eth_signTypedData_v4), routing every
// tool call through the same-origin /api/call proxy to avoid CORS.
(function () {
  "use strict";

  // x402 network name -> EVM chain id.
  const NETWORKS = {
    base: { id: 8453, hex: "0x2105", name: "Base" },
    "base-sepolia": { id: 84532, hex: "0x14a34", name: "Base Sepolia" },
    ethereum: { id: 1, hex: "0x1", name: "Ethereum" },
    mainnet: { id: 1, hex: "0x1", name: "Ethereum" },
    "avalanche": { id: 43114, hex: "0xa86a", name: "Avalanche" },
    "polygon": { id: 137, hex: "0x89", name: "Polygon" },
    "arbitrum": { id: 42161, hex: "0xa4b1", name: "Arbitrum" },
    "optimism": { id: 10, hex: "0xa", name: "Optimism" },
  };
  const CHAIN_ADD = {
    "0x2105": { chainName: "Base", nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 }, rpcUrls: ["https://mainnet.base.org"], blockExplorerUrls: ["https://basescan.org"] },
    "0x14a34": { chainName: "Base Sepolia", nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 }, rpcUrls: ["https://sepolia.base.org"], blockExplorerUrls: ["https://sepolia.basescan.org"] },
  };

  const state = { provider: null, address: null, chainId: null, connecting: false };
  const listeners = new Set();

  // EIP-6963: modern wallets announce themselves via events and may not set
  // window.ethereum at all (especially with multiple wallets installed).
  const discovered = new Map(); // rdns -> { info, provider }

  // Remember which wallet the user connected so a page refresh can silently
  // re-attach to the SAME wallet (eth_accounts, no prompt) instead of losing it.
  const SAVED_KEY = "ati-wallet";
  function saveWallet(id) { try { if (id) localStorage.setItem(SAVED_KEY, id); } catch (e) {} }
  function clearWallet() { try { localStorage.removeItem(SAVED_KEY); } catch (e) {} }
  function savedWallet() { try { return localStorage.getItem(SAVED_KEY); } catch (e) { return null; } }

  function setupDiscovery() {
    if (window.__atiDiscovery) return;
    window.__atiDiscovery = true;
    window.addEventListener("eip6963:announceProvider", (e) => {
      const detail = e && e.detail;
      if (!detail || !detail.provider) return;
      const key = (detail.info && (detail.info.rdns || detail.info.uuid || detail.info.name)) || String(discovered.size);
      const had = discovered.has(key);
      discovered.set(key, { info: detail.info, provider: detail.provider });
      // A wallet appeared. Refresh the button, and if it's the one the user
      // connected before a refresh, silently re-attach (6963 announces async,
      // so the saved wallet often shows up after the initial reconnect try).
      if (!had && !state.address) { emit(); if (savedWallet()) reconnect(); }
    });
    try { window.dispatchEvent(new Event("eip6963:requestProvider")); } catch (e) {}
  }

  function discoveredProviders() { return Array.from(discovered.values()); }

  function provider() {
    if (state.provider) return state.provider;
    let p = window.ethereum;
    // If multiple wallets injected on window.ethereum, prefer a known one.
    if (p && Array.isArray(p.providers) && p.providers.length) {
      p = p.providers.find((x) => x.isMetaMask) || p.providers.find((x) => x.isCoinbaseWallet) || p.providers[0];
    }
    // Fall back to an EIP-6963-announced provider.
    if (!p && discovered.size) {
      const list = discoveredProviders();
      const mm = list.find((x) => x.info && /metamask/i.test(x.info.rdns || x.info.name || ""));
      p = (mm || list[0]).provider;
    }
    state.provider = p || null;
    return state.provider;
  }
  function hasProvider() { return listWallets().length > 0; }

  function injectedName(prov) {
    return prov.isMetaMask ? "MetaMask" : prov.isCoinbaseWallet ? "Coinbase Wallet" : prov.isRabby ? "Rabby"
      : prov.isBraveWallet ? "Brave Wallet" : prov.isTrust ? "Trust Wallet" : prov.isPhantom ? "Phantom" : "Browser Wallet";
  }

  // Connectable wallets. EIP-6963 is the standard discovery mechanism (the same
  // one wagmi/RainbowKit use) and supersedes the legacy window.ethereum object —
  // so we only fall back to window.ethereum when no wallet announces via 6963.
  // This prevents the same wallet (e.g. MetaMask) from appearing twice.
  function listWallets() {
    let out = [];
    discoveredProviders().forEach(({ info, provider: prov }, i) => {
      const key = (info && (info.rdns || info.uuid || info.name)) || ("w" + i);
      out.push({ key: key, name: (info && info.name) || injectedName(prov), icon: (info && info.icon) || null, provider: prov, rdns: info && info.rdns });
    });
    if (!out.length && window.ethereum) {
      const e = window.ethereum;
      const injected = Array.isArray(e.providers) && e.providers.length ? e.providers : [e];
      injected.forEach((prov, i) => {
        out.push({ key: "injected-" + i, name: injectedName(prov), icon: null, provider: prov });
      });
    }
    // De-dupe defensively by provider reference, then by name.
    const seenP = new Set(), seenN = new Set();
    out = out.filter((w) => {
      if (seenP.has(w.provider)) return false; seenP.add(w.provider);
      const n = (w.rdns || w.name || "").toLowerCase();
      if (n && seenN.has(n)) return false; if (n) seenN.add(n);
      return true;
    });
    return out;
  }
  function walletCount() { return listWallets().length; }

  function emit() {
    const snap = getState();
    listeners.forEach((fn) => { try { fn(snap); } catch (e) {} });
    try { window.dispatchEvent(new CustomEvent("ati:wallet", { detail: snap })); } catch (e) {}
  }
  function subscribe(fn) { listeners.add(fn); return () => listeners.delete(fn); }
  function getState() {
    return {
      hasProvider: hasProvider(),
      address: state.address,
      chainId: state.chainId,
      chainIdNum: state.chainId ? parseInt(state.chainId, 16) : null,
      connecting: state.connecting,
    };
  }

  function wireEvents(p) {
    if (!p || p.__atiWired) return;
    p.__atiWired = true;
    if (p.on) {
      p.on("accountsChanged", (accs) => {
        state.address = (accs && accs[0]) || null;
        if (!state.address) clearWallet();   // user disconnected — don't auto-reattach
        emit();
      });
      p.on("chainChanged", (cid) => { state.chainId = cid; emit(); });
    }
  }

  async function connect(key) {
    const list = listWallets();
    let chosen = key ? list.find((w) => w.key === key) : null;
    const p = (chosen && chosen.provider) || provider();
    if (!p) { emit(); throw new Error("No wallet detected. Install MetaMask, Coinbase Wallet, Rabby, or another browser wallet."); }
    state.provider = p; // lock the user's selection for the session
    state.connecting = true; emit();
    try {
      let accs;
      try {
        accs = await p.request({ method: "eth_requestAccounts" });
      } catch (e) {
        // Some wallets throw (e.g. "wallet must have at least one account") even
        // when accounts exist — fall back to the already-authorized list.
        try { accs = await p.request({ method: "eth_accounts" }); } catch (e2) {}
        if (!accs || !accs.length) {
          if (e && e.code === 4001) throw new Error("Connection request was rejected.");
          throw new Error((e && e.message) || "Wallet did not return an account. Unlock it (and create/select an account), then retry.");
        }
      }
      state.address = (accs && accs[0]) || null;
      if (!state.address) throw new Error("No account available. Unlock your wallet and create or select an account, then retry.");
      state.chainId = await p.request({ method: "eth_chainId" });
      wireEvents(p);
      // Persist the exact wallet picked so a refresh re-attaches to it.
      const used = chosen || listWallets().find((wal) => wal.provider === p);
      saveWallet((used && (used.rdns || used.key)) || "injected");
      return getState();
    } finally {
      state.connecting = false; emit();
    }
  }

  // Silently restore the previously-connected wallet on load — no prompt. Picks
  // the SAME wallet the user chose (matched by rdns/key) once it has announced.
  async function reconnect() {
    const saved = savedWallet();
    if (!saved || state.address) return getState();
    const w = listWallets().find((x) => x.rdns === saved || x.key === saved);
    const p = (w && w.provider) || (saved === "injected" ? provider() : null);
    if (!p) return getState(); // its wallet hasn't announced yet —6963 handler retries
    try {
      const accs = await p.request({ method: "eth_accounts" }); // authorized accounts, no popup
      if (accs && accs[0]) {
        state.provider = p;
        state.address = accs[0];
        state.chainId = await p.request({ method: "eth_chainId" });
        wireEvents(p);
        emit();
      }
    } catch (e) {}
    return getState();
  }

  async function refresh() {
    const p = provider();
    if (!p) return getState();
    try {
      const accs = await p.request({ method: "eth_accounts" });
      state.address = (accs && accs[0]) || null;
      if (state.address) { state.chainId = await p.request({ method: "eth_chainId" }); wireEvents(p); }
    } catch (e) {}
    emit();
    return getState();
  }

  async function ensureChain(hex) {
    const p = provider();
    if (!p) throw new Error("No wallet");
    if (state.chainId && state.chainId.toLowerCase() === hex.toLowerCase()) return;
    try {
      await p.request({ method: "wallet_switchEthereumChain", params: [{ chainId: hex }] });
    } catch (err) {
      if (err && (err.code === 4902 || (err.data && err.data.originalError && err.data.originalError.code === 4902)) && CHAIN_ADD[hex]) {
        await p.request({ method: "wallet_addEthereumChain", params: [Object.assign({ chainId: hex }, CHAIN_ADD[hex])] });
      } else {
        throw err;
      }
    }
    state.chainId = await p.request({ method: "eth_chainId" }); emit();
  }

  // ---- low-level helpers ----
  function randomNonce() {
    const b = new Uint8Array(32);
    (window.crypto || window.msCrypto).getRandomValues(b);
    return "0x" + Array.from(b).map((x) => x.toString(16).padStart(2, "0")).join("");
  }
  function b64(str) {
    // JSON here is pure ASCII (hex + digits), so btoa is safe.
    return btoa(unescape(encodeURIComponent(str)));
  }
  function fmtUsdcAtomic(v) {
    try { return (Number(v) / 1e6).toString(); } catch (e) { return String(v); }
  }

  async function postCall(url, body, xPayment) {
    const res = await fetch("/api/call", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ url, body, xPayment: xPayment || undefined }),
    });
    // Success path: the proxy wraps the upstream call in { ok, status, headers, body }.
    let env = null;
    try { env = await res.json(); } catch (e) { /* non-JSON proxy response */ }
    if (env && typeof env.status === "number") return env;
    // Proxy-level error (host not allowlisted, bad url, upstream fetch failed, …)
    // returns { error } with no numeric status — normalize it into an envelope so
    // callers surface the real reason instead of "Tool returned undefined".
    const msg = (env && (env.error || env.message)) || ("Execution proxy error (HTTP " + res.status + ")");
    return { ok: false, status: res.status, headers: {}, body: env, error: msg };
  }

  // Build + sign the x402 "exact" EVM payment (EIP-3009 transferWithAuthorization).
  async function buildPayment(accept) {
    const p = provider();
    if (!p || !state.address) throw new Error("Connect a wallet first.");
    const net = NETWORKS[accept.network];
    if (!net) throw new Error("Unsupported x402 network: " + accept.network);
    await ensureChain(net.hex);

    const now = Math.floor(Date.now() / 1000);
    const authorization = {
      from: state.address,
      to: accept.payTo,
      value: String(accept.maxAmountRequired),
      validAfter: "0",
      validBefore: String(now + (accept.maxTimeoutSeconds || 60)),
      nonce: randomNonce(),
    };
    const typedData = {
      types: {
        EIP712Domain: [
          { name: "name", type: "string" },
          { name: "version", type: "string" },
          { name: "chainId", type: "uint256" },
          { name: "verifyingContract", type: "address" },
        ],
        TransferWithAuthorization: [
          { name: "from", type: "address" },
          { name: "to", type: "address" },
          { name: "value", type: "uint256" },
          { name: "validAfter", type: "uint256" },
          { name: "validBefore", type: "uint256" },
          { name: "nonce", type: "bytes32" },
        ],
      },
      domain: {
        name: (accept.extra && accept.extra.name) || "USD Coin",
        version: (accept.extra && accept.extra.version) || "2",
        chainId: net.id,
        verifyingContract: accept.asset,
      },
      primaryType: "TransferWithAuthorization",
      message: authorization,
    };
    const signature = await p.request({
      method: "eth_signTypedData_v4",
      params: [state.address, JSON.stringify(typedData)],
    });
    const payment = {
      x402Version: 1,
      scheme: accept.scheme || "exact",
      network: accept.network,
      payload: { signature, authorization },
    };
    return b64(JSON.stringify(payment));
  }

  function decodePaymentResponse(headerVal) {
    if (!headerVal) return null;
    try { return JSON.parse(decodeURIComponent(escape(atob(headerVal)))); } catch (e) {
      try { return JSON.parse(atob(headerVal)); } catch (e2) { return headerVal; }
    }
  }

  // High-level: run a tool. Returns { ok, status, data, paid, payment, accept, error }.
  async function runTool(tool, inputs) {
    const url = tool.endpoint;
    if (!url) return { ok: false, status: 0, error: "Tool has no endpoint." };

    let env;
    try { env = await postCall(url, inputs); }
    catch (e) { return { ok: false, status: 0, error: "Network error reaching proxy: " + (e && e.message || e) }; }

    if (env.status === 402 && env.body && Array.isArray(env.body.accepts)) {
      const accept = env.body.accepts[0];
      if (!state.address) {
        return { ok: false, status: 402, needWallet: true, accept, price_usdc: fmtUsdcAtomic(accept.maxAmountRequired),
          error: "Payment required (x402). Connect a wallet to pay " + fmtUsdcAtomic(accept.maxAmountRequired) + " USDC and run." };
      }
      let xPayment;
      try { xPayment = await buildPayment(accept); }
      catch (e) { return { ok: false, status: 402, accept, error: "Payment signing failed: " + (e && e.message || e) }; }

      let env2;
      try { env2 = await postCall(url, inputs, xPayment); }
      catch (e) { return { ok: false, status: 0, error: "Network error settling payment: " + (e && e.message || e) }; }
      return {
        ok: env2.status >= 200 && env2.status < 300,
        status: env2.status,
        data: env2.body,
        paid: env2.status >= 200 && env2.status < 300,
        accept,
        price_usdc: fmtUsdcAtomic(accept.maxAmountRequired),
        payment: decodePaymentResponse(env2.headers && env2.headers["x-payment-response"]),
        error: env2.status >= 200 && env2.status < 300 ? null
          : (env2.body && env2.body.error) ? env2.body.error
          : env2.error ? env2.error
          : "Settlement returned " + env2.status,
      };
    }

    return {
      ok: env.status >= 200 && env.status < 300,
      status: env.status,
      data: env.body,
      error: env.status >= 200 && env.status < 300 ? null
        : (env.body && env.body.error) ? env.body.error
        : env.error ? env.error
        : "Tool returned " + env.status,
    };
  }

  function isMobile() {
    return /iPhone|iPad|iPod|Android/i.test(navigator.userAgent);
  }

  function mobileWalletLinks() {
    const url = encodeURIComponent(window.location.href);
    return [
      { name: "MetaMask", url: "https://metamask.app.link/dapp/" + window.location.host + window.location.pathname },
      { name: "Coinbase Wallet", url: "https://go.cb-w.com/dapp?cb_url=" + url },
      { name: "Trust Wallet", url: "https://link.trustwallet.com/open_url?coin_id=60&url=" + url },
    ];
  }

  window.ATI = {
    connect, reconnect, refresh, ensureChain, subscribe, getState, runTool,
    hasProvider, walletCount, listWallets, fmtUsdcAtomic, NETWORKS,
    isMobile, mobileWalletLinks,
  };

  // Discover wallets (EIP-6963) immediately. If the user connected before, the
  // saved wallet is re-attached silently (reconnect); the 6963 announce handler
  // retries once each wallet appears. With no saved wallet, fall back to the
  // legacy eager check.
  setupDiscovery();
  function init() { if (savedWallet()) reconnect(); else refresh(); }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();

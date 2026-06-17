// Unit tests for the x402 gate on the flat /api/verify endpoint.
// Run with: node --test api/verify.x402.test.js  (no network: covers helpers +
// the pre-facilitator challenge paths that short-circuit before any fetch).
const test = require("node:test");
const assert = require("node:assert/strict");

const handler = require("./verify.js");

test("paymentRequirements advertises USDC-on-Base exact terms", () => {
  const r = handler.paymentRequirements("agenttoolindex.xyz");
  assert.equal(r.scheme, "exact");
  assert.equal(r.network, "base");
  assert.equal(r.asset.toLowerCase(), "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913");
  assert.equal(r.maxAmountRequired, "1000");
  assert.equal(r.resource, "https://agenttoolindex.xyz/api/verify");
  assert.deepEqual(r.extra, { name: "USD Coin", version: "2" });
});

test("encodeReceipt/decodePayment round-trip base64 JSON", () => {
  const obj = { x402Version: 1, scheme: "exact", payload: { signature: "0xabc" } };
  assert.deepEqual(handler.decodePayment(handler.encodeReceipt(obj)), obj);
});

test("decodePayment returns null on garbage", () => {
  assert.equal(handler.decodePayment("not-base64-json!!"), null);
  assert.equal(handler.decodePayment(""), null);
});

// Minimal mock res that records what send() writes.
function mockRes() {
  return {
    statusCode: 200, headers: {}, body: null,
    setHeader(k, v) { this.headers[k.toLowerCase()] = v; },
    end(b) { this.body = b; },
  };
}

test("no X-PAYMENT header yields a 402 challenge with accepts", async () => {
  const req = { method: "POST", headers: { host: "agenttoolindex.xyz" }, query: { chain_id: 8453, tool_id: 136 } };
  const res = mockRes();
  await handler(req, res);
  assert.equal(res.statusCode, 402);
  const body = JSON.parse(res.body);
  assert.equal(body.x402Version, 1);
  assert.equal(body.accepts.length, 1);
  assert.equal(body.accepts[0].payTo, "0xa102a2cb8aac6c7d2c477412ebb7d41d0ce53495");
  assert.match(body.error, /X-PAYMENT/);
});

test("malformed X-PAYMENT header yields a 402, not a crash", async () => {
  const req = { method: "POST", headers: { host: "agenttoolindex.xyz", "x-payment": "@@@" }, query: { chain_id: 8453, tool_id: 136 } };
  const res = mockRes();
  await handler(req, res);
  assert.equal(res.statusCode, 402);
  assert.match(JSON.parse(res.body).error, /malformed/);
});

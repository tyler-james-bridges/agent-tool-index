# Agent Tool Index

Agent-first visual index for ERC-8257 tools on Base.

This is a live demo scaffold built in Rust with Axum and Alloy. It syncs the Base ERC-8257 registry, caches tool records locally, verifies manifests with JCS + Keccak, and exposes both a visual dashboard and agent-readable endpoints.

## Run

```bash
cargo run -- sync
cargo run -- serve
```

Then open `http://127.0.0.1:8787`.

`cargo run -- backfill-events` is available if you only want to refresh stored registry logs without running a full tool sync.

## Endpoints

- `GET /` visual explorer
- `GET /tools/{tool_id}` visual tool detail page
- `POST /api/sync` run a live sync
- `GET /api/tools` agent-readable tool list
- `GET /api/tools/{tool_id}` single tool record
- `GET /api/resolve` resolver usage helper
- `POST /api/resolve` resolve intent/filter criteria to candidate tools
- `POST /api/tools/{tool_id}/can_call` plan whether a caller can invoke a tool
- `GET /api/stats` index stats
- `GET /llms.txt` agent context
- `GET /openapi.json` schema-rich OpenAPI surface

## Config

- `BASE_RPC_URL`, default `https://mainnet.base.org`
- `ERC8257_CACHE`, default `data/tools.json`
- `ERC8257_DB`, default `data/index.sqlite`

## Current Scope

- Base registry only
- `toolCount()` plus `getToolConfig(id)` sync
- Handles `ToolIsDeregistered(uint256)` reverts
- Manifest fetch + hash verification
- SQLite persistence for snapshots, current tool records, and registry events
- Blockscout log backfill for ToolRegistry event history
- Event history enriches deregistered tools with prior metadata where available
- Agent callability planner for active, x402, auth, and predicate-gated tools

## Agent Resolve

Example:

```bash
curl -X POST http://127.0.0.1:8787/api/resolve \
  -H 'Content-Type: application/json' \
  -d '{"query":"wallet risk", "status":"active", "x402":true, "limit":5}'
```

Supported fields: `query`, `status`, `access`, `manifest_status`, `x402`, `limit`.

## Agent Call Planning

Example:

```bash
curl -X POST http://127.0.0.1:8787/api/tools/28/can_call \
  -H 'Content-Type: application/json' \
  -d '{"allow_x402":true, "budget_usdc":1}'
```

Supported fields: `wallet`, `budget_usdc`, `allow_x402`, `has_auth`.

The planner is conservative. It does not claim predicate access unless the caller provides enough context. It returns `callable`, `conditional`, or `not_callable` plus requirements, blockers, and invocation steps.

## Event-Aware Backfill Plan

The demo now stores Blockscout event history, but the production index should replace the current `toolCount()` loop with event state reconstruction:

1. Store registry config per chain: `chain_id`, `registry`, `from_block`, `cursor_block`.
2. Backfill logs in bounded chunks for `ToolRegistered`, `ToolDeregistered`, `ToolMetadataUpdated`, and `AccessPredicateUpdated`.
3. Persist raw events with `block_number`, `tx_hash`, `log_index`, event type, and decoded payload.
4. Materialize current state keyed by `chain_id + registry + tool_id`.
5. Preserve historical versions so deregistered tools keep their original metadata.
6. Run manifest fetch/hash verification after each registration or metadata update.
7. Advance the cursor only after all events in a chunk are committed.
8. Reorg-safe mode: keep a finality buffer, re-check recent blocks, and replace conflicting logs.

The `toolCount()` loop can remain as a reconciliation check: count active, deregistered, and missing IDs, then alert if event-derived state disagrees with direct reads.

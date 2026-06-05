# Agent Tool Index

Agent-first visual index for [ERC-8257](https://github.com/ethereum/ERCs/pull/1723) tools on Base.

**Live:** [agent-tool-index.vercel.app](https://agent-tool-index.vercel.app)

Syncs the onchain [ToolRegistry](https://github.com/ProjectOpenSea/tool-registry) (`0x265BB2...baD2cf1` on Base), fetches and verifies each tool manifest (JCS + keccak256), and exposes both a visual explorer and agent-readable API.

## Features

- Human and Agent lens views with light/dark themes
- Manifest hash verification against onchain records
- x402 payment detection and pricing extraction
- Access predicate awareness (open vs gated)
- Intent-based tool resolution (`/api/resolve`)
- Agent call planner (`/api/tools/{id}/can_call`)
- SQLite persistence with Blockscout event backfill
- `llms.txt` and OpenAPI for agent consumption

## Quick Start

```bash
# Clone and build
git clone https://github.com/tyler-james-bridges/agent-tool-index.git
cd agent-tool-index
cargo build --release

# Sync the registry (fetches all tools from Base)
cargo run --release -- sync

# Start the server
cargo run --release -- serve
# Open http://127.0.0.1:8787
```

## Static Deploy

The Vercel deployment serves a pre-synced snapshot via `web/registry-data.js`. The visual explorer works fully from this snapshot. API routes (`/api/*`) require the Rust server.

```bash
# Refresh the static snapshot
cargo run --release -- sync
# Then deploy web/ to any static host
```

## API

All API routes require the Rust server (`cargo run -- serve`).

| Endpoint | Method | Description |
|---|---|---|
| `/api/tools` | GET | List all indexed tools |
| `/api/tools/{id}` | GET | Single tool record |
| `/api/tools/{id}/can_call` | POST | Plan whether a caller can invoke a tool |
| `/api/resolve` | POST | Resolve intent/filter criteria to candidate tools |
| `/api/stats` | GET | Index statistics |
| `/api/sync` | POST | Trigger a live registry sync |
| `/llms.txt` | GET | Agent context file |
| `/openapi.json` | GET | OpenAPI 3.1 schema |

### Resolve

```bash
curl -X POST http://127.0.0.1:8787/api/resolve \
  -H 'Content-Type: application/json' \
  -d '{"query":"wallet risk", "status":"active", "x402":true, "limit":5}'
```

Supported fields: `query`, `status`, `access`, `manifest_status`, `x402`, `limit`.

### Call Planning

```bash
curl -X POST http://127.0.0.1:8787/api/tools/28/can_call \
  -H 'Content-Type: application/json' \
  -d '{"allow_x402":true, "budget_usdc":1}'
```

Returns `callable`, `conditional`, or `not_callable` with requirements, blockers, and invocation steps.

## Config

| Variable | Default |
|---|---|
| `BASE_RPC_URL` | `https://mainnet.base.org` |
| `ERC8257_CACHE` | `data/tools.json` |
| `ERC8257_DB` | `data/index.sqlite` |

## Stack

- Rust (Axum + Alloy + rusqlite)
- React (CDN, no build step) with JSX transpiled via Babel standalone
- Base chain via public RPC
- Blockscout API for event history

## Related

- [ERC-8257 Tool Registry (contracts)](https://github.com/ProjectOpenSea/tool-registry)
- [Tool SDK (TypeScript CLI)](https://github.com/ProjectOpenSea/tool-sdk)
- [OpenSea Agent Skills](https://github.com/ProjectOpenSea/opensea-skill)

## License

MIT

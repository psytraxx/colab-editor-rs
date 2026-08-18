# Cloudflare Worker WebSocket Server

This is a Cloudflare Workers implementation of the collaborative editor server using Durable Objects.

## Setup

1. Install dependencies:
```bash
cd worker
npm install
```

2. Run locally:
```bash
npm run dev
```

3. Deploy to Cloudflare:
```bash
npm run deploy
```

## Architecture

- **Worker** (`src/index.ts`): HTTP handler that routes WebSocket connections to Durable Objects
- **Durable Object** (`EditorRoom` class): Manages WebSocket connections, CRDT state, and broadcasts changes
- **Automerge**: CRDT library for conflict-free collaborative editing

## WebSocket Protocol

JSON messages, matching the `WsMessage` enum in `client/src/protocol.rs`:
- `Init`: Server → client, sent once on connect. Carries the assigned `user_id`, the current document `snapshot` (Automerge bytes as a JSON array, or `null`), and the list of currently online `users`.
- `Content`: Either direction. Carries a full Automerge document snapshot (bytes as a JSON array). This is **state-based sync** — the entire document is sent on every change, not an incremental Automerge sync message.
- `UserState`: Either direction. Presence info (`user_id`, `online`, `editing`).

## Known limitations

- **State-based sync.** Clients send a full Automerge snapshot (document *and*
  history) on every keystroke, and the relay overwrites its stored copy with
  whatever arrives. Two consequences: message size grows without bound as a
  document is edited, and a client sending a stale snapshot can regress server
  state for late joiners. Moving to `save_incremental()` or Automerge's sync
  protocol would fix both, but requires the relay to accumulate chunks or run
  Automerge itself to merge rather than replace.
- **No authentication.** Any client that can reach `/ws` can read and write the
  document. Payloads are size- and shape-checked, but not authorized.
- **Single room.** All connections share one `default-room` Durable Object.

## Configuration

Edit `wrangler.toml` to configure:
- Worker name
- Durable Object bindings
- Compatibility settings

Run `npm run typecheck` before deploying — `wrangler deploy` bundles with esbuild,
which does **not** typecheck.

## Deployment

There is no GitHub Actions workflow for this worker; the repo's only workflow,
`.github/workflows/deploy-to-pages.yml`, deploys the `client/` app to GitHub Pages.

Production is reachable at `wss://colab-editor-rs.dynamicflash.workers.dev/ws`
(hardcoded in `client/src/main.rs`). Deploy with `npm run deploy` (`wrangler deploy`).

## Notes

- Uses a single "default-room" for all connections (can be extended to support multiple rooms)
- State is persisted in Durable Object storage (automatic)
- Scales automatically with Cloudflare's edge network

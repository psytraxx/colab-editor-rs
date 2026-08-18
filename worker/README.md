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

JSON messages, matching the `WsMessage` enum in `client/src/main.rs`:
- `Init`: Server → client, sent once on connect. Carries the assigned `user_id`, the current document `snapshot` (Automerge bytes as a JSON array, or `null`), and the list of currently online `users`.
- `Content`: Either direction. Carries a full Automerge document snapshot (bytes as a JSON array). This is **state-based sync** — the entire document is sent on every change, not an incremental Automerge sync message.
- `UserState`: Either direction. Presence info (`user_id`, `user_name`, `online`, `editing`).

Note: there is no `Welcome` or `Sync` message type — this doc previously described a different protocol shape than what's implemented.

## Configuration

Edit `wrangler.toml` to configure:
- Worker name
- Durable Object bindings
- Compatibility settings

## Deployment

There is no GitHub Actions workflow for this worker (the repo's only workflow, `.github/workflows/deploy-to-pages.yml`, deploys the `client/` app to GitHub Pages and does not touch `worker/`).

Production is reachable at `wss://colab-editor-rs.dynamicflash.workers.dev/ws` (hardcoded in `client/src/main.rs`), but how it gets deployed there is **not currently documented or verifiable from the repo**. The two likely mechanisms:

1. **Cloudflare Workers Builds (Git integration)** — the Cloudflare dashboard connected directly to this repo, auto-deploying `worker/` on push to `main`. This is dashboard-side configuration and leaves no trace in git.
2. **Manual deploy** — someone runs `npm run deploy` (`wrangler deploy`) locally/ad hoc.

Confirm which applies by checking the Cloudflare dashboard (Workers & Pages → colab-editor-rs → Settings → Builds) for a connected Git repository.

## Notes

- Uses a single "default-room" for all connections (can be extended to support multiple rooms)
- State is persisted in Durable Object storage (automatic)
- Scales automatically with Cloudflare's edge network

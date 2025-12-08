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

Same as the Rust server - uses JSON messages with types:
- `Welcome`: Server sends user ID to client
- `Sync`: Automerge sync messages (binary data as array)
- `UserState`: User presence information

## Configuration

Edit `wrangler.toml` to configure:
- Worker name
- Durable Object bindings
- Compatibility settings

## Notes

- Uses a single "default-room" for all connections (can be extended to support multiple rooms)
- State is persisted in Durable Object storage (automatic)
- Scales automatically with Cloudflare's edge network

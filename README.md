# Collaborative Editor (Rust + WASM)

A **collaborative text editor** built with Rust, WebAssembly, and Automerge CRDT, relayed through a lightweight Cloudflare Worker. https://psytraxx.github.io/colab-editor-rs/

## Features

- **Conflict-Free Sync**: Uses Automerge CRDT for automatic conflict resolution
- **Real-time Collaboration**: See other users' edits and presence in real-time
- **Rich Text Editing**: Powered by Quill for WYSIWYG editing
- **User Presence**: Visual indicators showing who's online and who's currently editing
- **Serverless Hosting**: The relay runs on a Cloudflare Worker + Durable Object — no traditional backend to operate

## Architecture

This is a **client + relay** design, not peer-to-peer:

- **Client** (`client/`): Rust/WASM app built with the Yew framework. Holds an Automerge document (`AutoCommit`) locally, renders the UI, and talks to the relay over a single WebSocket connection.
- **Relay** (`worker/`): A Cloudflare Worker using a Durable Object (`EditorRoom`) as a single shared room. It tracks connected WebSocket sessions, persists the latest Automerge document snapshot in Durable Object storage, and broadcasts messages to every other connected client (hub-and-spoke, not a mesh).
- **Automerge**: CRDT library ensuring eventual consistency across all clients merging concurrent edits.

### How It Works

1. **Connect**: On load, the client opens a WebSocket to the worker's `/ws` endpoint.
2. **Init**: The Durable Object assigns the client a random user ID and sends an `Init` message containing the current document snapshot (if any) and the list of currently online users.
3. **Edit**: Local edits update the Automerge document. On any change, the client serializes the *entire* document (`doc.save()`) and sends it as a `Content` message — this is state-based (full-snapshot) sync, not incremental.
4. **Relay & Persist**: The worker persists the received snapshot to Durable Object storage and rebroadcasts it to every other connected client.
5. **Merge**: Receiving clients `merge()` the incoming snapshot into their local Automerge document, so concurrent edits resolve automatically via CRDT semantics.
6. **Presence**: Clients also send `UserState` messages (online/editing flags) which the worker rebroadcasts to keep everyone's presence indicators in sync.

## Technology Stack

- **Rust** + **WebAssembly** for client-side logic
- **Yew** framework for reactive UI
- **Automerge** CRDT for distributed state management
- **Quill** for rich text editing
- **Trunk** for WASM build tooling
- **Cloudflare Workers + Durable Objects** for the WebSocket relay (see `worker/README.md`)

## Building & Running

### Prerequisites
- Rust toolchain (with `wasm32-unknown-unknown` target)
- Trunk (`cargo install trunk`)
- Node.js (for the worker, see `worker/README.md`)

### Client — Development
```bash
cd client
trunk serve --port 8081
```

Open your browser to `http://localhost:8081`. By default the client connects to `ws://localhost:8787/ws` when running on `localhost`/`127.0.0.1`, and to the deployed production worker otherwise — see `client/src/main.rs`.

### Client — Production Build
```bash
cd client
trunk build --release
```

Static files will be in `client/dist/`.

### Relay Worker
```bash
cd worker
npm install
npm run dev      # local dev server, defaults to ws://localhost:8787/ws
npm run deploy   # deploy to Cloudflare
```

## Usage

1. **Start the app**: Open it in your browser; it connects to the relay automatically.
2. **View mode**: Read-only rendered view of the document.
3. **Edit mode**: Toggle "Edit mode" to open the Quill editor and start collaborating — all connected clients see changes in near real-time.

## Project Structure

```
colab-editor-rs/
├── client/            # Rust/WASM client (Yew UI + Automerge doc + WebSocket)
│   ├── src/
│   │   ├── main.rs    # App component, WS handling, editor lifecycle
│   │   ├── doc.rs     # Automerge document model and accessors
│   │   ├── marks.rs   # Quill Delta attributes <-> Automerge marks
│   │   ├── protocol.rs # Wire protocol shared with the worker
│   │   └── quill.rs   # Quill JS bindings and Delta types
│   ├── index.html     # Entry HTML
└── worker/            # Cloudflare Worker relay (Durable Object)
    └── src/
        └── index.ts   # HTTP router + EditorRoom Durable Object
```

## Security Considerations

- All document content and presence data pass through the Cloudflare Worker relay, which persists the latest snapshot in Durable Object storage.
- There is currently **no authentication or access control** — anyone with the WebSocket URL can join the single shared room and read/write the document.
- Consider adding auth, per-room isolation, and/or end-to-end encryption before using this for sensitive content.

## Notes

- **Persistence**: The document snapshot is persisted server-side in Durable Object storage; there is no client-side (IndexedDB) persistence yet.
- **Single Room**: The worker currently routes all clients into one shared `default-room` Durable Object. Multi-room support would require adding room IDs to the routing.
- **Sync Model**: Full-document-snapshot sync on every change (not Automerge's incremental sync protocol), which is simple but not bandwidth-efficient for large documents.

## Future Enhancements

- [ ] IndexedDB local/offline persistence
- [ ] Offline editing with sync on reconnect
- [ ] Multi-room support
- [ ] Authentication / access control
- [ ] End-to-end encryption
- [ ] Incremental Automerge sync instead of full-snapshot broadcasts
- [ ] File attachments
- [ ] Export to PDF/Markdown

## License

MIT

## Contributing

Contributions welcome! This is a demonstration of collaborative editing with Rust/WASM, Automerge, and Cloudflare Workers.

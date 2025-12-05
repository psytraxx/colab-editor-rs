git-u# WebRTC Collaborative Editor (Rust + WASM)

A **serverless peer-to-peer collaborative text editor** built with Rust, WebAssembly, Automerge CRDT, and WebRTC.

## 🚀 Features

- **Pure P2P Architecture**: No central server required - peers connect directly via WebRTC
- **Conflict-Free Sync**: Uses Automerge CRDT for automatic conflict resolution
- **Real-time Collaboration**: See other users' edits and presence in real-time
- **Rich Text Editing**: Powered by TinyMCE for WYSIWYG editing
- **User Presence**: Visual indicators showing who's online and what they're editing

## 🏗️ Architecture

### Client-Only Design
This application runs entirely in the browser with no backend server:

- **WebRTC Data Channels**: Direct peer-to-peer communication
- **PeerJS**: Simplified WebRTC with built-in signaling (uses public signaling server)
- **Automerge**: CRDT library ensuring eventual consistency across all peers
- **Full Mesh Network**: Each peer maintains connections to all other peers

### How It Works

1. **Peer Discovery**: When you open the app, PeerJS assigns you a unique ID
2. **Connection**: Share your Peer ID with others, and they can connect to you
3. **Sync**: Once connected, Automerge automatically syncs document state
4. **Collaboration**: All edits are broadcast to connected peers in real-time
5. **Conflict Resolution**: Automerge handles concurrent edits automatically

## 🛠️ Technology Stack

- **Rust** + **WebAssembly** for high-performance client-side logic
- **Yew** framework for reactive UI
- **Automerge** CRDT for distributed state management
- **PeerJS** for WebRTC peer connections
- **TinyMCE** for rich text editing
- **Trunk** for WASM build tooling

## 📦 Building & Running

### Prerequisites
- Rust toolchain (with wasm32-unknown-unknown target)
- Trunk (`cargo install trunk`)

### Development
```bash
cd client
trunk serve --port 8081
```

Open your browser to `http://localhost:8081`

### Production Build
```bash
cd client
trunk build --release
```

Static files will be in `client/dist/`

## 🎮 Usage

1. **Start the app**: Open it in your browser
2. **Get your Peer ID**: Displayed at the top of the page
3. **Connect to peers**: 
   - Share your Peer ID with collaborators
   - Enter their Peer ID in the input field and click "Connect"
4. **Collaborate**: Once connected, start editing together!

### Modes
- **View Mode**: Read-only view with rendered HTML
- **Edit Mode**: Live collaborative editing with TinyMCE

## 🔧 Project Structure

```
colab-editor-rs/
├── client/           # WASM client application
│   ├── src/
│   │   └── main.rs   # Main P2P logic
│   ├── index.html    # Entry HTML
│   └── inline_peer.js # PeerJS wrapper
├── common/           # Shared types/constants
│   └── src/
│       └── lib.rs    # WsMessage, UserState definitions
└── server/           # (DEPRECATED - no longer used)
```

## 🌐 How P2P Networking Works

### Connection Flow
```
Peer A                    PeerJS Signaling Server             Peer B
  |                                  |                           |
  |------- Register --------------->|                           |
  |<------ Assigned ID: "abc123" ---|                           |
  |                                  |<-------- Register ---------|
  |                                  |------- ID: "xyz789" ----->|
  |                                  |                           |
  |------- Connect to "xyz789" ---->|                           |
  |                                  |------- Signal ----------->|
  |<--------------------------------- WebRTC Handshake -------->|
  |                                  |                           |
  |<============ Direct P2P Connection Established ============>|
```

### Data Synchronization
All document changes flow through:
1. **Local Edit** → Update Automerge document
2. **Generate Sync Message** → Automerge creates minimal change set
3. **Broadcast** → Send to all connected peers via WebRTC
4. **Receive & Merge** → Peers apply changes to their Automerge docs
5. **UI Update** → React to document changes

## 🔒 Security Considerations

- Data is transmitted directly between peers (P2P)
- PeerJS signaling server only facilitates initial connection setup
- No central authority stores or sees your data
- Consider adding end-to-end encryption for sensitive content

## 📝 Notes

- **Browser Compatibility**: Requires modern browser with WebRTC support
- **NAT Traversal**: PeerJS handles most NAT/firewall scenarios via STUN/TURN
- **Persistence**: Currently in-memory only - add IndexedDB for local persistence
- **Scalability**: Full mesh works well for small groups (2-10 peers)

## 🚧 Future Enhancements

- [ ] IndexedDB local persistence
- [ ] Offline editing with sync on reconnect
- [ ] End-to-end encryption
- [ ] File attachments
- [ ] Export to PDF/Markdown
- [ ] Custom TURN server for better NAT traversal

## 📄 License

MIT

## 🤝 Contributing

Contributions welcome! This is a demonstration of P2P collaborative editing with Rust/WASM.

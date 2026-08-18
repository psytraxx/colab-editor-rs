export interface Env {
  EDITOR_ROOM: DurableObjectNamespace;
}

// Worker entry point
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    // WebSocket upgrade endpoint
    if (url.pathname === '/ws') {
      // Use a single room for simplicity (you can add room IDs later)
      const roomId = env.EDITOR_ROOM.idFromName('default-room');
      const room = env.EDITOR_ROOM.get(roomId);
      return room.fetch(request);
    }

    // Health check
    if (url.pathname === '/') {
      return new Response('Collaborative Editor WebSocket Server (Relay Mode)', {
        headers: { 'Content-Type': 'text/plain' }
      });
    }

    return new Response('Not Found', { status: 404 });
  }
};

// Durable Object for managing editor state and connections
export class EditorRoom {
  private state: DurableObjectState;
  private snapshot: Uint8Array | null;

  constructor(state: DurableObjectState) {
    this.state = state;
    this.snapshot = null;
  }

  async loadSnapshot(): Promise<void> {
    if (this.snapshot) return;
    const stored = await this.state.storage.get<Uint8Array>('document');
    if (stored) {
      this.snapshot = stored;
      console.log('Loaded snapshot from storage');
    }
  }

  // Presence is derived from the currently connected sockets so it survives
  // Durable Object hibernation (no in-memory session map to lose).
  private userStates(): UserState[] {
    return this.state.getWebSockets().map((ws) => {
      const att = ws.deserializeAttachment() as Attachment;
      return { user_id: att.userId, online: true, editing: att.editing };
    });
  }

  async fetch(request: Request): Promise<Response> {
    // Expect WebSocket upgrade
    const upgradeHeader = request.headers.get('Upgrade');
    if (upgradeHeader !== 'websocket') {
      return new Response('Expected WebSocket', { status: 426 });
    }

    // Load snapshot if available
    await this.loadSnapshot();

    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair);

    // Accept WebSocket connection (hibernation-aware)
    this.state.acceptWebSocket(server);

    // Attach session data to the socket so it can be recovered after hibernation
    const userId = this.generateUserId();
    const attachment: Attachment = { userId, editing: false };
    server.serializeAttachment(attachment);

    // Send Init message (users list must include the new socket)
    this.send(server, {
      Init: {
        user_id: userId,
        snapshot: this.snapshot ? Array.from(this.snapshot) : null,
        users: this.userStates()
      }
    });

    // Broadcast new user to others
    this.broadcast({
      UserState: { user_id: userId, online: true, editing: false }
    }, server);

    return new Response(null, {
      status: 101,
      webSocket: client
    });
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    const attachment = ws.deserializeAttachment() as Attachment | null;
    if (!attachment) return;

    try {
      const data = typeof message === 'string' ? message : new TextDecoder().decode(message);
      const msg = JSON.parse(data) as ClientMessage;

      if (msg.Content !== undefined) {
        // Content = full Automerge snapshot as a JSON byte array. Validate
        // shape and size before persisting: this endpoint is unauthenticated,
        // so a malformed payload must not be able to clobber the document.
        const content = msg.Content;
        if (!Array.isArray(content) || content.length === 0) {
          console.warn('Rejected Content: not a non-empty array');
          return;
        }
        if (content.length > MAX_SNAPSHOT_BYTES) {
          console.warn(`Rejected Content: ${content.length} bytes exceeds cap`);
          return;
        }
        this.snapshot = new Uint8Array(content as number[]);
        await this.state.storage.put('document', this.snapshot);
        this.broadcast({ Content: content as number[] }, ws);
      } else if (msg.UserState !== undefined) {
        this.handleUserState(ws, attachment, msg.UserState as UserState);
      }
    } catch (err) {
      console.error('Error handling message:', err);
    }
  }

  async webSocketClose(ws: WebSocket): Promise<void> {
    this.handleDisconnect(ws);
  }

  // Without this, an abnormally-terminated socket never broadcasts
  // `online: false` and peers keep showing a ghost user indefinitely.
  async webSocketError(ws: WebSocket): Promise<void> {
    this.handleDisconnect(ws);
  }

  private handleDisconnect(ws: WebSocket): void {
    const attachment = ws.deserializeAttachment() as Attachment | null;
    if (!attachment) return;

    console.log(`User ${attachment.userId} disconnected`);
    this.broadcast({
      UserState: { user_id: attachment.userId, online: false, editing: false }
    }, ws);
  }

  private handleUserState(ws: WebSocket, attachment: Attachment, incomingState: UserState): void {
    attachment.editing = incomingState.editing ?? false;
    ws.serializeAttachment(attachment);

    this.broadcast({
      UserState: { user_id: attachment.userId, online: true, editing: attachment.editing }
    }, ws);
  }

  private send(ws: WebSocket, message: ServerMessage): void {
    try {
      ws.send(JSON.stringify(message));
    } catch (err) {
      console.error('Error sending message:', err);
    }
  }

  private broadcast(message: ServerMessage, exclude?: WebSocket): void {
    const msgString = JSON.stringify(message);
    for (const ws of this.state.getWebSockets()) {
      if (ws !== exclude) {
        try {
          ws.send(msgString);
        } catch (err) {
          console.error('Error broadcasting:', err);
        }
      }
    }
  }

  private generateUserId(): string {
    const chars = 'abcdefghijkmnpqrstuvwxyz23456789';
    let result = '';
    for (let i = 0; i < 8; i++) {
      result += chars[Math.floor(Math.random() * chars.length)];
    }
    return result;
  }
}

// Type definitions

/** Largest accepted Automerge snapshot, in bytes. Bounds what an unauthenticated
 *  client can push into Durable Object storage in a single message. */
const MAX_SNAPSHOT_BYTES = 5 * 1024 * 1024;

interface Attachment {
  userId: string;
  editing: boolean;
}

interface UserState {
  user_id: string;
  online: boolean;
  editing: boolean;
}

/** Messages the relay sends to clients. Mirrors the Rust `WsMessage` enum in
 *  `client/src/protocol.rs`; keeping it typed is what stops the two sides
 *  drifting apart silently. */
type ServerMessage =
  | { Init: { user_id: string; snapshot: number[] | null; users: UserState[] } }
  | { Content: number[] }
  | { UserState: UserState };

/** Messages clients send to the relay. */
type ClientMessage =
  | { Content?: unknown; UserState?: unknown };

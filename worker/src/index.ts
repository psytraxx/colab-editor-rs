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
  private sessions: Map<WebSocket, Session>;
  private users: Map<string, UserState>;
  private snapshot: Uint8Array | null;

  constructor(state: DurableObjectState) {
    this.state = state;
    this.sessions = new Map();
    this.users = new Map();
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

    // Accept WebSocket connection
    this.state.acceptWebSocket(server);

    // Generate user ID
    const userId = this.generateUserId();
    const userName = userId;

    // Create session
    const session: Session = {
      userId,
      userName,
      ws: server
    };

    this.sessions.set(server, session);

    // Create initial user state
    const userState: UserState = {
      user_id: userId,
      user_name: userName,
      online: true
    };
    this.users.set(userId, userState);

    // Send Init message
    this.send(server, {
      Init: {
        user_id: userId,
        snapshot: this.snapshot ? Array.from(this.snapshot) : null,
        users: Array.from(this.users.values())
      }
    });

    // Broadcast new user to others
    this.broadcast({
      UserState: userState
    }, server);

    return new Response(null, {
      status: 101,
      webSocket: client
    });
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    const session = this.sessions.get(ws);
    if (!session) return;

    try {
      const data = typeof message === 'string' ? message : new TextDecoder().decode(message);
      const msg = JSON.parse(data);

      if (msg.Content) {
        // Content = Full Snapshot
        // 1. Update local state
        this.snapshot = new Uint8Array(msg.Content);
        // 2. Persist
        await this.state.storage.put('document', this.snapshot);
        // 3. Broadcast to others (exclude sender)
        this.broadcast({ Content: msg.Content }, ws);
      } else if (msg.UserState) {
        this.handleUserState(session, msg.UserState);
      }
    } catch (err) {
      console.error('Error handling message:', err);
    }
  }

  async webSocketClose(ws: WebSocket): Promise<void> {
    const session = this.sessions.get(ws);
    if (!session) return;

    console.log(`User ${session.userId} disconnected`);

    // Mark user as offline
    const userState = this.users.get(session.userId);
    if (userState) {
      userState.online = false;
      this.broadcast({
        UserState: userState
      });
      this.users.delete(session.userId);
    }

    this.sessions.delete(ws);
  }

  private handleUserState(session: Session, incomingState: UserState): void {
    // Update user state
    const userState: UserState = {
      user_id: session.userId,
      user_name: session.userName,
      online: true
    };

    this.users.set(session.userId, userState);

    // Broadcast to all clients (exclude sender)
    this.broadcast({
      UserState: userState
    }, session.ws);
  }

  private send(ws: WebSocket, message: any): void {
    try {
      ws.send(JSON.stringify(message));
    } catch (err) {
      console.error('Error sending message:', err);
    }
  }

  private broadcast(message: any, exclude?: WebSocket): void {
    const msgString = JSON.stringify(message);
    for (const [ws, session] of this.sessions.entries()) {
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
interface Session {
  userId: string;
  userName: string;
  ws: WebSocket;
}

interface UserState {
  user_id: string;
  user_name: string;
  online: boolean;
}

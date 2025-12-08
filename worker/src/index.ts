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

  constructor(state: DurableObjectState) {
    this.state = state;
    this.sessions = new Map();
    this.users = new Map();
  }

  async fetch(request: Request): Promise<Response> {
    // Expect WebSocket upgrade
    const upgradeHeader = request.headers.get('Upgrade');
    if (upgradeHeader !== 'websocket') {
      return new Response('Expected WebSocket', { status: 426 });
    }

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
      editing: false,
      field: null,
      online: true
    };
    this.users.set(userId, userState);

    // Send Welcome message
    this.send(server, {
      Welcome: userId
    });

    // Send all existing users to new client
    for (const [uid, user] of this.users.entries()) {
      this.send(server, {
        UserState: user
      });
    }

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

      if (msg.Sync) {
        // Relay Sync message to all OTHER clients
        // We do NOT process it here, just pass it on
        this.broadcast(msg, ws);
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
    // Update user state with server-assigned info
    const userState: UserState = {
      user_id: session.userId,
      user_name: session.userName,
      editing: incomingState.editing,
      field: incomingState.field,
      online: true
    };

    this.users.set(session.userId, userState);

    // Broadcast to all clients
    this.broadcast({
      UserState: userState
    });
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
  editing: boolean;
  field: string | null;
  online: boolean;
}

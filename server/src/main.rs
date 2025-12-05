use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
    routing::get,
    Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use std::net::SocketAddr;
use std::collections::HashMap;
use tokio::sync::{broadcast, Mutex, RwLock};
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use automerge::{
    transaction::Transactable,
    sync::{self, SyncDoc},
    AutoCommit,
};
use common::{WsMessage, UserState};

struct AppState {
    doc: Mutex<AutoCommit>,
    db: sled::Db,
    tx: broadcast::Sender<BroadcastMsg>,
    users: RwLock<HashMap<String, UserState>>,
}

#[derive(Clone, Debug)]
enum BroadcastMsg {
    DocChanged,
    UserState(UserState),
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let db = sled::open("my_db").unwrap();

    let doc = if let Ok(Some(data)) = db.get("doc_data") {
        println!("Loading existing document...");
        AutoCommit::load(&data).expect("Failed to load doc")
    } else {
        println!("Creating new document...");
        let mut doc = AutoCommit::new();
        doc.put(automerge::ROOT, common::DOC_KEY_TITLE, "Untitled").unwrap();
        doc.put(automerge::ROOT, common::DOC_KEY_BODY, "").unwrap();
        doc.put(automerge::ROOT, common::DOC_KEY_KEYWORDS, "").unwrap();
        doc.put(automerge::ROOT, common::DOC_KEY_VERSION, 0u64).unwrap();
        doc
    };

    let (tx, _rx) = broadcast::channel(100);

    let app_state = Arc::new(AppState {
        doc: Mutex::new(doc),
        db,
        tx,
        users: RwLock::new(HashMap::new()),
    });

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(app_state);

    // Bind to all interfaces so the service is reachable from other hosts on the network
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut sync_state = sync::State::new();
    let mut rx = state.tx.subscribe();

    // Generate unique random 8-char ID for user
    let user_name: String = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let chars: Vec<char> = "abcdefghijkmnpqrstuvwxyz23456789".chars().collect();
        (0..8).map(|i| {
            let idx = ((seed >> (i * 5)) as usize) % chars.len();
            chars[idx]
        }).collect()
    };
    let user_id = user_name.clone();

    println!("[SERVER] New connection: {}", user_id);

    // Send Welcome message
    let welcome_msg = WsMessage::Welcome(user_id.clone());
    let json = serde_json::to_string(&welcome_msg).unwrap();
    if sender.send(Message::Text(json.into())).await.is_err() {
        return;
    }

    // Create initial user state (not editing yet)
    let user_state = UserState {
        user_id: user_id.clone(),
        user_name: user_name.clone(),
        editing: false,
        field: None,
        online: true,
    };
    
    {
        let mut users = state.users.write().await;
        users.insert(user_id.clone(), user_state.clone());
        println!("[SERVER] Total users now: {}", users.len());
    }

    // Send existing users to new client
    {
        let users = state.users.read().await;
        println!("[SERVER] Sending {} existing users to new client {}", users.len(), user_id);
        for (uid, user) in users.iter() {
            println!("[SERVER]   -> Sending user {} to {}", uid, user_id);
            let msg = WsMessage::UserState(user.clone());
            let json = serde_json::to_string(&msg).unwrap();
            if sender.send(Message::Text(json.into())).await.is_err() {
                return;
            }
        }
    }

    // Broadcast new user to others
    println!("[SERVER] Broadcasting new user {} to others", user_id);
    let _ = state.tx.send(BroadcastMsg::UserState(user_state));

    // Initial Sync
    let initial_msg = {
        let mut doc = state.doc.lock().await;
        let x = doc.sync().generate_sync_message(&mut sync_state);
        x
    };

    if let Some(msg) = initial_msg {
        let ws_msg = WsMessage::Sync(msg.encode());
        let json = serde_json::to_string(&ws_msg).unwrap();
        if sender.send(Message::Text(json.into())).await.is_err() {
            return;
        }
    }

    loop {
        tokio::select! {
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text.as_str()) {
                            match ws_msg {
                                WsMessage::Sync(binary) => {
                                    if let Some(resp) = handle_sync_msg(&state, &mut sync_state, binary).await {
                                        let json = serde_json::to_string(&resp).unwrap();
                                        if sender.send(Message::Text(json.into())).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                                WsMessage::UserState(incoming_state) => {
                                    handle_user_state_msg(&state, incoming_state, &user_id, &user_name).await;
                                }
                                WsMessage::Welcome(_) => {}
                            }
                        }
                    }
                    _ => break,
                }
            }
            result = rx.recv() => {
                match result {
                    Ok(BroadcastMsg::DocChanged) => {
                        let reply_msg = {
                            let mut doc = state.doc.lock().await;
                            let x = doc.sync().generate_sync_message(&mut sync_state);
                            x
                        };

                        if let Some(msg) = reply_msg {
                            let resp = WsMessage::Sync(msg.encode());
                            let json = serde_json::to_string(&resp).unwrap();
                            if sender.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Ok(BroadcastMsg::UserState(user_state)) => {
                        // Don't send own state back
                        if user_state.user_id != user_id {
                            println!("[SERVER] Sending UserState to {}: {} (editing={}, field={:?}, online={})", 
                                user_id, user_state.user_name, user_state.editing, user_state.field, user_state.online);
                            let msg = WsMessage::UserState(user_state);
                            let json = serde_json::to_string(&msg).unwrap();
                            if sender.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }

    // User disconnected
    println!("[SERVER] User {} ({}) disconnected", user_id, user_name);
    {
        let mut users = state.users.write().await;
        users.remove(&user_id);
        println!("[SERVER] Total users now: {}", users.len());
    }
    let offline_state = UserState {
        user_id: user_id.clone(),
        user_name,
        editing: false,
        field: None,
        online: false,
    };
    println!("[SERVER] Broadcasting offline state for {}", user_id);
    let _ = state.tx.send(BroadcastMsg::UserState(offline_state));
}

async fn handle_sync_msg(
    state: &Arc<AppState>,
    sync_state: &mut sync::State,
    binary: Vec<u8>,
) -> Option<WsMessage> {
    if let Ok(sync_msg) = sync::Message::decode(&binary) {
        let mut doc = state.doc.lock().await;
        
        let heads_before = doc.get_heads();
        doc.sync().receive_sync_message(sync_state, sync_msg).unwrap();
        let heads_after = doc.get_heads();

        if heads_before != heads_after {
            let saved = doc.save();
            state.db.insert("doc_data", saved).unwrap();
            let _ = state.tx.send(BroadcastMsg::DocChanged);
        }
        
        let x = doc.sync().generate_sync_message(sync_state);
        x.map(|msg| WsMessage::Sync(msg.encode()))
    } else {
        None
    }
}

async fn handle_user_state_msg(
    state: &Arc<AppState>,
    mut incoming_state: UserState,
    user_id: &str,
    user_name: &str,
) {
    println!("[SERVER] Received UserState from {}: editing={}, field={:?}", 
        user_id, incoming_state.editing, incoming_state.field);
    
    // Fill in server-assigned user info
    incoming_state.user_id = user_id.to_string();
    incoming_state.user_name = user_name.to_string();
    incoming_state.online = true;
    
    // Update stored state
    {
        let mut users = state.users.write().await;
        users.insert(user_id.to_string(), incoming_state.clone());
    }
    
    println!("[SERVER] Broadcasting UserState: {} editing={} field={:?}", 
        incoming_state.user_name, incoming_state.editing, incoming_state.field);
    let _ = state.tx.send(BroadcastMsg::UserState(incoming_state));
}

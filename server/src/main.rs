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
use tokio::sync::{broadcast, Mutex};
use std::sync::Arc; // Arc still from std
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use automerge::{
    transaction::Transactable,
    sync::{self, SyncDoc},
    AutoCommit,
};
use common::WsMessage;

struct AppState {
    doc: Mutex<AutoCommit>,
    db: sled::Db,
    tx: broadcast::Sender<()>,
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
        doc.put(automerge::ROOT, common::DOC_KEY_DESCRIPTION, "").unwrap();
        doc.put(automerge::ROOT, common::DOC_KEY_VERSION, 0u64).unwrap();
        doc
    };

    let (tx, _rx) = broadcast::channel(100);

    let app_state = Arc::new(AppState {
        doc: Mutex::new(doc),
        db,
        tx,
    });

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(app_state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
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
                                    if let Ok(sync_msg) = sync::Message::decode(&binary) {
                                        let reply_msg = {
                                            let mut doc = state.doc.lock().await;
                                            
                                            let heads_before = doc.get_heads();
                                            doc.sync().receive_sync_message(&mut sync_state, sync_msg).unwrap();
                                            let heads_after = doc.get_heads();

                                            if heads_before != heads_after {
                                                let saved = doc.save();
                                                state.db.insert("doc_data", saved).unwrap();
                                                
                                                // Notify others
                                                let _ = state.tx.send(());
                                            }
                                            
                                            // Generate reply while lock is held
                                            let x = doc.sync().generate_sync_message(&mut sync_state);
                                            x
                                        }; // Lock dropped

                                        if let Some(msg) = reply_msg {
                                            let resp = WsMessage::Sync(msg.encode());
                                            let json = serde_json::to_string(&resp).unwrap();
                                            if sender.send(Message::Text(json.into())).await.is_err() {
                                                break;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => break,
                }
            }
            _ = rx.recv() => {
                let reply_msg = {
                    let mut doc = state.doc.lock().await;                    let x = doc.sync().generate_sync_message(&mut sync_state);
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
        }
    }
}

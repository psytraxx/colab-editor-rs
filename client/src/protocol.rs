//! Wire protocol shared with the Cloudflare Worker relay, plus the document
//! field keys used as Automerge map keys at the document root.

use serde::{Deserialize, Serialize};

// Document field keys
pub const DOC_KEY_TITLE: &str = "title";
pub const DOC_KEY_BODY: &str = "body";
pub const DOC_KEY_KEYWORDS: &str = "keywords";
pub const DOC_KEY_VERSION: &str = "version";

// WebSocket message types
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub enum WsMessage {
    Init {
        user_id: String,
        snapshot: Option<Vec<u8>>,
        users: Vec<UserState>,
    },
    Content(Vec<u8>),
    UserState(UserState),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserState {
    pub user_id: String,
    pub online: bool,
    pub editing: bool,
}

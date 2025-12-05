use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum WsMessage {
    /// Welcome message with assigned user ID
    Welcome(String),
    /// Automerge sync message (binary)
    Sync(Vec<u8>),
    /// User state update (presence, cursor, editing mode)
    UserState(UserState),
}

/// Represents a user's current state including presence and cursor
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserState {
    pub user_id: String,
    pub user_name: String,
    pub editing: bool,           // true if in edit mode
    pub field: Option<String>,   // which field they're focused on: "title", "description", "body"
    pub online: bool,            // false when disconnecting
}

pub const DOC_KEY_TITLE: &str = "title";
pub const DOC_KEY_BODY: &str = "body";
pub const DOC_KEY_DESCRIPTION: &str = "description";
pub const DOC_KEY_VERSION: &str = "version";

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum WsMessage {
    /// Automerge sync message (binary)
    Sync(Vec<u8>),
    /// Request to reset/load initial state if needed
    Join,
}

pub const DOC_KEY_TITLE: &str = "title";
pub const DOC_KEY_BODY: &str = "body";
pub const DOC_KEY_DESCRIPTION: &str = "description";
pub const DOC_KEY_VERSION: &str = "version"; // The hidden version field

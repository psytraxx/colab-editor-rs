use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum WsMessage {
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
    pub color: String,
    pub editing: bool,           // true if in edit mode
    pub field: Option<String>,   // which field they're focused on: "title", "description", "body"
    pub online: bool,            // false when disconnecting
}

pub const DOC_KEY_TITLE: &str = "title";
pub const DOC_KEY_BODY: &str = "body";
pub const DOC_KEY_DESCRIPTION: &str = "description";
pub const DOC_KEY_VERSION: &str = "version";

// Predefined colors for users
pub const USER_COLORS: [&str; 8] = [
    "#e91e63", // pink
    "#9c27b0", // purple
    "#3f51b5", // indigo
    "#03a9f4", // light blue
    "#009688", // teal
    "#8bc34a", // light green
    "#ff9800", // orange
    "#795548", // brown
];

// Random names for users
pub const USER_NAMES: [&str; 16] = [
    "Fox", "Owl", "Bear", "Wolf", "Hawk", "Deer", "Lynx", "Crow",
    "Otter", "Raven", "Moose", "Eagle", "Bison", "Heron", "Puma", "Finch",
];

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RTCError {
    #[error("Invalid base64 encoding")]
    InvalidBase64(#[from] base64::DecodeError),

    #[error("Invalid UTF-8 string")]
    InvalidString(#[from] std::str::Utf8Error),

    #[error("JSON serialization error")]
    SerializationError(#[from] serde_json::Error),

    #[error("Invalid WebRTC offer")]
    InvalidOffer,

    #[error("WebRTC connection error: {0}")]
    ConnectionError(String),

    #[error("State error: {0}")]
    StateError(String),
}

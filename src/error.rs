use thiserror::Error;

/// Result type for Ascend operations
pub type Result<T> = std::result::Result<T, AscendError>;

/// Errors that can occur when interacting with Ascend speakers
#[derive(Error, Debug)]
pub enum AscendError {
    /// WebSocket connection error
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    /// Connection was closed unexpectedly
    #[error("Connection closed")]
    ConnectionClosed,

    /// Request timed out waiting for response
    #[error("Request timeout")]
    Timeout,

    /// API returned an error response
    #[error("API error: {detail}")]
    ApiError {
        /// Error detail message from the API
        detail: String,
    },

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Room not found during discovery
    #[error("Room not found: {0}")]
    RoomNotFound(String),

    /// Invalid or unexpected response from API
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// Channel receive error
    #[error("Channel error: {0}")]
    ChannelError(String),
}

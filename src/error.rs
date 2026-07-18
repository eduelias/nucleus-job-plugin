//! Error types for the jobs runner.

use std::time::Duration;

/// Errors produced by the jobs engine, transport, and handler layers.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A transport (MQTT/IPC) level failure.
    #[error("transport error: {0}")]
    Transport(String),

    /// Failed to (de)serialize a Jobs protocol payload.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// The IoT Jobs service rejected a request (on a `.../rejected` topic).
    #[error("jobs request rejected: {code}: {message}")]
    Rejected {
        /// Service error code (e.g. `VersionMismatch`, `InvalidStateTransition`).
        code: String,
        /// Human-readable message from the service.
        message: String,
    },

    /// The job document could not be understood by this runner.
    #[error("invalid job document: {0}")]
    InvalidJobDocument(String),

    /// The requested handler is not permitted (not in the allow-list directory).
    #[error("handler not allowed: {0}")]
    HandlerNotAllowed(String),

    /// The handler process could not be spawned or waited on.
    #[error("handler execution failed: {0}")]
    HandlerExec(String),

    /// The handler exceeded its allotted time budget.
    #[error("handler timed out after {0:?}")]
    HandlerTimeout(Duration),

    /// Configuration was missing or invalid.
    #[error("configuration error: {0}")]
    Config(String),

    /// An underlying I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience result alias.
pub type Result<T> = std::result::Result<T, Error>;

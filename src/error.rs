//! Error types for RustRoutines
//!
//! This module provides error handling types used throughout the library.

use thiserror::Error;

/// Main error type for RustRoutines operations
#[derive(Error, Debug)]
pub enum Error {
    /// Channel send operation failed
    #[error("Failed to send on channel: {reason}")]
    SendError {
        /// Reason for the send failure
        reason: String
    },
    
    /// Channel receive operation failed
    #[error("Failed to receive from channel: {reason}")]
    RecvError {
        /// Reason for the receive failure
        reason: String
    },
    
    /// Channel is closed
    #[error("Channel is closed")]
    ChannelClosed,
    
    /// Routine spawn failed
    #[error("Failed to spawn routine: {reason}")]
    SpawnError {
        /// Reason for the spawn failure
        reason: String
    },
    
    /// Runtime error
    #[error("Runtime error: {reason}")]
    RuntimeError {
        /// Reason for the runtime error
        reason: String
    },
    
    /// Timeout error
    #[error("Operation timed out")]
    Timeout,
}

/// Convenient result type alias
pub type Result<T> = std::result::Result<T, Error>;

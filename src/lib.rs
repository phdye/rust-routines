//! # RustRoutines
//!
//! Go-like concurrency primitives for Rust, providing goroutines, channels, and select functionality.
//!
//! ## Features
//!
//! - **Routines**: Lightweight, stackful coroutines similar to Go's goroutines
//! - **Channels**: Type-safe message passing with buffered and unbuffered channels
//! - **Select**: Multi-channel operations with timeout and default cases
//! - **Runtime**: Efficient work-stealing scheduler for managing routines
//!
//! ## Quick Start
//!
//! ```rust
//! use rust_routines::prelude::*;
//!
//! # #[tokio::main]
//! # async fn main() {
//! // Create a channel
//! let (tx, rx) = channel::<i32>();
//!
//! // Spawn a routine
//! go!(async move {
//!     tx.send(42).await.unwrap();
//! });
//!
//! // Receive from the channel
//! let value = rx.recv().await.unwrap();
//! assert_eq!(value, 42);
//! # }
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]
#![warn(clippy::all)]
#![deny(unsafe_code)]

pub mod channel;
pub mod routine;
pub mod select;
pub mod runtime;
pub mod error;
pub mod scheduler;

/// Convenient re-exports for common functionality
pub mod prelude {
    pub use crate::channel::{channel, bounded_channel, unbounded_channel};
    pub use crate::routine::{spawn, sleep, yield_now};
    pub use crate::select::{select, Select};
    pub use crate::error::{Error, Result};
    
    /// Macro for spawning goroutines with Go-like syntax
    #[macro_export]
    macro_rules! go {
        // Support async move blocks
        (async move $body:block) => {
            $crate::routine::spawn(async move $body)
        };
        // Support async blocks
        (async $body:block) => {
            $crate::routine::spawn(async $body)
        };
        // Support move blocks (implicitly async)
        (move $body:block) => {
            $crate::routine::spawn(async move $body)
        };
        // Support any future expression
        ($expr:expr) => {
            $crate::routine::spawn($expr)
        };
    }
    
    pub use go;
}

// Re-export the prelude at crate root for convenience
pub use prelude::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn basic_channel_test() {
        let (tx, mut rx) = channel::<i32>();
        
        go!(async move {
            tx.send(42).await.unwrap();
        });
        
        let value = rx.recv().await.unwrap();
        assert_eq!(value, 42);
    }
}

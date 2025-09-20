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
//! ```rust,no_run
//! use rust_routines::prelude::*;
//!
//! # #[tokio::main]
//! # async fn main() {
//! // Create a channel
//! let (tx, mut rx) = channel::<i32>();
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
pub mod select_macro;
pub mod runtime;
pub mod runtime_v2;
pub mod error;
pub mod scheduler;
pub mod bridge;
pub mod executor;
pub mod timer;
pub mod io;

/// Convenient re-exports for common functionality
pub mod prelude {
    pub use crate::channel::{channel, bounded_channel, unbounded_channel};
    pub use crate::routine::{spawn, spawn_enhanced, spawn_with_name, sleep, yield_now};
    pub use crate::select::{select_recv, select_timeout, select_fair, select_blocking, select_try, select_with_timeout};
    pub use crate::error::{Error, Result};
    pub use crate::runtime::{init_global_runtime, RuntimeConfig, with_runtime};
    pub use crate::scheduler::scheduler_stats;
    
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
    
    /// Enhanced macro for spawning goroutines with better performance monitoring
    #[macro_export]
    macro_rules! go_enhanced {
        // Support async move blocks
        (async move $body:block) => {
            $crate::routine::spawn_enhanced(async move $body)
        };
        // Support async blocks
        (async $body:block) => {
            $crate::routine::spawn_enhanced(async $body)
        };
        // Support move blocks (implicitly async)
        (move $body:block) => {
            $crate::routine::spawn_enhanced(async move $body)
        };
        // Support any future expression
        ($expr:expr) => {
            $crate::routine::spawn_enhanced($expr)
        };
    }
    
    /// Named macro for spawning goroutines with debugging names
    #[macro_export]
    macro_rules! go_named {
        // Support async move blocks with name
        ($name:expr, async move $body:block) => {
            $crate::routine::spawn_with_name(async move $body, $name.to_string())
        };
        // Support async blocks with name
        ($name:expr, async $body:block) => {
            $crate::routine::spawn_with_name(async $body, $name.to_string())
        };
        // Support move blocks with name (implicitly async)
        ($name:expr, move $body:block) => {
            $crate::routine::spawn_with_name(async move $body, $name.to_string())
        };
        // Support any future expression with name
        ($name:expr, $expr:expr) => {
            $crate::routine::spawn_with_name($expr, $name.to_string())
        };
    }
    
    pub use go;
    pub use go_enhanced;
    pub use go_named;
}

// Re-export the prelude at crate root for convenience
pub use prelude::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_channel_test() {
        let (tx, rx) = channel::<i32>();
        
        // Spawn a thread to send value
        std::thread::spawn(move || {
            tx.send(42).unwrap();
        });
        
        let value = rx.recv().unwrap();
        assert_eq!(value, 42);
    }
}

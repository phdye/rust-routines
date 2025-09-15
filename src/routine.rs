//! Routine (goroutine-like) implementation for RustRoutines
//!
//! Provides lightweight, stackful coroutines similar to Go's goroutines.

use crate::error::{Error, Result};
use std::future::Future;
use tokio::task::JoinHandle;

/// A handle to a spawned routine
pub struct RoutineHandle<T> {
    inner: JoinHandle<T>,
}

/// Spawn a new routine (goroutine equivalent)
pub fn spawn<F, T>(future: F) -> RoutineHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handle = tokio::spawn(future);
    RoutineHandle { inner: handle }
}

/// Alias for spawn - more Go-like naming
pub fn go<F, T>(future: F) -> RoutineHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    spawn(future)
}

impl<T> RoutineHandle<T> {
    /// Wait for the routine to complete and get its result
    pub async fn join(self) -> Result<T> {
        self.inner.await
            .map_err(|e| Error::RuntimeError { 
                reason: format!("Routine panicked: {}", e) 
            })
    }
    
    /// Abort the routine
    pub fn abort(&self) {
        self.inner.abort();
    }
    
    /// Check if the routine is finished
    pub fn is_finished(&self) -> bool {
        self.inner.is_finished()
    }
}

/// Yield control to allow other routines to run
pub async fn yield_now() {
    tokio::task::yield_now().await;
}

/// Sleep for the specified duration
pub async fn sleep(duration: std::time::Duration) {
    tokio::time::sleep(duration).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_spawn_routine() {
        let handle = spawn(async {
            42
        });
        
        let result = handle.join().await.unwrap();
        assert_eq!(result, 42);
    }
    
    #[tokio::test]
    async fn test_go_macro_equivalent() {
        let handle = go(async {
            "hello from routine".to_string()
        });
        
        let result = handle.join().await.unwrap();
        assert_eq!(result, "hello from routine");
    }
    
    #[tokio::test]
    async fn test_yield_now() {
        let start = std::time::Instant::now();
        yield_now().await;
        // Should complete quickly
        assert!(start.elapsed() < Duration::from_millis(100));
    }
}

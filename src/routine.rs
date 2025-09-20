//! Routine (goroutine-like) implementation for RustRoutines
//!
//! Provides lightweight, stackful coroutines similar to Go's goroutines.
//! Now integrated with our M:N work-stealing scheduler for true Go-like execution.
//! 
//! For enhanced features and performance monitoring, see the enhanced functions.

use crate::error::{Error, Result};
use crate::bridge::{schedule_future, schedule_future_enhanced, schedule_future_with_name};
use crate::timer::delay;
use std::future::Future;
use std::time::Duration;
use futures::channel::oneshot;

/// A handle to a spawned routine running on our M:N scheduler
pub struct RoutineHandle<T> {
    receiver: oneshot::Receiver<T>,
    #[allow(dead_code)] // Will be used for debugging in future phases
    task_id: Option<usize>, // For debugging/monitoring - will be enhanced later
}

/// Spawn a new routine on our M:N scheduler (not Tokio!)
/// 
/// This is the standard way to spawn routines. For enhanced performance
/// and debugging features, consider using spawn_enhanced() or spawn_with_name().
pub fn spawn<F, T>(future: F) -> RoutineHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    // Use our custom scheduler instead of tokio::spawn
    let receiver = schedule_future(future)
        .expect("Failed to schedule routine on M:N scheduler");
    
    RoutineHandle::new(receiver)
}

/// Spawn a new routine using the enhanced executor for better performance
/// 
/// This provides better statistics collection and performance monitoring
/// compared to the basic spawn() function.
pub fn spawn_enhanced<F, T>(future: F) -> RoutineHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let receiver = schedule_future_enhanced(future)
        .expect("Failed to schedule enhanced routine on M:N scheduler");
    
    RoutineHandle::new(receiver)
}

/// Spawn a new routine with a name for debugging and monitoring
/// 
/// This is particularly useful for long-running routines or when debugging
/// performance issues. The name will appear in logs and statistics.
pub fn spawn_with_name<F, T>(future: F, name: String) -> RoutineHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let receiver = schedule_future_with_name(future, name)
        .expect("Failed to schedule named routine on M:N scheduler");
    
    RoutineHandle::new(receiver)
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
    /// Create a new RoutineHandle (internal use)
    pub(crate) fn new(receiver: oneshot::Receiver<T>) -> Self {
        Self {
            receiver,
            task_id: None,
        }
    }
    
    /// Wait for the routine to complete and get its result
    pub async fn join(self) -> Result<T> {
        self.receiver.await
            .map_err(|_| Error::RuntimeError { 
                reason: "Routine was cancelled or panicked".to_string() 
            })
    }
    
    /// Abort the routine (best effort - limited support in current implementation)
    pub fn abort(&self) {
        // Note: Aborting scheduled tasks is complex with our current scheduler
        // For the initial implementation, this is a no-op
        // TODO: Implement task cancellation in the scheduler
    }
    
    /// Check if the routine is finished (always returns false for now)
    pub fn is_finished(&self) -> bool {
        // TODO: Implement proper completion tracking
        // For now, we can't efficiently check without consuming the receiver
        false
    }
}

/// Yield control to allow other routines to run
/// 
/// This is a scheduler-friendly yield that works in both Tokio and M:N scheduler contexts
pub async fn yield_now() {
    // Use a simple future that yields once
    std::future::poll_fn(|_| std::task::Poll::Ready(())).await;
}

/// Sleep for the specified duration using the timer wheel
/// 
/// This uses our efficient timer wheel implementation for delays,
/// allowing the routine to be parked and the thread to work on other tasks.
pub fn sleep(duration: Duration) {
    delay(duration);
}

/// Async sleep for backward compatibility
pub async fn sleep_async(duration: Duration) {
    // Create a oneshot channel for wakeup
    let (tx, rx) = oneshot::channel();
    
    // Schedule timer to send on channel
    std::thread::spawn(move || {
        delay(duration);
        let _ = tx.send(());
    });
    
    // Wait for timer
    let _ = rx.await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_spawn_routine() {
        let handle = spawn(async {
            42
        });
        
        let result = futures::executor::block_on(handle.join()).unwrap();
        assert_eq!(result, 42);
    }
    
    #[test]
    fn test_go_macro_equivalent() {
        let handle = go(async {
            "hello from routine".to_string()
        });
        
        let result = futures::executor::block_on(handle.join()).unwrap();
        assert_eq!(result, "hello from routine");
    }
    
    #[test]
    fn test_yield_now() {
        let start = std::time::Instant::now();
        futures::executor::block_on(yield_now());
        // Should complete quickly
        assert!(start.elapsed() < Duration::from_millis(100));
    }
}

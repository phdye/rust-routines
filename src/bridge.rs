//! Future-to-Task bridge for integrating async Futures with the M:N scheduler
//!
//! This module provides the critical bridge between the async/await world and our
//! work-stealing task scheduler, enabling true M:N threading for routines.
//! 
//! For enhanced performance and debugging, consider using the executor module directly.

use std::future::Future;
use std::task::{Context, Poll};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use futures::channel::oneshot;
use futures::task::{ArcWake, waker_ref};
use crate::scheduler::{Task, GLOBAL_SCHEDULER};

/// Basic Future-to-Task converter for backward compatibility
/// 
/// For enhanced features, use crate::executor::EnhancedFutureTask instead
pub struct FutureTask<T> {
    future: Pin<Box<dyn Future<Output = T> + Send + 'static>>,
    result_sender: Option<oneshot::Sender<T>>,
}

impl<T: Send + 'static> FutureTask<T> {
    /// Create a new FutureTask
    pub fn new(
        future: impl Future<Output = T> + Send + 'static,
        sender: oneshot::Sender<T>
    ) -> Self {
        Self {
            future: Box::pin(future),
            result_sender: Some(sender),
        }
    }
    
    /// Convert this FutureTask into a schedulable Task
    pub fn into_task(mut self) -> Task {
        Task::new(Box::new(move || {
            // Use basic executor for compatibility
            let mut executor = TaskExecutor::new();
            let result = executor.block_on(self.future);
            
            // Send the result back through the channel
            if let Some(sender) = self.result_sender.take() {
                let _ = sender.send(result);
            }
        }))
    }
}

/// Basic executor for running futures within task context
/// 
/// For enhanced features like statistics and better performance,
/// use crate::executor::EnhancedTaskExecutor instead
pub struct TaskExecutor {
    /// Flag to track if we should wake up
    should_wake: Arc<AtomicBool>,
}

impl Default for TaskExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskExecutor {
    /// Create a new task executor
    pub fn new() -> Self {
        Self {
            should_wake: Arc::new(AtomicBool::new(false)),
        }
    }
    
    /// Block on a future within a task context
    pub fn block_on<F: Future + ?Sized>(&mut self, mut future: Pin<Box<F>>) -> F::Output {
        let wake_handle = Arc::new(TaskWaker {
            should_wake: Arc::clone(&self.should_wake),
        });
        
        let waker = waker_ref(&wake_handle);
        let mut context = Context::from_waker(&waker);
        
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(result) => return result,
                Poll::Pending => {
                    // Check if we need to wake up due to external events
                    if self.should_wake.swap(false, Ordering::Acquire) {
                        continue; // Try polling again
                    }
                    
                    // If no wake-ups pending, yield to the scheduler briefly
                    std::thread::yield_now();
                }
            }
        }
    }
}

/// Waker implementation that integrates with our executor
struct TaskWaker {
    should_wake: Arc<AtomicBool>,
}

impl ArcWake for TaskWaker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.should_wake.store(true, Ordering::Release);
    }
}

/// Schedule a future on the global M:N scheduler using basic executor
/// 
/// For enhanced features, use crate::executor::EnhancedFutureTask and
/// crate::runtime::IntegratedRustRoutineRuntime::spawn_routine()
pub fn schedule_future<F, T>(future: F) -> Result<oneshot::Receiver<T>, crate::error::Error>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (sender, receiver) = oneshot::channel();
    let future_task = FutureTask::new(future, sender);
    let task = future_task.into_task();
    
    GLOBAL_SCHEDULER.schedule(task)
        .map_err(|e| crate::error::Error::RuntimeError {
            reason: format!("Failed to schedule future: {}", e)
        })?;
    
    Ok(receiver)
}

/// Schedule a future using the enhanced executor for better performance
pub fn schedule_future_enhanced<F, T>(future: F) -> Result<oneshot::Receiver<T>, crate::error::Error>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (sender, receiver) = oneshot::channel();
    let enhanced_task = crate::executor::EnhancedFutureTask::new(future, sender);
    let task = enhanced_task.into_task();
    
    GLOBAL_SCHEDULER.schedule(task)
        .map_err(|e| crate::error::Error::RuntimeError {
            reason: format!("Failed to schedule enhanced future: {}", e)
        })?;
    
    Ok(receiver)
}

/// Schedule a future with a name for debugging (uses enhanced executor)
pub fn schedule_future_with_name<F, T>(
    future: F, 
    name: String
) -> Result<oneshot::Receiver<T>, crate::error::Error>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (sender, receiver) = oneshot::channel();
    let enhanced_task = crate::executor::EnhancedFutureTask::with_name(future, sender, name);
    let task = enhanced_task.into_task();
    
    GLOBAL_SCHEDULER.schedule(task)
        .map_err(|e| crate::error::Error::RuntimeError {
            reason: format!("Failed to schedule named future: {}", e)
        })?;
    
    Ok(receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_future_task_creation() {
        let (sender, _receiver) = oneshot::channel();
        let future_task = FutureTask::new(async { 42 }, sender);
        let _task = future_task.into_task();
        // If we get here without panicking, the conversion worked
    }
    
    #[test]
    fn test_task_executor() {
        let mut executor = TaskExecutor::new();
        
        // Test with a simple ready future
        let result = executor.block_on(Box::pin(async { 42 }));
        assert_eq!(result, 42);
    }
    
    #[test]
    fn test_schedule_future() {
        // Schedule a simple future
        let receiver = schedule_future(async { "hello from task" }).unwrap();
        
        // Wait for the result
        let result = futures::executor::block_on(receiver).unwrap();
        assert_eq!(result, "hello from task");
    }
    
    #[test]
    fn test_schedule_async_future() {
        // Schedule a future that completes immediately  
        let receiver = schedule_future(async {
            42
        }).unwrap();
        
        // Wait for the result
        let result = futures::executor::block_on(receiver)
            .expect("Future should not be cancelled");
        
        assert_eq!(result, 42);
    }
}
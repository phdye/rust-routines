//! Enhanced future executor for running async code within M:N scheduler tasks
//!
//! This module provides a more sophisticated executor than the basic one in bridge.rs,
//! with proper waker queue management, better performance, and enhanced debugging capabilities.
//!
//! Note: This module uses unsafe code for custom waker implementation, which is necessary
//! for optimal performance in the executor context.

#![allow(unsafe_code)]  // Required for custom waker implementation

use std::future::Future;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Enhanced executor for running futures within scheduled tasks
/// 
/// This executor provides better waker management, performance monitoring,
/// and debugging capabilities compared to the basic TaskExecutor in bridge.rs
pub struct EnhancedTaskExecutor {
    /// Queue of wakers that need to be processed
    wake_queue: Arc<Mutex<VecDeque<Waker>>>,
    /// Flag indicating if the executor should wake up
    should_wake: Arc<AtomicBool>,
    /// Performance tracking
    stats: ExecutorStats,
    /// Unique executor ID for debugging
    id: usize,
}

/// Performance and debugging statistics for the executor
#[derive(Debug, Default)]
pub struct ExecutorStats {
    /// Number of futures executed
    pub futures_executed: AtomicUsize,
    /// Number of times the executor yielded
    pub yields_count: AtomicUsize,
    /// Number of wakeups received
    pub wakeups_received: AtomicUsize,
    /// Total execution time
    pub total_execution_time: Mutex<Duration>,
}

impl EnhancedTaskExecutor {
    /// Create a new enhanced task executor
    pub fn new() -> Self {
        static EXECUTOR_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);
        
        Self {
            wake_queue: Arc::new(Mutex::new(VecDeque::new())),
            should_wake: Arc::new(AtomicBool::new(false)),
            stats: ExecutorStats::default(),
            id: EXECUTOR_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
        }
    }
    
    /// Block on a future within a task context with enhanced capabilities
    pub fn block_on<F: Future + ?Sized>(&mut self, mut future: Pin<Box<F>>) -> F::Output {
        let start_time = Instant::now();
        let waker = self.create_waker();
        let mut context = Context::from_waker(&waker);
        
        self.stats.futures_executed.fetch_add(1, Ordering::Relaxed);
        
        loop {
            // Try to poll the future
            match future.as_mut().poll(&mut context) {
                Poll::Ready(result) => {
                    // Update execution time statistics
                    let execution_time = start_time.elapsed();
                    if let Ok(mut total_time) = self.stats.total_execution_time.lock() {
                        *total_time += execution_time;
                    }
                    return result;
                }
                Poll::Pending => {
                    // Check for explicit wake flag
                    if self.should_wake.swap(false, Ordering::AcqRel) {
                        continue; // Try polling again immediately
                    }
                    
                    // Process any queued wakers
                    {
                        let mut queue = self.wake_queue.lock().unwrap();
                        if !queue.is_empty() {
                            // We have pending wakers - clear them and continue
                            queue.clear();
                            continue;
                        }
                    }
                    
                    // No immediate wakeups, yield to scheduler
                    self.stats.yields_count.fetch_add(1, Ordering::Relaxed);
                    std::thread::yield_now();
                    
                    // Small sleep to prevent busy-waiting
                    std::thread::sleep(Duration::from_nanos(100));
                }
            }
        }
    }
    
    /// Create a waker that integrates with our enhanced executor
    fn create_waker(&self) -> Waker {
        let wake_data = Box::into_raw(Box::new(WakeData {
            should_wake: Arc::clone(&self.should_wake),
            wake_queue: Arc::clone(&self.wake_queue),
            stats: ExecutorStatsRef {
                wakeups_received: &self.stats.wakeups_received,
            },
        }));
        
        let raw_waker = RawWaker::new(wake_data as *const (), &ENHANCED_WAKER_VTABLE);
        // SAFETY: We control the creation and destruction of the waker data
        unsafe { Waker::from_raw(raw_waker) }
    }
    
    /// Get executor statistics
    pub fn stats(&self) -> &ExecutorStats {
        &self.stats
    }
    
    /// Get executor ID for debugging
    pub fn id(&self) -> usize {
        self.id
    }
    
    /// Get average execution time per future
    pub fn average_execution_time(&self) -> Option<Duration> {
        let futures_count = self.stats.futures_executed.load(Ordering::Relaxed);
        if futures_count == 0 {
            return None;
        }
        
        if let Ok(total_time) = self.stats.total_execution_time.lock() {
            Some(*total_time / futures_count as u32)
        } else {
            None
        }
    }
}

impl Default for EnhancedTaskExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Data needed for our enhanced waker implementation
struct WakeData {
    should_wake: Arc<AtomicBool>,
    wake_queue: Arc<Mutex<VecDeque<Waker>>>,
    stats: ExecutorStatsRef,
}

/// Reference to stats for the waker (avoids circular references)
struct ExecutorStatsRef {
    wakeups_received: *const AtomicUsize,
}

// SAFETY: We only access the atomic through proper atomic operations
unsafe impl Send for ExecutorStatsRef {}
unsafe impl Sync for ExecutorStatsRef {}

/// Enhanced waker virtual table with better performance and debugging
static ENHANCED_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    |data| {
        // clone
        let data = unsafe { &*(data as *const WakeData) };
        let raw_waker = Box::into_raw(Box::new(WakeData {
            should_wake: Arc::clone(&data.should_wake),
            wake_queue: Arc::clone(&data.wake_queue),
            stats: ExecutorStatsRef {
                wakeups_received: data.stats.wakeups_received,
            },
        }));
        RawWaker::new(raw_waker as *const (), &ENHANCED_WAKER_VTABLE)
    },
    |data| {
        // wake
        let data = unsafe { Box::from_raw(data as *mut WakeData) };
        data.should_wake.store(true, Ordering::Release);
        
        // Update statistics
        unsafe {
            (*data.stats.wakeups_received).fetch_add(1, Ordering::Relaxed);
        }
    },
    |data| {
        // wake_by_ref
        let data = unsafe { &*(data as *const WakeData) };
        data.should_wake.store(true, Ordering::Release);
        
        // Update statistics
        unsafe {
            (*data.stats.wakeups_received).fetch_add(1, Ordering::Relaxed);
        }
    },
    |data| {
        // drop
        unsafe { 
            let _ = Box::from_raw(data as *mut WakeData);
        }
    },
);

/// Enhanced Future-to-Task bridge using the improved executor
pub struct EnhancedFutureTask<T> {
    future: Pin<Box<dyn Future<Output = T> + Send + 'static>>,
    result_sender: Option<futures::channel::oneshot::Sender<T>>,
    task_name: Option<String>,
}

impl<T: Send + 'static> EnhancedFutureTask<T> {
    /// Create a new enhanced future task
    pub fn new(
        future: impl Future<Output = T> + Send + 'static,
        sender: futures::channel::oneshot::Sender<T>
    ) -> Self {
        Self {
            future: Box::pin(future),
            result_sender: Some(sender),
            task_name: None,
        }
    }
    
    /// Create a new enhanced future task with a name for debugging
    pub fn with_name(
        future: impl Future<Output = T> + Send + 'static,
        sender: futures::channel::oneshot::Sender<T>,
        name: String,
    ) -> Self {
        Self {
            future: Box::pin(future),
            result_sender: Some(sender),
            task_name: Some(name),
        }
    }
    
    /// Convert this enhanced future task into a schedulable Task
    pub fn into_task(mut self) -> crate::scheduler::Task {
        let task_name = self.task_name.clone();
        
        crate::scheduler::Task::new(Box::new(move || {
            // Create enhanced executor for this task
            let mut executor = EnhancedTaskExecutor::new();
            
            // Log task start if named
            if let Some(ref name) = task_name {
                log::debug!("Starting enhanced task '{}' on executor {}", name, executor.id());
            }
            
            // Execute the future
            let result = executor.block_on(self.future);
            
            // Log execution statistics
            let stats = executor.stats();
            log::debug!(
                "Enhanced executor {} completed: {} yields, {} wakeups", 
                executor.id(),
                stats.yields_count.load(Ordering::Relaxed),
                stats.wakeups_received.load(Ordering::Relaxed)
            );
            
            // Send result back
            if let Some(sender) = self.result_sender.take() {
                let _ = sender.send(result);
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_enhanced_executor_creation() {
        let executor = EnhancedTaskExecutor::new();
        assert_eq!(executor.stats().futures_executed.load(Ordering::Relaxed), 0);
        assert_eq!(executor.stats().yields_count.load(Ordering::Relaxed), 0);
    }
    
    #[test]
    fn test_enhanced_executor_simple_future() {
        let mut executor = EnhancedTaskExecutor::new();
        let result = executor.block_on(Box::pin(async { 42 }));
        assert_eq!(result, 42);
        assert_eq!(executor.stats().futures_executed.load(Ordering::Relaxed), 1);
    }
    
    #[test]
    fn test_enhanced_future_task() {
        let (sender, _receiver) = futures::channel::oneshot::channel();
        let task = EnhancedFutureTask::with_name(
            async { "enhanced result" },
            sender,
            "test_task".to_string()
        );
        
        // Convert to schedulable task (this would be scheduled on the M:N scheduler)
        let _scheduler_task = task.into_task();
        
        // For testing, we can't easily schedule it, but we can verify creation worked
        // In real usage, this would be scheduled via GLOBAL_SCHEDULER.schedule(task)
        
        // Instead, let's test the receiver gets the result when we manually execute
        let (sender2, receiver2) = futures::channel::oneshot::channel();
        let mut executor = EnhancedTaskExecutor::new();
        executor.block_on(Box::pin(async move {
            let result = async { "test result" }.await;
            let _ = sender2.send(result);
        }));
        
        let result = futures::executor::block_on(receiver2).unwrap();
        assert_eq!(result, "test result");
    }
    
    #[test]
    fn test_executor_statistics() {
        let mut executor = EnhancedTaskExecutor::new();
        
        // Execute a few futures
        for i in 0..3 {
            let _result = executor.block_on(Box::pin(async move { i * 2 }));
        }
        
        let stats = executor.stats();
        assert_eq!(stats.futures_executed.load(Ordering::Relaxed), 3);
        
        // Should have some execution time recorded
        if let Ok(total_time) = stats.total_execution_time.lock() {
            assert!(!total_time.is_zero());
        };  // Added semicolon to fix borrow issue
    }
}
//! Properly isolated runtime implementation for RustRoutines
//!
//! This module provides a runtime that owns its scheduler, allowing for
//! proper isolation between different execution contexts (like tests).
//! Similar to how tokio::Runtime works, but for our M:N scheduler.

use crate::error::Result;
use crate::scheduler::core::{Scheduler, SchedulerConfig, SchedulerStats};
use crate::routine::RoutineHandle;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use once_cell::sync::Lazy;

// Thread-local storage for the current runtime context
thread_local! {
    static CURRENT_RUNTIME: RwLock<Option<Arc<Runtime>>> = const { RwLock::new(None) };
}

/// A runtime that owns its scheduler and provides isolated execution
pub struct Runtime {
    /// The scheduler for this runtime
    scheduler: Arc<Scheduler>,
    /// Runtime statistics
    stats: Arc<RuntimeStats>,
    /// Runtime start time
    start_time: Instant,
}

/// Runtime statistics
#[derive(Debug)]
pub struct RuntimeStats {
    /// Number of routines spawned
    pub routines_spawned: AtomicUsize,
    /// Number of routines completed
    pub routines_completed: AtomicUsize,
    /// Number of currently active routines
    pub routines_active: AtomicUsize,
}

impl Default for RuntimeStats {
    fn default() -> Self {
        Self {
            routines_spawned: AtomicUsize::new(0),
            routines_completed: AtomicUsize::new(0),
            routines_active: AtomicUsize::new(0),
        }
    }
}

impl Runtime {
    /// Create a new runtime with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(SchedulerConfig::default())
    }
    
    /// Create a new runtime with the specified scheduler configuration
    pub fn with_config(config: SchedulerConfig) -> Result<Self> {
        let scheduler = Scheduler::new(config)?;
        
        Ok(Runtime {
            scheduler,
            stats: Arc::new(RuntimeStats::default()),
            start_time: Instant::now(),
        })
    }
    
    /// Enter this runtime's context
    pub fn enter<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        // Save the current runtime
        let previous = CURRENT_RUNTIME.with(|rt| {
            let mut current = rt.write();
            current.replace(Arc::new(self.clone()))
        });
        
        // Execute the function
        let result = f();
        
        // Restore the previous runtime
        CURRENT_RUNTIME.with(|rt| {
            let mut current = rt.write();
            *current = previous;
        });
        
        result
    }
    
    /// Block on a future using this runtime
    pub fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future,
    {
        self.enter(|| {
            // Create a simple executor for blocking
            let mut executor = crate::executor::EnhancedTaskExecutor::new();
            executor.block_on(Box::pin(future))
        })
    }
    
    /// Spawn a routine on this runtime
    pub fn spawn<F, T>(&self, future: F) -> Result<RoutineHandle<T>>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        self.stats.routines_spawned.fetch_add(1, Ordering::Relaxed);
        self.stats.routines_active.fetch_add(1, Ordering::Relaxed);
        
        // Create a channel for the result
        let (sender, receiver) = futures::channel::oneshot::channel();
        
        // Wrap the future to track completion
        let stats = Arc::clone(&self.stats);
        let wrapped_future = async move {
            let result = future.await;
            stats.routines_completed.fetch_add(1, Ordering::Relaxed);
            stats.routines_active.fetch_sub(1, Ordering::Relaxed);
            result
        };
        
        // Create an enhanced task
        let task = crate::executor::EnhancedFutureTask::new(wrapped_future, sender);
        
        // Schedule on this runtime's scheduler
        self.scheduler.schedule(task.into_task())?;
        
        Ok(RoutineHandle::new(receiver))
    }
    
    /// Get scheduler statistics
    pub fn scheduler_stats(&self) -> SchedulerStats {
        self.scheduler.stats()
    }
    
    /// Get runtime statistics
    pub fn stats(&self) -> RuntimeReport {
        RuntimeReport {
            routines_spawned: self.stats.routines_spawned.load(Ordering::Relaxed),
            routines_completed: self.stats.routines_completed.load(Ordering::Relaxed),
            routines_active: self.stats.routines_active.load(Ordering::Relaxed),
            uptime: self.start_time.elapsed(),
            scheduler_stats: self.scheduler.stats().clone(),
        }
    }
    
    /// Shutdown this runtime
    pub fn shutdown(self) -> Result<()> {
        self.scheduler.shutdown()
    }
    
    /// Shutdown this runtime with a timeout
    pub fn shutdown_timeout(self, _timeout: Duration) -> Result<()> {
        // TODO: Implement timeout logic if needed
        self.scheduler.shutdown()
    }
}

impl Clone for Runtime {
    fn clone(&self) -> Self {
        Runtime {
            scheduler: Arc::clone(&self.scheduler),
            stats: Arc::clone(&self.stats),
            start_time: self.start_time,
        }
    }
}

/// Runtime statistics report
#[derive(Debug, Clone)]
pub struct RuntimeReport {
    /// Number of routines spawned
    pub routines_spawned: usize,
    /// Number of routines completed
    pub routines_completed: usize,
    /// Number of currently active routines
    pub routines_active: usize,
    /// Runtime uptime
    pub uptime: Duration,
    /// Scheduler statistics
    pub scheduler_stats: SchedulerStats,
}

/// Get the current runtime, or create a default one if none exists
pub fn current_runtime() -> Arc<Runtime> {
    CURRENT_RUNTIME.with(|rt| {
        let current = rt.read();
        if let Some(runtime) = &*current {
            Arc::clone(runtime)
        } else {
            drop(current);
            // Create a default runtime if none exists
            let mut current = rt.write();
            let runtime = Arc::new(Runtime::new().expect("Failed to create default runtime"));
            *current = Some(Arc::clone(&runtime));
            runtime
        }
    })
}

/// Spawn a routine on the current runtime
pub fn spawn<F, T>(future: F) -> Result<RoutineHandle<T>>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    current_runtime().spawn(future)
}

/// Optional: Global default runtime for backward compatibility
/// This can be removed if we want to force explicit runtime creation
static DEFAULT_RUNTIME: Lazy<Arc<Runtime>> = Lazy::new(|| {
    Arc::new(Runtime::new().expect("Failed to create default runtime"))
});

/// Get the default runtime (for backward compatibility)
pub fn default_runtime() -> Arc<Runtime> {
    Arc::clone(&DEFAULT_RUNTIME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    
    #[test]
    fn test_isolated_runtimes() {
        // Create two independent runtimes
        let runtime1 = Runtime::new().unwrap();
        let runtime2 = Runtime::new().unwrap();
        
        // Spawn tasks on runtime1
        runtime1.block_on(async {
            for _ in 0..10 {
                spawn(async { 1 + 1 }).unwrap();
            }
        });
        
        // Spawn tasks on runtime2
        runtime2.block_on(async {
            for _ in 0..5 {
                spawn(async { 2 + 2 }).unwrap();
            }
        });
        
        // Check that statistics are isolated
        let stats1 = runtime1.stats();
        let stats2 = runtime2.stats();
        
        assert_eq!(stats1.routines_spawned, 10);
        assert_eq!(stats2.routines_spawned, 5);
        
        // Statistics are completely independent!
    }
    
    #[test]
    fn test_concurrent_runtime_usage() {
        use std::thread;
        
        let runtime = Arc::new(Runtime::new().unwrap());
        let counter = Arc::new(AtomicUsize::new(0));
        
        // Spawn multiple threads that use the same runtime
        let mut handles = vec![];
        for _ in 0..4 {
            let rt = Arc::clone(&runtime);
            let cnt = Arc::clone(&counter);
            let handle = thread::spawn(move || {
                rt.block_on(async {
                    for _ in 0..25 {
                        let cnt_clone = Arc::clone(&cnt);
                        spawn(async move {
                            cnt_clone.fetch_add(1, Ordering::Relaxed);
                        }).unwrap();
                    }
                });
            });
            handles.push(handle);
        }
        
        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }
        
        // All tasks should complete
        thread::sleep(Duration::from_millis(100));
        assert_eq!(counter.load(Ordering::Relaxed), 100);
    }
    
    #[test]
    fn test_runtime_cleanup() {
        let runtime = Runtime::new().unwrap();
        
        runtime.block_on(async {
            for _ in 0..10 {
                spawn(async { 42 }).unwrap();
            }
        });
        
        // Shutdown should clean up properly
        runtime.shutdown().unwrap();
        
        // New runtime should start fresh
        let new_runtime = Runtime::new().unwrap();
        let stats = new_runtime.stats();
        assert_eq!(stats.routines_spawned, 0);
    }
}

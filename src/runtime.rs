//! Runtime implementation for RustRoutines
//!
//! Provides an efficient work-stealing scheduler for managing routines.

use crate::error::{Error, Result};
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::runtime::{Builder, Runtime};

/// Global runtime statistics
#[derive(Debug, Default)]
pub struct RuntimeStats {
    /// Number of routines spawned
    pub routines_spawned: AtomicUsize,
    /// Number of routines completed
    pub routines_completed: AtomicUsize,
    /// Number of active routines
    pub routines_active: AtomicUsize,
}

/// Runtime configuration
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Number of worker threads (0 = auto-detect)
    pub worker_threads: usize,
    /// Thread name prefix
    pub thread_name: String,
    /// Enable I/O driver
    pub enable_io: bool,
    /// Enable timer driver
    pub enable_time: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            worker_threads: 0, // Auto-detect
            thread_name: "rust-routine".to_string(),
            enable_io: true,
            enable_time: true,
        }
    }
}

/// The RustRoutines runtime
pub struct RustRoutineRuntime {
    runtime: Runtime,
    stats: Arc<RuntimeStats>,
    config: RuntimeConfig,
}

impl RustRoutineRuntime {
    /// Create a new runtime with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(RuntimeConfig::default())
    }
    
    /// Create a new runtime with the specified configuration
    pub fn with_config(config: RuntimeConfig) -> Result<Self> {
        let mut builder = Builder::new_multi_thread();
        
        if config.worker_threads > 0 {
            builder.worker_threads(config.worker_threads);
        }
        
        builder.thread_name(&config.thread_name);
        
        if config.enable_io {
            builder.enable_io();
        }
        
        if config.enable_time {
            builder.enable_time();
        }
        
        let runtime = builder.build()
            .map_err(|e| Error::RuntimeError { 
                reason: format!("Failed to create runtime: {}", e) 
            })?;
            
        Ok(Self {
            runtime,
            stats: Arc::new(RuntimeStats::default()),
            config,
        })
    }
    
    /// Spawn a future on the runtime
    pub fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.stats.routines_spawned.fetch_add(1, Ordering::Relaxed);
        self.stats.routines_active.fetch_add(1, Ordering::Relaxed);
        
        let stats = Arc::clone(&self.stats);
        let handle = self.runtime.spawn(async move {
            let result = future.await;
            stats.routines_completed.fetch_add(1, Ordering::Relaxed);
            stats.routines_active.fetch_sub(1, Ordering::Relaxed);
            result
        });
        
        handle
    }
    
    /// Block on a future
    pub fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future,
    {
        self.runtime.block_on(future)
    }
    
    /// Get runtime statistics
    pub fn stats(&self) -> RuntimeStats {
        RuntimeStats {
            routines_spawned: AtomicUsize::new(self.stats.routines_spawned.load(Ordering::Relaxed)),
            routines_completed: AtomicUsize::new(self.stats.routines_completed.load(Ordering::Relaxed)),
            routines_active: AtomicUsize::new(self.stats.routines_active.load(Ordering::Relaxed)),
        }
    }
    
    /// Get runtime configuration
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }
    
    /// Shutdown the runtime
    pub fn shutdown_timeout(self, timeout: std::time::Duration) {
        self.runtime.shutdown_timeout(timeout);
    }
}

impl Default for RustRoutineRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to create default runtime")
    }
}

/// Get the current runtime statistics (requires a global runtime)
pub fn current_runtime_stats() -> Option<RuntimeStats> {
    // This would require a global runtime instance
    // For now, return None - will implement global runtime later
    None
}

/// Initialize the global runtime with the specified configuration
pub fn init_runtime(config: RuntimeConfig) -> Result<()> {
    // This would set up a global runtime instance
    // For now, just validate the config
    let _runtime = RustRoutineRuntime::with_config(config)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_runtime_creation() {
        let runtime = RustRoutineRuntime::new().unwrap();
        assert_eq!(runtime.config().thread_name, "rust-routine");
    }
    
    #[test]
    fn test_runtime_with_config() {
        let config = RuntimeConfig {
            worker_threads: 2,
            thread_name: "test-runtime".to_string(),
            enable_io: true,
            enable_time: true,
        };
        
        let runtime = RustRoutineRuntime::with_config(config).unwrap();
        assert_eq!(runtime.config().worker_threads, 2);
        assert_eq!(runtime.config().thread_name, "test-runtime");
    }
    
    #[test]
    fn test_runtime_spawn() {
        let runtime = RustRoutineRuntime::new().unwrap();
        
        let handle = runtime.spawn(async {
            42
        });
        
        let result = runtime.block_on(handle).unwrap();
        assert_eq!(result, 42);
    }
    
    #[test]
    fn test_runtime_stats() {
        let runtime = RustRoutineRuntime::new().unwrap();
        
        let initial_stats = runtime.stats();
        assert_eq!(initial_stats.routines_spawned.load(Ordering::Relaxed), 0);
        
        let _handle = runtime.spawn(async {
            tokio::time::sleep(Duration::from_millis(1)).await;
        });
        
        // Should have incremented spawn count
        let stats_after_spawn = runtime.stats();
        assert_eq!(stats_after_spawn.routines_spawned.load(Ordering::Relaxed), 1);
    }
}

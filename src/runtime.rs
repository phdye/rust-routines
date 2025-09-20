//! Pure M:N runtime implementation for RustRoutines
//!
//! Provides runtime management for our M:N scheduler without any Tokio dependencies.
//! This is the core runtime that manages worker threads, I/O operations, and timers.

use crate::error::{Error, Result};
use crate::scheduler::scheduler_stats;
use crate::scheduler::core::SchedulerStats;
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::thread::{self, JoinHandle};
use once_cell::sync::Lazy;
use parking_lot::{RwLock, Mutex};
use crossbeam::channel::{bounded, Sender, Receiver};

/// Global runtime statistics for the M:N scheduler
#[derive(Debug)]
pub struct RuntimeStats {
    /// Number of routines spawned
    pub routines_spawned: AtomicUsize,
    /// Number of routines completed
    pub routines_completed: AtomicUsize,
    /// Number of currently active routines
    pub routines_active: AtomicUsize,
    /// Number of I/O operations
    pub io_operations: AtomicUsize,
    /// Number of timer operations
    pub timer_operations: AtomicUsize,
}

impl Default for RuntimeStats {
    fn default() -> Self {
        Self {
            routines_spawned: AtomicUsize::new(0),
            routines_completed: AtomicUsize::new(0),
            routines_active: AtomicUsize::new(0),
            io_operations: AtomicUsize::new(0),
            timer_operations: AtomicUsize::new(0),
        }
    }
}

/// Runtime configuration for the M:N scheduler
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Number of CPU worker threads (0 = auto-detect)
    pub cpu_threads: usize,
    /// Number of I/O worker threads
    pub io_threads: usize,
    /// Thread name prefix for CPU workers
    pub cpu_thread_name: String,
    /// Thread name prefix for I/O workers
    pub io_thread_name: String,
    /// Enable CPU affinity for workers
    pub enable_cpu_affinity: bool,
    /// Work-stealing batch size
    pub steal_batch_size: usize,
    /// Park timeout for workers in milliseconds
    pub park_timeout_ms: u64,
    /// Enable NUMA awareness
    pub numa_aware: bool,
    /// Enable detailed statistics
    pub enable_stats: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        let cpu_count = num_cpus::get();
        
        Self {
            cpu_threads: cpu_count,
            io_threads: 128, // Default I/O thread pool size (like Go)
            cpu_thread_name: "cpu-worker".to_string(),
            io_thread_name: "io-worker".to_string(),
            enable_cpu_affinity: cfg!(target_os = "linux"),
            steal_batch_size: 32,
            park_timeout_ms: 1,
            numa_aware: false,
            enable_stats: true,
        }
    }
}

/// I/O operation request
enum IORequest {
    Read(String, Sender<Result<Vec<u8>>>),
    Write(String, Vec<u8>, Sender<Result<()>>),
    // Add more I/O operations as needed
}

/// The pure M:N runtime for RustRoutines
pub struct RustRoutineRuntime {
    /// Runtime configuration
    config: RuntimeConfig,
    /// Runtime statistics
    stats: Arc<RuntimeStats>,
    /// Runtime start time
    start_time: Instant,
    /// I/O thread pool
    io_threads: Vec<JoinHandle<()>>,
    /// I/O request channel
    io_sender: Sender<IORequest>,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
}

impl RustRoutineRuntime {
    /// Create a new runtime with default configuration
    pub fn new() -> Result<Self> {
        Self::with_config(RuntimeConfig::default())
    }
    
    /// Create a new runtime with custom configuration
    pub fn with_config(config: RuntimeConfig) -> Result<Self> {
        // Initialize the M:N scheduler (already done via GLOBAL_SCHEDULER lazy static)
        log::info!("Initializing RustRoutines runtime with {} CPU workers and {} I/O workers",
                  config.cpu_threads, config.io_threads);
        
        // Create I/O thread pool
        let (io_sender, io_receiver) = bounded::<IORequest>(1024);
        let io_receiver = Arc::new(Mutex::new(io_receiver));
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut io_threads = Vec::new();
        
        for i in 0..config.io_threads {
            let receiver = Arc::clone(&io_receiver);
            let shutdown_clone = Arc::clone(&shutdown);
            let thread_name = format!("{}-{}", config.io_thread_name, i);
            
            let handle = thread::Builder::new()
                .name(thread_name)
                .spawn(move || {
                    Self::io_worker_loop(receiver, shutdown_clone);
                })
                .map_err(|e| Error::RuntimeError {
                    reason: format!("Failed to spawn I/O thread: {}", e)
                })?;
                
            io_threads.push(handle);
        }
        
        Ok(Self {
            config,
            stats: Arc::new(RuntimeStats::default()),
            start_time: Instant::now(),
            io_threads,
            io_sender,
            shutdown,
        })
    }
    
    /// I/O worker thread main loop
    fn io_worker_loop(receiver: Arc<Mutex<Receiver<IORequest>>>, shutdown: Arc<AtomicBool>) {
        while !shutdown.load(Ordering::Relaxed) {
            // Try to get an I/O request
            let request = {
                let rx = receiver.lock();
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(req) => req,
                    Err(_) => continue, // Timeout or channel closed
                }
            };
            
            // Process the I/O request
            match request {
                IORequest::Read(path, sender) => {
                    let result = std::fs::read(&path)
                        .map_err(|e| Error::IOError {
                            reason: format!("Failed to read {}: {}", path, e)
                        });
                    let _ = sender.send(result);
                }
                IORequest::Write(path, data, sender) => {
                    let result = std::fs::write(&path, data)
                        .map_err(|e| Error::IOError {
                            reason: format!("Failed to write {}: {}", path, e)
                        });
                    let _ = sender.send(result);
                }
            }
        }
    }
    
    /// Block on I/O operation (migrates routine to I/O thread pool)
    pub fn block_on_io<F, T>(&self, f: F) -> T
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        self.stats.io_operations.fetch_add(1, Ordering::Relaxed);
        
        // For now, just execute directly
        // TODO: Implement proper routine migration to I/O pool
        f()
    }
    
    /// Read a file using the I/O thread pool
    pub fn read_file(&self, path: String) -> Result<Vec<u8>> {
        let (tx, rx) = bounded(1);
        self.io_sender.send(IORequest::Read(path, tx))
            .map_err(|_| Error::RuntimeError {
                reason: "I/O thread pool shut down".to_string()
            })?;
        
        self.stats.io_operations.fetch_add(1, Ordering::Relaxed);
        
        rx.recv()
            .map_err(|_| Error::RuntimeError {
                reason: "Failed to receive I/O response".to_string()
            })?
    }
    
    /// Write a file using the I/O thread pool  
    pub fn write_file(&self, path: String, data: Vec<u8>) -> Result<()> {
        let (tx, rx) = bounded(1);
        self.io_sender.send(IORequest::Write(path, data, tx))
            .map_err(|_| Error::RuntimeError {
                reason: "I/O thread pool shut down".to_string()
            })?;
            
        self.stats.io_operations.fetch_add(1, Ordering::Relaxed);
        
        rx.recv()
            .map_err(|_| Error::RuntimeError {
                reason: "Failed to receive I/O response".to_string()
            })?
    }
    
    /// Get runtime statistics
    pub fn stats(&self) -> RuntimeStats {
        RuntimeStats {
            routines_spawned: AtomicUsize::new(
                self.stats.routines_spawned.load(Ordering::Relaxed)
            ),
            routines_completed: AtomicUsize::new(
                self.stats.routines_completed.load(Ordering::Relaxed)
            ),
            routines_active: AtomicUsize::new(
                self.stats.routines_active.load(Ordering::Relaxed)
            ),
            io_operations: AtomicUsize::new(
                self.stats.io_operations.load(Ordering::Relaxed)
            ),
            timer_operations: AtomicUsize::new(
                self.stats.timer_operations.load(Ordering::Relaxed)
            ),
        }
    }
    
    /// Get scheduler statistics
    pub fn scheduler_stats(&self) -> SchedulerStats {
        scheduler_stats()
    }
    
    /// Get runtime uptime
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }
    
    /// Shutdown the runtime
    pub fn shutdown(self) {
        log::info!("Shutting down RustRoutines runtime");
        
        // Signal shutdown
        self.shutdown.store(true, Ordering::Relaxed);
        
        // Wait for I/O threads to finish
        for handle in self.io_threads {
            let _ = handle.join();
        }
        
        // Scheduler shutdown is handled by its Drop implementation
    }
}

/// Global runtime instance
pub static GLOBAL_RUNTIME: Lazy<Arc<RwLock<Option<RustRoutineRuntime>>>> = 
    Lazy::new(|| Arc::new(RwLock::new(None)));

/// Initialize the global runtime
pub fn init_global_runtime(config: RuntimeConfig) -> Result<()> {
    let mut runtime = GLOBAL_RUNTIME.write();
    if runtime.is_some() {
        return Err(Error::RuntimeError {
            reason: "Runtime already initialized".to_string()
        });
    }
    
    *runtime = Some(RustRoutineRuntime::with_config(config)?);
    Ok(())
}

/// Get reference to global runtime
pub fn with_runtime<F, R>(f: F) -> Result<R>
where
    F: FnOnce(&RustRoutineRuntime) -> R,
{
    let runtime = GLOBAL_RUNTIME.read();
    match runtime.as_ref() {
        Some(rt) => Ok(f(rt)),
        None => {
            // Auto-initialize with defaults if not initialized
            drop(runtime);
            init_global_runtime(RuntimeConfig::default())?;
            let runtime = GLOBAL_RUNTIME.read();
            Ok(f(runtime.as_ref().unwrap()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_runtime_creation() {
        let runtime = RustRoutineRuntime::new().unwrap();
        assert_eq!(runtime.stats.routines_spawned.load(Ordering::Relaxed), 0);
    }
    
    #[test]
    fn test_runtime_with_config() {
        let config = RuntimeConfig {
            cpu_threads: 4,
            io_threads: 8,
            ..Default::default()
        };
        
        let runtime = RustRoutineRuntime::with_config(config).unwrap();
        assert_eq!(runtime.config.cpu_threads, 4);
        assert_eq!(runtime.config.io_threads, 8);
    }
    
    #[test]
    fn test_global_runtime_init() {
        // Clear any existing runtime
        *GLOBAL_RUNTIME.write() = None;
        
        let config = RuntimeConfig::default();
        init_global_runtime(config).unwrap();
        
        with_runtime(|rt| {
            assert_eq!(rt.stats.io_operations.load(Ordering::Relaxed), 0);
        }).unwrap();
    }
}

//! I/O thread pool with routine migration support
//!
//! This module provides automatic detection of I/O operations and migrates
//! routines between CPU and I/O thread pools to prevent blocking.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use crossbeam::channel::{bounded, Sender};
use crossbeam::deque::{Worker, Stealer, Injector};
use once_cell::sync::Lazy;

/// Routine ID for tracking migrations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RoutineId(pub usize);

/// Type of operation being performed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    Cpu,
    Io,
    Unknown,
}

/// I/O operation wrapper that migrates routine to I/O pool
pub struct IoOperation {
    pub routine_id: RoutineId,
    pub operation: Box<dyn FnOnce() + Send + 'static>,
    pub result_sender: Sender<()>,
}

/// Thread pool specifically for I/O operations
pub struct IoThreadPool {
    /// Worker threads
    workers: Vec<JoinHandle<()>>,
    /// Number of threads in the pool
    num_threads: usize,
    /// Work queue for I/O operations
    work_queue: Arc<Injector<IoOperation>>,
    /// Stealers for work stealing
    stealers: Vec<Stealer<IoOperation>>,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
    /// Statistics
    stats: IoPoolStats,
}

/// Statistics for I/O thread pool
#[derive(Debug, Default)]
pub struct IoPoolStats {
    /// Number of I/O operations executed
    pub operations_executed: AtomicUsize,
    /// Number of routine migrations to I/O pool
    pub migrations_to_io: AtomicUsize,
    /// Number of routine migrations from I/O pool
    pub migrations_from_io: AtomicUsize,
    /// Current number of active I/O operations
    pub active_operations: AtomicUsize,
}

impl IoThreadPool {
    /// Create a new I/O thread pool
    pub fn new(num_threads: usize) -> Arc<Self> {
        let work_queue = Arc::new(Injector::new());
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut workers = Vec::new();
        let mut stealers = Vec::new();
        
        // Create worker threads
        for i in 0..num_threads {
            let worker: Worker<IoOperation> = Worker::new_fifo();
            let stealer = worker.stealer();
            stealers.push(stealer.clone());
            
            let queue = work_queue.clone();
            let shutdown_clone = shutdown.clone();
            let all_stealers = stealers.clone();
            
            let handle = thread::Builder::new()
                .name(format!("io-worker-{}", i))
                .spawn(move || {
                    Self::worker_loop(worker, queue, all_stealers, shutdown_clone);
                })
                .expect("Failed to spawn I/O worker thread");
                
            workers.push(handle);
        }
        
        Arc::new(IoThreadPool {
            workers,
            num_threads,
            work_queue,
            stealers,
            shutdown,
            stats: IoPoolStats::default(),
        })
    }
    
    /// Worker thread main loop
    fn worker_loop(
        worker: Worker<IoOperation>,
        global: Arc<Injector<IoOperation>>,
        stealers: Vec<Stealer<IoOperation>>,
        shutdown: Arc<AtomicBool>,
    ) {
        while !shutdown.load(Ordering::Relaxed) {
            // Try to get work from local queue first
            let task = worker.pop()
                .or_else(|| {
                    // Try to steal from global queue
                    loop {
                        match global.steal() {
                            crossbeam::deque::Steal::Success(t) => return Some(t),
                            crossbeam::deque::Steal::Empty => break,
                            crossbeam::deque::Steal::Retry => continue,
                        }
                    }
                    None
                })
                .or_else(|| {
                    // Try to steal from other workers
                    stealers.iter()
                        .map(|s| s.steal())
                        .find_map(|s| match s {
                            crossbeam::deque::Steal::Success(t) => Some(t),
                            _ => None,
                        })
                });
                
            if let Some(operation) = task {
                // Execute the I/O operation
                (operation.operation)();
                
                // Notify completion
                let _ = operation.result_sender.send(());
            } else {
                // No work available, park the thread
                thread::park_timeout(Duration::from_millis(1));
            }
        }
    }
    
    /// Submit an I/O operation to the pool
    pub fn submit(&self, operation: IoOperation) {
        self.stats.operations_executed.fetch_add(1, Ordering::Relaxed);
        self.stats.active_operations.fetch_add(1, Ordering::Relaxed);
        self.work_queue.push(operation);
        
        // Wake up a worker thread
        // Note: In a real implementation, we'd track parked threads
    }
    
    /// Migrate a routine to the I/O pool for an operation
    pub fn migrate_for_io<F, T>(&self, routine_id: RoutineId, f: F) -> T
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        self.stats.migrations_to_io.fetch_add(1, Ordering::Relaxed);
        
        let (tx, rx) = bounded(1);
        let (complete_tx, complete_rx) = bounded(0);
        
        // Wrap the operation to send result back
        let operation = IoOperation {
            routine_id,
            operation: Box::new(move || {
                let result = f();
                let _ = tx.send(result);
            }),
            result_sender: complete_tx,
        };
        
        // Submit to I/O pool
        self.submit(operation);
        
        // Wait for completion
        let _ = complete_rx.recv();
        
        self.stats.active_operations.fetch_sub(1, Ordering::Relaxed);
        self.stats.migrations_from_io.fetch_add(1, Ordering::Relaxed);
        
        // Get the result
        rx.recv().expect("I/O operation failed to return result")
    }
    
    /// Get statistics about the I/O pool
    pub fn stats(&self) -> &IoPoolStats {
        &self.stats
    }
    
    /// Shutdown the I/O thread pool
    pub fn shutdown(self) {
        self.shutdown.store(true, Ordering::Relaxed);
        
        // Unpark all threads
        for _ in 0..self.num_threads {
            self.work_queue.push(IoOperation {
                routine_id: RoutineId(0),
                operation: Box::new(|| {}),
                result_sender: bounded(0).0,
            });
        }
        
        // Wait for workers to finish
        for worker in self.workers {
            let _ = worker.join();
        }
    }
}

/// Global I/O thread pool
pub static GLOBAL_IO_POOL: Lazy<Arc<IoThreadPool>> = Lazy::new(|| {
    let num_threads = std::env::var("RUST_ROUTINES_IO_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(128); // Default to 128 like Go
        
    IoThreadPool::new(num_threads)
});

// Thread-local flag indicating if we're in an I/O context
thread_local! {
    static IN_IO_CONTEXT: AtomicBool = AtomicBool::new(false);
}

/// Check if current thread is in I/O context
pub fn in_io_context() -> bool {
    IN_IO_CONTEXT.with(|flag| flag.load(Ordering::Relaxed))
}

/// Set I/O context flag
pub fn set_io_context(value: bool) {
    IN_IO_CONTEXT.with(|flag| flag.store(value, Ordering::Relaxed));
}

/// Automatically detect and handle I/O operations
pub fn auto_io<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    if in_io_context() {
        // Already in I/O context, just execute
        f()
    } else {
        // Migrate to I/O pool - use a simple counter for routine ID
        static ROUTINE_COUNTER: AtomicUsize = AtomicUsize::new(0);
        let routine_id = RoutineId(ROUTINE_COUNTER.fetch_add(1, Ordering::Relaxed));
        
        GLOBAL_IO_POOL.migrate_for_io(routine_id, || {
            set_io_context(true);
            let result = f();
            set_io_context(false);
            result
        })
    }
}

/// Blocking I/O operation wrapper
pub fn block_on_io<F, T>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    auto_io(f)
}

/// File I/O operations
pub mod file {
    use super::*;
    use std::path::Path;
    use std::io;
    
    /// Read a file with automatic I/O pool migration
    pub fn read<P: AsRef<Path> + Send + 'static>(path: P) -> io::Result<Vec<u8>> {
        auto_io(move || std::fs::read(path))
    }
    
    /// Write a file with automatic I/O pool migration
    pub fn write<P, C>(path: P, contents: C) -> io::Result<()>
    where
        P: AsRef<Path> + Send + 'static,
        C: AsRef<[u8]> + Send + 'static,
    {
        auto_io(move || std::fs::write(path, contents))
    }
    
    /// Read to string with automatic I/O pool migration
    pub fn read_to_string<P: AsRef<Path> + Send + 'static>(path: P) -> io::Result<String> {
        auto_io(move || std::fs::read_to_string(path))
    }
}

/// Network I/O operations
pub mod net {
    use super::*;
    use std::net::{TcpListener, TcpStream, SocketAddr};
    use std::io;
    
    /// Connect to a TCP address with automatic I/O pool migration
    pub fn tcp_connect<A: Into<SocketAddr>>(addr: A) -> io::Result<TcpStream> {
        let addr = addr.into();
        auto_io(move || TcpStream::connect(addr))
    }
    
    /// Bind a TCP listener with automatic I/O pool migration
    pub fn tcp_bind<A: Into<SocketAddr>>(addr: A) -> io::Result<TcpListener> {
        let addr = addr.into();
        auto_io(move || TcpListener::bind(addr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_io_pool_creation() {
        let pool = IoThreadPool::new(4);
        assert_eq!(pool.num_threads, 4);
    }
    
    #[test]
    fn test_io_migration() {
        let pool = IoThreadPool::new(2);
        let routine_id = RoutineId(1);
        
        let result = pool.migrate_for_io(routine_id, || {
            // Simulate I/O operation
            std::thread::sleep(Duration::from_millis(10));
            42
        });
        
        assert_eq!(result, 42);
        assert_eq!(pool.stats.migrations_to_io.load(Ordering::Relaxed), 1);
        assert_eq!(pool.stats.migrations_from_io.load(Ordering::Relaxed), 1);
    }
    
    #[test]
    fn test_auto_io() {
        let result = auto_io(|| {
            // This should run in I/O pool
            assert!(in_io_context());
            "io result"
        });
        
        assert_eq!(result, "io result");
    }
    
    #[test]
    fn test_file_io() {
        use std::fs;
        
        // Create a temp file
        let path = "test_io_file.txt";
        let content = "test content";
        
        // Write with I/O pool
        file::write(path, content).unwrap();
        
        // Read with I/O pool
        let read_content = file::read_to_string(path).unwrap();
        assert_eq!(read_content, content);
        
        // Cleanup
        fs::remove_file(path).unwrap();
    }
}

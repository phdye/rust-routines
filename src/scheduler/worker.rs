//! Worker thread implementation for the M:N scheduler
//!
//! Each worker manages a pool of routines and executes them on an OS thread.

use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::collections::VecDeque;
use std::time::Duration;
use parking_lot::{Condvar, Mutex as ParkingMutex};
use crate::error::{Error, Result};
use super::queue::Task;

/// Unique identifier for a worker thread
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkerId(pub usize);

impl WorkerId {
    /// Get the numeric ID
    pub fn as_usize(&self) -> usize {
        self.0
    }
}

/// Worker thread state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    /// Worker is running and processing tasks
    Running,
    /// Worker is parked (idle, waiting for work)
    Parked,
    /// Worker is shutting down
    Stopping,
    /// Worker has stopped
    Stopped,
}

/// Statistics for a worker thread
#[derive(Debug, Default)]
pub struct WorkerStats {
    /// Number of tasks executed
    pub tasks_executed: AtomicUsize,
    /// Number of tasks stolen from other workers
    pub tasks_stolen: AtomicUsize,
    /// Number of times the worker went to sleep
    pub park_count: AtomicUsize,
    /// Number of times the worker woke up
    pub unpark_count: AtomicUsize,
}

/// Worker thread configuration
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    /// Worker ID
    pub id: WorkerId,
    /// Thread name
    pub name: String,
    /// CPU affinity (optional)
    pub cpu_affinity: Option<usize>,
    /// Maximum number of tasks to steal at once
    pub steal_batch_size: usize,
    /// Park timeout duration when idle
    pub park_timeout: Duration,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            id: WorkerId(0),
            name: "rust-routine-worker-0".to_string(),
            cpu_affinity: None,
            steal_batch_size: 32,
            park_timeout: Duration::from_millis(1),
        }
    }
}

/// Worker thread that executes tasks
pub struct Worker {
    /// Worker configuration
    config: WorkerConfig,
    /// Current state
    state: Arc<AtomicUsize>,
    /// Local run queue (LIFO for better cache locality)
    local_queue: Arc<ParkingMutex<VecDeque<Task>>>,
    /// Condition variable for parking/unparking
    park_condvar: Arc<Condvar>,
    /// Statistics
    stats: Arc<WorkerStats>,
    /// Shutdown flag
    should_stop: Arc<AtomicBool>,
    /// Thread handle
    thread_handle: Option<JoinHandle<()>>,
}

impl Worker {
    /// Create a new worker with the given configuration
    pub fn new(config: WorkerConfig) -> Self {
        Self {
            config,
            state: Arc::new(AtomicUsize::new(WorkerState::Parked as usize)),
            local_queue: Arc::new(ParkingMutex::new(VecDeque::new())),
            park_condvar: Arc::new(Condvar::new()),
            stats: Arc::new(WorkerStats::default()),
            should_stop: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        }
    }
    
    /// Start the worker thread
    pub fn start(&mut self, scheduler: Arc<super::scheduler::Scheduler>) -> Result<()> {
        if self.thread_handle.is_some() {
            return Err(Error::RuntimeError {
                reason: "Worker already started".to_string(),
            });
        }
        
        let config = self.config.clone();
        let state = Arc::clone(&self.state);
        let local_queue = Arc::clone(&self.local_queue);
        let park_condvar = Arc::clone(&self.park_condvar);
        let stats = Arc::clone(&self.stats);
        let should_stop = Arc::clone(&self.should_stop);
        
        let handle = thread::Builder::new()
            .name(config.name.clone())
            .spawn(move || {
                // Set CPU affinity if configured
                #[cfg(target_os = "linux")]
                if let Some(cpu) = config.cpu_affinity {
                    set_cpu_affinity(cpu);
                }
                
                // Main worker loop
                worker_loop(
                    config,
                    state,
                    local_queue,
                    park_condvar,
                    stats,
                    should_stop,
                    scheduler,
                );
            })
            .map_err(|e| Error::RuntimeError {
                reason: format!("Failed to spawn worker thread: {}", e),
            })?;
        
        self.thread_handle = Some(handle);
        self.state.store(WorkerState::Running as usize, Ordering::Release);
        
        Ok(())
    }
    
    /// Stop the worker thread
    pub fn stop(&mut self) -> Result<()> {
        self.should_stop.store(true, Ordering::Release);
        self.unpark();
        
        if let Some(handle) = self.thread_handle.take() {
            handle.join().map_err(|_| Error::RuntimeError {
                reason: "Worker thread panicked".to_string(),
            })?;
        }
        
        self.state.store(WorkerState::Stopped as usize, Ordering::Release);
        Ok(())
    }
    
    /// Push a task to the worker's local queue
    pub fn push_task(&self, task: Task) {
        let mut queue = self.local_queue.lock();
        queue.push_back(task);
        drop(queue);
        
        // Wake up the worker if it's parked
        if self.get_state() == WorkerState::Parked {
            self.unpark();
        }
    }
    
    /// Try to steal tasks from this worker's queue
    pub fn steal_tasks(&self, max_steal: usize) -> Vec<Task> {
        let mut queue = self.local_queue.lock();
        let mut stolen = Vec::new();
        
        // Steal from the front (FIFO) while worker processes from back (LIFO)
        let steal_count = (queue.len() / 2).min(max_steal);
        for _ in 0..steal_count {
            if let Some(task) = queue.pop_front() {
                stolen.push(task);
            }
        }
        
        stolen
    }
    
    /// Park the worker (put it to sleep)
    pub fn park(&self) {
        self.state.store(WorkerState::Parked as usize, Ordering::Release);
        self.stats.park_count.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Unpark the worker (wake it up)
    pub fn unpark(&self) {
        self.state.store(WorkerState::Running as usize, Ordering::Release);
        self.stats.unpark_count.fetch_add(1, Ordering::Relaxed);
        self.park_condvar.notify_one();
    }
    
    /// Get the current state of the worker
    pub fn get_state(&self) -> WorkerState {
        match self.state.load(Ordering::Acquire) {
            0 => WorkerState::Running,
            1 => WorkerState::Parked,
            2 => WorkerState::Stopping,
            3 => WorkerState::Stopped,
            _ => WorkerState::Stopped,
        }
    }
    
    /// Get worker ID
    pub fn id(&self) -> WorkerId {
        self.config.id
    }
    
    /// Get worker statistics
    pub fn stats(&self) -> WorkerStats {
        WorkerStats {
            tasks_executed: AtomicUsize::new(self.stats.tasks_executed.load(Ordering::Relaxed)),
            tasks_stolen: AtomicUsize::new(self.stats.tasks_stolen.load(Ordering::Relaxed)),
            park_count: AtomicUsize::new(self.stats.park_count.load(Ordering::Relaxed)),
            unpark_count: AtomicUsize::new(self.stats.unpark_count.load(Ordering::Relaxed)),
        }
    }
    
    /// Check if the worker has tasks in its queue
    pub fn has_tasks(&self) -> bool {
        !self.local_queue.lock().is_empty()
    }
}

/// Main worker loop that processes tasks
fn worker_loop(
    config: WorkerConfig,
    state: Arc<AtomicUsize>,
    local_queue: Arc<ParkingMutex<VecDeque<Task>>>,
    park_condvar: Arc<Condvar>,
    stats: Arc<WorkerStats>,
    should_stop: Arc<AtomicBool>,
    scheduler: Arc<super::scheduler::Scheduler>,
) {
    let mut consecutive_steals = 0;
    
    while !should_stop.load(Ordering::Acquire) {
        // Try to get a task from the local queue (LIFO for cache locality)
        let task = {
            let mut queue = local_queue.lock();
            queue.pop_back()
        };
        
        if let Some(task) = task {
            // Execute the task
            task.run();
            stats.tasks_executed.fetch_add(1, Ordering::Relaxed);
            consecutive_steals = 0;
        } else {
            // No local work, try to steal from other workers
            if let Some(stolen_tasks) = scheduler.steal_work(config.id) {
                let stolen_count = stolen_tasks.len();
                if stolen_count > 0 {
                    let mut queue = local_queue.lock();
                    for task in stolen_tasks {
                        queue.push_back(task);
                    }
                    stats.tasks_stolen.fetch_add(stolen_count, Ordering::Relaxed);
                    consecutive_steals = 0;
                    continue;
                }
            }
            
            // No work available, consider parking
            consecutive_steals += 1;
            
            if consecutive_steals > 3 {
                // Park the worker after multiple failed steal attempts
                state.store(WorkerState::Parked as usize, Ordering::Release);
                stats.park_count.fetch_add(1, Ordering::Relaxed);
                
                let mut queue = local_queue.lock();
                if queue.is_empty() && !should_stop.load(Ordering::Acquire) {
                    // Wait for new work or timeout
                    let _timeout = park_condvar.wait_for(
                        &mut queue,
                        config.park_timeout,
                    );
                }
                
                state.store(WorkerState::Running as usize, Ordering::Release);
                stats.unpark_count.fetch_add(1, Ordering::Relaxed);
                consecutive_steals = 0;
            } else {
                // Yield to avoid busy spinning
                thread::yield_now();
            }
        }
    }
    
    state.store(WorkerState::Stopped as usize, Ordering::Release);
}

/// Set CPU affinity for the current thread (Linux only)
#[cfg(target_os = "linux")]
fn set_cpu_affinity(cpu: usize) {
    use libc::{cpu_set_t, CPU_SET, CPU_ZERO, sched_setaffinity};
    use std::mem;
    
    unsafe {
        let mut set: cpu_set_t = mem::zeroed();
        CPU_ZERO(&mut set);
        CPU_SET(cpu, &mut set);
        sched_setaffinity(0, mem::size_of::<cpu_set_t>(), &set);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_worker_creation() {
        let config = WorkerConfig {
            id: WorkerId(1),
            name: "test-worker".to_string(),
            ..Default::default()
        };
        
        let worker = Worker::new(config);
        assert_eq!(worker.id(), WorkerId(1));
        assert_eq!(worker.get_state(), WorkerState::Parked);
    }
    
    #[test]
    fn test_worker_state_transitions() {
        let worker = Worker::new(WorkerConfig::default());
        
        assert_eq!(worker.get_state(), WorkerState::Parked);
        
        worker.unpark();
        assert_eq!(worker.get_state(), WorkerState::Running);
        
        worker.park();
        assert_eq!(worker.get_state(), WorkerState::Parked);
    }
    
    #[test]
    fn test_task_queue_operations() {
        let worker = Worker::new(WorkerConfig::default());
        
        // Push multiple tasks to allow stealing
        for _ in 0..4 {
            let task = Task::new(Box::new(|| {
                // Task logic
            }));
            worker.push_task(task);
        }
        
        assert!(worker.has_tasks());
        
        // Steal tasks - should steal half (2 out of 4)
        let stolen = worker.steal_tasks(10);
        assert_eq!(stolen.len(), 2);
        assert!(worker.has_tasks()); // Should still have 2 tasks left
    }
}

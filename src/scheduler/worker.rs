//! Worker thread implementation for the M:N scheduler
//!
//! Each worker manages a pool of routines and executes them on an OS thread.

use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::sync::{Arc, Weak};
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
    /// Local run queue for compatibility (LIFO for better cache locality)
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
    
    /// Get the stealer for this worker's queue
    /// Note: This is a placeholder - real stealer will be created in the worker thread
    pub fn get_stealer(&self) -> super::queue::Stealer<Task> {
        // We'll return a dummy stealer for now - the real implementation would
        // need to handle this differently
        let (_, stealer) = super::queue::WorkQueue::new();
        stealer
    }
    
    /// Start the worker thread
    pub fn start(&mut self, scheduler: Weak<super::core::Scheduler>) -> Result<()> {
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
    
    /// Push a task to the worker's queue
    /// Note: Tasks are pushed to the local queue since the work queue is owned by the worker thread
    pub fn push_task(&self, task: Task) {
        let mut queue = self.local_queue.lock();
        queue.push_back(task);
        drop(queue);
        
        // Wake up the worker if it's parked
        if self.get_state() == WorkerState::Parked {
            self.unpark();
        }
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
    /// Note: This method is no longer used with the new work queue design
    /// but kept for compatibility
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
    scheduler: Weak<super::core::Scheduler>,
) {
    // Create this worker's own work queue
    let (work_queue, stealer) = super::queue::WorkQueue::new();
    
    // Register the stealer with the scheduler
    if let Some(scheduler_ref) = scheduler.upgrade() {
        scheduler_ref.register_stealer(config.id, stealer);
    }
    
    let mut consecutive_steals = 0;
    
    while !should_stop.load(Ordering::Acquire) {
        // Try to get a task from the work queue first
        let task = work_queue.pop().or_else(|| {
            // Fall back to local queue
            let mut queue = local_queue.lock();
            queue.pop_back()
        });
        
        if let Some(task) = task {
            // Execute the task
            task.run();
            stats.tasks_executed.fetch_add(1, Ordering::Relaxed);
            consecutive_steals = 0;
        } else {
            // No local work, try to steal from other workers or global queue
            let stolen_work = if let Some(scheduler) = scheduler.upgrade() {
                scheduler.steal_work(config.id)
            } else {
                // Scheduler has been dropped, exit
                break;
            };
            
            if let Some(stolen_tasks) = stolen_work {
                let stolen_count = stolen_tasks.len();
                if stolen_count > 0 {
                    // Add stolen tasks to work queue
                    for task in stolen_tasks {
                        work_queue.push(task);
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
                if queue.is_empty() && work_queue.is_empty() && !should_stop.load(Ordering::Acquire) {
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
        
        // Push multiple tasks to test the queue
        for _ in 0..4 {
            let task = Task::new(Box::new(|| {
                // Task logic
            }));
            worker.push_task(task);
        }
        
        assert!(worker.has_tasks());
        
        // Note: steal_tasks method removed in new design
        // Stealing now happens through work-stealing deques in worker threads
    }
}

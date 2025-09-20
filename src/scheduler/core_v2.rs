//! Scheduler coordination without global state
//!
//! Manages worker threads and distributes tasks across them.
//! Each scheduler instance is independent and isolated.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::collections::HashMap;
use parking_lot::RwLock;
use crate::error::{Error, Result};
use super::worker::{Worker, WorkerId, WorkerConfig};
use super::queue::{Task, GlobalQueue, Stealer};
use super::steal::{WorkStealer, StealStrategy, SchedulingPolicy, LoadBalancer};

/// Scheduler configuration
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Number of worker threads (0 = number of CPU cores)
    pub num_workers: usize,
    /// Thread name prefix
    pub thread_name_prefix: String,
    /// Enable CPU affinity
    pub enable_cpu_affinity: bool,
    /// Maximum tasks to steal at once
    pub steal_batch_size: usize,
    /// Park timeout in milliseconds
    pub park_timeout_ms: u64,
    /// Enable NUMA awareness
    pub numa_aware: bool,
    /// Work-stealing strategy
    pub steal_strategy: StealStrategy,
    /// Scheduling policy
    pub scheduling_policy: SchedulingPolicy,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        let num_cpus = num_cpus::get();
        
        Self {
            num_workers: num_cpus,
            thread_name_prefix: "rust-routine-worker".to_string(),
            enable_cpu_affinity: cfg!(target_os = "linux"),
            steal_batch_size: 32,
            park_timeout_ms: 1,
            numa_aware: false,
            steal_strategy: StealStrategy::Random,
            scheduling_policy: SchedulingPolicy::LIFO,
        }
    }
}

/// Scheduler statistics - now properly cloneable for reporting
#[derive(Debug, Clone)]
pub struct SchedulerStats {
    /// Total tasks scheduled
    pub tasks_scheduled: usize,
    /// Total tasks completed
    pub tasks_completed: usize,
    /// Number of active workers
    pub active_workers: usize,
    /// Number of parked workers
    pub parked_workers: usize,
}

/// Internal scheduler statistics using atomics
struct SchedulerStatsInternal {
    /// Total tasks scheduled
    pub tasks_scheduled: AtomicUsize,
    /// Total tasks completed
    pub tasks_completed: AtomicUsize,
    /// Number of active workers
    pub active_workers: AtomicUsize,
    /// Number of parked workers
    pub parked_workers: AtomicUsize,
}

impl Default for SchedulerStatsInternal {
    fn default() -> Self {
        Self {
            tasks_scheduled: AtomicUsize::new(0),
            tasks_completed: AtomicUsize::new(0),
            active_workers: AtomicUsize::new(0),
            parked_workers: AtomicUsize::new(0),
        }
    }
}

/// The scheduler that coordinates worker threads
/// Now properly isolated - no global state!
pub struct Scheduler {
    /// Scheduler configuration
    config: SchedulerConfig,
    /// Worker threads
    workers: RwLock<HashMap<WorkerId, Arc<Worker>>>,
    /// Stealers for each worker's queue (thread-safe)
    stealers: RwLock<HashMap<WorkerId, Stealer<Task>>>,
    /// Global task queue
    global_queue: GlobalQueue,
    /// Work stealer for load balancing
    work_stealer: RwLock<WorkStealer>,
    /// Load balancer for steal decisions
    #[allow(dead_code)]
    load_balancer: LoadBalancer,
    /// Scheduler statistics
    stats: Arc<SchedulerStatsInternal>,
    /// Shutdown flag
    shutdown: AtomicBool,
    /// Round-robin counter for task distribution
    round_robin: AtomicUsize,
}

impl Scheduler {
    /// Create a new scheduler with the given configuration
    /// Each scheduler is completely independent!
    pub fn new(config: SchedulerConfig) -> Result<Arc<Self>> {
        let num_workers = if config.num_workers == 0 {
            num_cpus::get()
        } else {
            config.num_workers
        };
        
        let scheduler = Arc::new(Scheduler {
            config: config.clone(),
            workers: RwLock::new(HashMap::new()),
            stealers: RwLock::new(HashMap::new()),
            global_queue: GlobalQueue::new(),
            work_stealer: RwLock::new(WorkStealer::new(
                config.steal_strategy,
                config.scheduling_policy,
            )),
            load_balancer: LoadBalancer::default(),
            stats: Arc::new(SchedulerStatsInternal::default()),
            shutdown: AtomicBool::new(false),
            round_robin: AtomicUsize::new(0),
        });
        
        // Create workers
        let mut workers = HashMap::new();
        let mut stealers = HashMap::new();
        
        for i in 0..num_workers {
            let worker_id = WorkerId(i);
            
            // Create worker configuration
            let worker_config = WorkerConfig {
                id: worker_id,
                name: format!("{}-{}", config.thread_name_prefix, i),
                cpu_affinity: if config.enable_cpu_affinity {
                    Some(i % num_cpus::get())
                } else {
                    None
                },
                steal_batch_size: config.steal_batch_size,
                park_timeout: std::time::Duration::from_millis(config.park_timeout_ms),
            };
            
            // Create worker
            let mut worker = Worker::new(worker_config);
            
            // Get the stealer before starting
            let stealer = worker.stealer();
            stealers.insert(worker_id, stealer);
            
            // Start the worker with a reference to this scheduler
            let scheduler_weak = Arc::downgrade(&scheduler);
            worker.start(scheduler_weak)?;
            
            workers.insert(worker_id, Arc::new(worker));
        }
        
        // Store workers and stealers
        *scheduler.workers.write() = workers;
        *scheduler.stealers.write() = stealers;
        
        // Update active worker count
        scheduler.stats.active_workers.store(num_workers, Ordering::Relaxed);
        
        log::info!("Scheduler created with {} workers", num_workers);
        
        Ok(scheduler)
    }
    
    /// Schedule a task for execution
    pub fn schedule(&self, task: Task) -> Result<()> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(Error::SchedulerShutdown);
        }
        
        // Update statistics
        self.stats.tasks_scheduled.fetch_add(1, Ordering::Relaxed);
        
        // Try to schedule to a worker using round-robin
        let workers = self.workers.read();
        if !workers.is_empty() {
            let worker_count = workers.len();
            let start_idx = self.round_robin.fetch_add(1, Ordering::Relaxed) % worker_count;
            
            // Try to find a non-busy worker starting from round-robin position
            for i in 0..worker_count {
                let idx = (start_idx + i) % worker_count;
                let worker_id = WorkerId(idx);
                
                if let Some(worker) = workers.get(&worker_id) {
                    if worker.try_schedule(task.clone()) {
                        return Ok(());
                    }
                }
            }
        }
        
        // Fall back to global queue
        self.global_queue.push(task);
        
        // Wake up a parked worker if available
        self.wake_parked_worker();
        
        Ok(())
    }
    
    /// Wake up a parked worker if available
    fn wake_parked_worker(&self) {
        let workers = self.workers.read();
        for worker in workers.values() {
            if worker.is_parked() {
                worker.wake();
                break;
            }
        }
    }
    
    /// Get a reference to the statistics
    pub fn stats(&self) -> SchedulerStats {
        SchedulerStats {
            tasks_scheduled: self.stats.tasks_scheduled.load(Ordering::Relaxed),
            tasks_completed: self.stats.tasks_completed.load(Ordering::Relaxed),
            active_workers: self.stats.active_workers.load(Ordering::Relaxed),
            parked_workers: self.stats.parked_workers.load(Ordering::Relaxed),
        }
    }
    
    /// Get the number of workers
    pub fn num_workers(&self) -> usize {
        self.workers.read().len()
    }
    
    /// Shutdown the scheduler
    pub fn shutdown(&self) -> Result<()> {
        self.shutdown_timeout(std::time::Duration::from_secs(30))
    }
    
    /// Shutdown the scheduler with a timeout
    pub fn shutdown_timeout(&self, timeout: std::time::Duration) -> Result<()> {
        // Set shutdown flag
        self.shutdown.store(true, Ordering::Release);
        
        // Stop all workers
        let workers = self.workers.read();
        for worker in workers.values() {
            worker.stop();
        }
        
        // Wait for workers to finish with timeout
        let start = std::time::Instant::now();
        loop {
            let all_stopped = workers.values().all(|w| !w.is_running());
            if all_stopped {
                break;
            }
            
            if start.elapsed() > timeout {
                return Err(Error::RuntimeError {
                    reason: "Scheduler shutdown timed out".to_string(),
                });
            }
            
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        
        log::info!("Scheduler shut down successfully");
        Ok(())
    }
}

// NO GLOBAL SCHEDULER! Each runtime creates its own.

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_scheduler_creation() {
        let config = SchedulerConfig::default();
        let scheduler = Scheduler::new(config).unwrap();
        assert!(scheduler.num_workers() > 0);
    }
    
    #[test]
    fn test_independent_schedulers() {
        // Create two independent schedulers
        let scheduler1 = Scheduler::new(SchedulerConfig::default()).unwrap();
        let scheduler2 = Scheduler::new(SchedulerConfig::default()).unwrap();
        
        // Schedule tasks on scheduler1
        for _ in 0..10 {
            scheduler1.schedule(Task::dummy()).unwrap();
        }
        
        // Schedule tasks on scheduler2
        for _ in 0..5 {
            scheduler2.schedule(Task::dummy()).unwrap();
        }
        
        // Check that statistics are independent
        let stats1 = scheduler1.stats();
        let stats2 = scheduler2.stats();
        
        assert_eq!(stats1.tasks_scheduled, 10);
        assert_eq!(stats2.tasks_scheduled, 5);
        
        // Shutting down one doesn't affect the other
        scheduler1.shutdown().unwrap();
        
        // scheduler2 can still schedule tasks
        scheduler2.schedule(Task::dummy()).unwrap();
        let stats2_after = scheduler2.stats();
        assert_eq!(stats2_after.tasks_scheduled, 6);
        
        scheduler2.shutdown().unwrap();
    }
}

//! Global scheduler coordination
//!
//! Manages worker threads and distributes tasks across them.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::collections::HashMap;
use parking_lot::RwLock;
use once_cell::sync::Lazy;
use crate::error::{Error, Result};
use super::worker::{Worker, WorkerId, WorkerConfig};
use super::queue::{Task, GlobalQueue, Stealer};
use super::steal::{WorkStealer, StealStrategy, SchedulingPolicy, LoadBalancer};

/// Global scheduler instance
pub static GLOBAL_SCHEDULER: Lazy<Arc<Scheduler>> = Lazy::new(|| {
    Scheduler::new(SchedulerConfig::default()).expect("Failed to create global scheduler")
});

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

/// Scheduler statistics
#[derive(Debug, Default)]
pub struct SchedulerStats {
    /// Total tasks scheduled
    pub tasks_scheduled: AtomicUsize,
    /// Total tasks completed
    pub tasks_completed: AtomicUsize,
    /// Number of active workers
    pub active_workers: AtomicUsize,
    /// Number of parked workers
    pub parked_workers: AtomicUsize,
}

impl Clone for SchedulerStats {
    fn clone(&self) -> Self {
        SchedulerStats {
            tasks_scheduled: AtomicUsize::new(self.tasks_scheduled.load(Ordering::Relaxed)),
            tasks_completed: AtomicUsize::new(self.tasks_completed.load(Ordering::Relaxed)),
            active_workers: AtomicUsize::new(self.active_workers.load(Ordering::Relaxed)),
            parked_workers: AtomicUsize::new(self.parked_workers.load(Ordering::Relaxed)),
        }
    }
}

/// The main scheduler that coordinates worker threads
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
    /// Reserved for future advanced load balancing algorithms
    #[allow(dead_code)]
    load_balancer: LoadBalancer,
    /// Scheduler statistics
    stats: Arc<SchedulerStats>,
    /// Shutdown flag
    shutdown: AtomicBool,
    /// Round-robin counter for task distribution
    round_robin: AtomicUsize,
}

impl Scheduler {
    /// Create a new scheduler with the given configuration
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
            stats: Arc::new(SchedulerStats::default()),
            shutdown: AtomicBool::new(false),
            round_robin: AtomicUsize::new(0),
        });
        
        // Create workers
        let mut workers = HashMap::new();
        
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
            
            // Start the worker with a reference to this scheduler
            let scheduler_weak = Arc::downgrade(&scheduler);
            worker.start(scheduler_weak)?;
            
            workers.insert(worker_id, Arc::new(worker));
        }
        
        // Store workers
        *scheduler.workers.write() = workers;
        
        Ok(scheduler)
    }
    
    /// Register a stealer for a worker (called by the worker when it starts)
    pub fn register_stealer(&self, worker_id: WorkerId, stealer: Stealer<Task>) {
        self.stealers.write().insert(worker_id, stealer);
    }
    
    /// Schedule a task for execution
    pub fn schedule(&self, task: Task) -> Result<()> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(Error::RuntimeError {
                reason: "Scheduler is shutting down".to_string(),
            });
        }
        
        self.stats.tasks_scheduled.fetch_add(1, Ordering::Relaxed);
        
        // Try to find a worker with available capacity using round-robin
        let workers = self.workers.read();
        let num_workers = workers.len();
        
        if num_workers > 0 {
            let start_index = self.round_robin.fetch_add(1, Ordering::Relaxed) % num_workers;
            
            // Try each worker starting from the round-robin position
            for offset in 0..num_workers {
                let index = (start_index + offset) % num_workers;
                let worker_id = WorkerId(index);
                
                if let Some(worker) = workers.get(&worker_id) {
                    worker.push_task(task);
                    return Ok(());
                }
            }
        }
        
        // Fallback to global queue if no workers available
        self.global_queue.push(task);
        Ok(())
    }
    
    /// Try to steal work for a worker
    pub fn steal_work(&self, worker_id: WorkerId) -> Option<Vec<Task>> {
        // First try to get work from the global queue
        if !self.global_queue.is_empty() {
            let tasks = self.global_queue.pop_batch(self.config.steal_batch_size);
            if !tasks.is_empty() {
                return Some(tasks);
            }
        }
        
        // Use the work stealer to steal from other workers
        let stealers = self.stealers.read();
        let victim_stealers: Vec<(WorkerId, Stealer<Task>)> = stealers
            .iter()
            .filter(|(id, _)| **id != worker_id)
            .map(|(id, stealer)| (*id, stealer.clone()))
            .collect();
        
        drop(stealers);
        
        let work_stealer = self.work_stealer.read();
        work_stealer.steal_work_from_stealers(worker_id, &victim_stealers, self.config.steal_batch_size)
    }
    
    /// Shutdown the scheduler
    pub fn shutdown(&self) -> Result<()> {
        self.shutdown.store(true, Ordering::Release);
        
        // Stop all workers
        let mut workers = self.workers.write();
        for worker in workers.values_mut() {
            if let Ok(_mut_worker) = Arc::try_unwrap(Arc::clone(worker)) {
                // This won't work because we have multiple references, but we can signal shutdown
            }
            // Wake up any parked workers so they can see the shutdown signal
            worker.unpark();
        }
        
        // For now, we don't have a clean way to stop individual workers
        // This is a limitation we should address in a future iteration
        
        Ok(())
    }
    
    /// Get scheduler statistics
    pub fn stats(&self) -> SchedulerStats {
        let workers = self.workers.read();
        
        let mut active = 0;
        let mut parked = 0;
        
        for worker in workers.values() {
            match worker.get_state() {
                super::worker::WorkerState::Running => active += 1,
                super::worker::WorkerState::Parked => parked += 1,
                _ => {}
            }
        }
        
        SchedulerStats {
            tasks_scheduled: AtomicUsize::new(
                self.stats.tasks_scheduled.load(Ordering::Relaxed)
            ),
            tasks_completed: AtomicUsize::new(
                self.stats.tasks_completed.load(Ordering::Relaxed)
            ),
            active_workers: AtomicUsize::new(active),
            parked_workers: AtomicUsize::new(parked),
        }
    }
    
    /// Get the number of workers
    pub fn num_workers(&self) -> usize {
        self.workers.read().len()
    }
    
    /// Check if the scheduler is shutting down
    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }
}

/// Initialize the global scheduler with custom configuration
pub fn init_scheduler(config: SchedulerConfig) -> Result<()> {
    // This would replace the global scheduler
    // For now, just validate the config
    let _ = Scheduler::new(config)?;
    Ok(())
}

/// Schedule a task on the global scheduler
pub fn schedule_task(task: Task) -> Result<()> {
    GLOBAL_SCHEDULER.schedule(task)
}

/// Get global scheduler statistics
pub fn scheduler_stats() -> SchedulerStats {
    GLOBAL_SCHEDULER.stats()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;
    
    #[test]
    fn test_scheduler_creation() {
        let config = SchedulerConfig {
            num_workers: 2,
            ..Default::default()
        };
        
        let scheduler = Scheduler::new(config).unwrap();
        assert_eq!(scheduler.num_workers(), 2);
        assert!(!scheduler.is_shutting_down());
    }
    
    #[test]
    fn test_task_scheduling() {
        let config = SchedulerConfig {
            num_workers: 2,
            ..Default::default()
        };
        
        let scheduler = Scheduler::new(config).unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        
        for _ in 0..10 {
            let counter_clone = Arc::clone(&counter);
            let task = Task::new(Box::new(move || {
                counter_clone.fetch_add(1, Ordering::Relaxed);
            }));
            
            scheduler.schedule(task).unwrap();
        }
        
        // Give some time for tasks to execute
        std::thread::sleep(std::time::Duration::from_millis(100));
        
        let stats = scheduler.stats();
        assert_eq!(stats.tasks_scheduled.load(Ordering::Relaxed), 10);
    }
    
    #[test]
    fn test_scheduler_shutdown() {
        let config = SchedulerConfig {
            num_workers: 1,
            ..Default::default()
        };
        
        let scheduler = Scheduler::new(config).unwrap();
        assert!(!scheduler.is_shutting_down());
        
        scheduler.shutdown().unwrap();
        assert!(scheduler.is_shutting_down());
        
        // Scheduling should fail after shutdown
        let task = Task::new(Box::new(|| {}));
        assert!(scheduler.schedule(task).is_err());
    }
}

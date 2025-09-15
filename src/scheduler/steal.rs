//! Work-stealing algorithm implementation
//!
//! This module provides advanced work-stealing strategies for load balancing.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use rand::{thread_rng, Rng};
use super::worker::{Worker, WorkerId};
use super::queue::Task;

/// Work-stealing strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StealStrategy {
    /// Random victim selection (default)
    Random,
    /// Round-robin victim selection
    RoundRobin,
    /// Steal from the busiest worker
    MostLoaded,
    /// Steal from nearest neighbor (for NUMA)
    Nearest,
}

/// Scheduling policy for task execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulingPolicy {
    /// Last-In-First-Out (better cache locality)
    LIFO,
    /// First-In-First-Out (fairness)
    FIFO,
    /// Priority-based scheduling
    Priority,
}

/// Work-stealing statistics
#[derive(Debug, Default)]
pub struct StealStats {
    /// Total steal attempts
    pub steal_attempts: AtomicUsize,
    /// Successful steals
    pub successful_steals: AtomicUsize,
    /// Failed steals (empty victim)
    pub failed_steals: AtomicUsize,
    /// Retry count due to contention
    pub retry_count: AtomicUsize,
}

/// Work-stealing coordinator
pub struct WorkStealer {
    /// Stealing strategy
    strategy: StealStrategy,
    /// Scheduling policy
    policy: SchedulingPolicy,
    /// Round-robin counter
    round_robin: AtomicUsize,
    /// Statistics
    stats: Arc<StealStats>,
}

impl WorkStealer {
    /// Create a new work stealer with the given strategy
    pub fn new(strategy: StealStrategy, policy: SchedulingPolicy) -> Self {
        Self {
            strategy,
            policy,
            round_robin: AtomicUsize::new(0),
            stats: Arc::new(StealStats::default()),
        }
    }
    
    /// Attempt to steal work for a thief worker
    pub fn steal_work(
        &self,
        thief_id: WorkerId,
        workers: &[Arc<Worker>],
        max_steal: usize,
    ) -> Option<Vec<Task>> {
        self.stats.steal_attempts.fetch_add(1, Ordering::Relaxed);
        
        let victims = self.select_victims(thief_id, workers);
        
        for victim_id in victims {
            if let Some(victim) = workers.iter().find(|w| w.id() == victim_id) {
                let stolen = victim.steal_tasks(max_steal);
                
                if !stolen.is_empty() {
                    self.stats.successful_steals.fetch_add(1, Ordering::Relaxed);
                    return Some(stolen);
                }
            }
        }
        
        self.stats.failed_steals.fetch_add(1, Ordering::Relaxed);
        None
    }
    
    /// Select victim workers based on the stealing strategy
    fn select_victims(&self, thief_id: WorkerId, workers: &[Arc<Worker>]) -> Vec<WorkerId> {
        let mut victims = Vec::new();
        let num_workers = workers.len();
        
        if num_workers <= 1 {
            return victims;
        }
        
        match self.strategy {
            StealStrategy::Random => {
                // Random shuffling for better load distribution
                let mut indices: Vec<usize> = (0..num_workers)
                    .filter(|&i| WorkerId(i) != thief_id)
                    .collect();
                
                let mut rng = thread_rng();
                for i in (1..indices.len()).rev() {
                    let j = rng.gen_range(0..=i);
                    indices.swap(i, j);
                }
                
                victims = indices.into_iter().map(WorkerId).collect();
            }
            
            StealStrategy::RoundRobin => {
                // Deterministic round-robin selection
                let start = self.round_robin.fetch_add(1, Ordering::Relaxed) % num_workers;
                
                for offset in 0..num_workers {
                    let idx = (start + offset) % num_workers;
                    let victim_id = WorkerId(idx);
                    
                    if victim_id != thief_id {
                        victims.push(victim_id);
                    }
                }
            }
            
            StealStrategy::MostLoaded => {
                // Sort workers by load (number of tasks)
                let mut worker_loads: Vec<(WorkerId, usize)> = workers
                    .iter()
                    .filter(|w| w.id() != thief_id)
                    .map(|w| (w.id(), if w.has_tasks() { 1 } else { 0 }))
                    .collect();
                
                // Sort by load in descending order
                worker_loads.sort_by(|a, b| b.1.cmp(&a.1));
                
                victims = worker_loads.into_iter().map(|(id, _)| id).collect();
            }
            
            StealStrategy::Nearest => {
                // For NUMA-aware stealing, prefer nearby workers
                // This is a simplified version - real NUMA would use topology
                let thief_idx = thief_id.as_usize();
                
                // Try immediate neighbors first
                if thief_idx > 0 {
                    victims.push(WorkerId(thief_idx - 1));
                }
                if thief_idx + 1 < num_workers {
                    victims.push(WorkerId(thief_idx + 1));
                }
                
                // Then expand outward
                for distance in 2..num_workers {
                    if thief_idx >= distance {
                        victims.push(WorkerId(thief_idx - distance));
                    }
                    if thief_idx + distance < num_workers {
                        victims.push(WorkerId(thief_idx + distance));
                    }
                }
            }
        }
        
        victims
    }
    
    /// Get stealing statistics
    pub fn stats(&self) -> StealStats {
        StealStats {
            steal_attempts: AtomicUsize::new(
                self.stats.steal_attempts.load(Ordering::Relaxed)
            ),
            successful_steals: AtomicUsize::new(
                self.stats.successful_steals.load(Ordering::Relaxed)
            ),
            failed_steals: AtomicUsize::new(
                self.stats.failed_steals.load(Ordering::Relaxed)
            ),
            retry_count: AtomicUsize::new(
                self.stats.retry_count.load(Ordering::Relaxed)
            ),
        }
    }
    
    /// Get the current strategy
    pub fn strategy(&self) -> StealStrategy {
        self.strategy
    }
    
    /// Get the current scheduling policy
    pub fn policy(&self) -> SchedulingPolicy {
        self.policy
    }
    
    /// Update the stealing strategy
    pub fn set_strategy(&mut self, strategy: StealStrategy) {
        self.strategy = strategy;
    }
    
    /// Update the scheduling policy
    pub fn set_policy(&mut self, policy: SchedulingPolicy) {
        self.policy = policy;
    }
}

impl Default for WorkStealer {
    fn default() -> Self {
        Self::new(StealStrategy::Random, SchedulingPolicy::LIFO)
    }
}

/// Load balancing heuristics
pub struct LoadBalancer {
    /// Threshold for stealing (percentage of max queue size)
    steal_threshold: f32,
    /// Minimum tasks to leave with victim
    min_victim_tasks: usize,
    /// Maximum tasks to steal at once
    max_steal_batch: usize,
}

impl LoadBalancer {
    /// Create a new load balancer
    pub fn new(steal_threshold: f32, min_victim_tasks: usize, max_steal_batch: usize) -> Self {
        Self {
            steal_threshold,
            min_victim_tasks,
            max_steal_batch,
        }
    }
    
    /// Calculate how many tasks to steal
    pub fn calculate_steal_amount(&self, victim_tasks: usize, thief_tasks: usize) -> usize {
        if victim_tasks <= self.min_victim_tasks {
            return 0;
        }
        
        // Try to balance the load between victim and thief
        let total = victim_tasks + thief_tasks;
        let target = total / 2;
        
        if thief_tasks >= target {
            return 0;
        }
        
        let steal_amount = (target - thief_tasks).min(victim_tasks - self.min_victim_tasks);
        steal_amount.min(self.max_steal_batch)
    }
    
    /// Check if stealing is recommended based on load imbalance
    pub fn should_steal(&self, victim_tasks: usize, thief_tasks: usize, max_queue: usize) -> bool {
        let threshold = (max_queue as f32 * self.steal_threshold) as usize;
        
        victim_tasks > threshold && thief_tasks < threshold / 2
    }
}

impl Default for LoadBalancer {
    fn default() -> Self {
        Self::new(0.5, 1, 32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_work_stealer_creation() {
        let stealer = WorkStealer::new(StealStrategy::Random, SchedulingPolicy::LIFO);
        assert_eq!(stealer.strategy(), StealStrategy::Random);
        assert_eq!(stealer.policy(), SchedulingPolicy::LIFO);
    }
    
    #[test]
    fn test_victim_selection_strategies() {
        let stealer = WorkStealer::new(StealStrategy::RoundRobin, SchedulingPolicy::FIFO);
        
        // Create dummy workers
        let workers: Vec<Arc<Worker>> = (0..4)
            .map(|i| {
                let config = super::super::worker::WorkerConfig {
                    id: WorkerId(i),
                    ..Default::default()
                };
                Arc::new(Worker::new(config))
            })
            .collect();
        
        let victims = stealer.select_victims(WorkerId(0), &workers);
        assert_eq!(victims.len(), 3);
        assert!(!victims.contains(&WorkerId(0)));
    }
    
    #[test]
    fn test_load_balancer() {
        let balancer = LoadBalancer::default();
        
        // Test steal amount calculation
        let steal_amount = balancer.calculate_steal_amount(100, 10);
        assert!(steal_amount > 0 && steal_amount <= 32);
        
        // Test should_steal decision
        assert!(balancer.should_steal(80, 5, 100));
        assert!(!balancer.should_steal(20, 15, 100));
    }
}

//! M:N scheduler implementation for RustRoutines
//!
//! This module provides a work-stealing scheduler that maps M routines to N OS threads,
//! similar to Go's runtime scheduler.

pub mod worker;
pub mod queue;
pub mod scheduler;
pub mod steal;

pub use scheduler::{Scheduler, SchedulerConfig};
pub use worker::{Worker, WorkerId};
pub use queue::{WorkQueue, Task};
pub use steal::{WorkStealer, StealStrategy, SchedulingPolicy, LoadBalancer};

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_scheduler_creation() {
        let config = SchedulerConfig::default();
        let scheduler = Scheduler::new(config);
        assert!(scheduler.is_ok());
        let scheduler = scheduler.unwrap();
        assert!(scheduler.num_workers() > 0);
    }
}

//! Test demonstrating the global scheduler problem
//! 
//! This test shows why the current GLOBAL_SCHEDULER design is broken.
//! Run with: cargo test --test demonstrate_problem
//!
//! These tests use a custom test harness that properly handles the rust-routines
//! runtime without conflicting with Tokio.

mod common;

use rust_routines::scheduler::scheduler_stats;
use std::sync::atomic::Ordering;

#[test]
fn test_global_scheduler_contamination_1() {
    common::run_test(|| async {
        let initial_stats = scheduler_stats();
        let initial_count = initial_stats.tasks_scheduled.load(Ordering::Relaxed);
        
        println!("Test 1 - Initial scheduled tasks: {}", initial_count);
        
        // This test might see tasks from other tests!
        // The count is unpredictable based on test execution order
        
        // Schedule some tasks
        for _ in 0..10 {
            // Uses GLOBAL_SCHEDULER
            rust_routines::go!(async {
                let mut sum = 0;
                for i in 0..100 {
                    sum += i;
                }
                sum
            });
        }
        
        let final_stats = scheduler_stats();
        let final_count = final_stats.tasks_scheduled.load(Ordering::Relaxed);
        let tasks_scheduled = final_count - initial_count;
        
        println!("Test 1 - Tasks scheduled in this test: {}", tasks_scheduled);
        
        // This assertion might fail if other tests ran first!
        // We expect 10, but might get more due to contamination
        assert!(tasks_scheduled >= 10, "Expected at least 10 tasks, got {}", tasks_scheduled);
    });
}

#[test]
fn test_global_scheduler_contamination_2() {
    common::run_test(|| async {
        let initial_stats = scheduler_stats();
        let initial_count = initial_stats.tasks_scheduled.load(Ordering::Relaxed);
        
        println!("Test 2 - Initial scheduled tasks: {}", initial_count);
        
        // This count will be affected by test_global_scheduler_contamination_1
        // if they run in the same process!
        
        // The initial count here will be non-zero if test 1 ran first
        // This demonstrates the global state problem
        
        // Schedule more tasks
        for _ in 0..5 {
            rust_routines::go!(async {
                let mut product = 1;
                for i in 1..10 {
                    product *= i;
                }
                product
            });
        }
        
        let final_stats = scheduler_stats();
        let final_count = final_stats.tasks_scheduled.load(Ordering::Relaxed);
        let tasks_scheduled = final_count - initial_count;
        
        println!("Test 2 - Tasks scheduled in this test: {}", tasks_scheduled);
        
        assert!(tasks_scheduled >= 5, "Expected at least 5 tasks, got {}", tasks_scheduled);
        
        // The total count will be cumulative across all tests
        println!("Test 2 - Total tasks scheduled globally: {}", final_count);
    });
}

#[test]
fn test_no_way_to_reset_scheduler() {
    common::run_test(|| async {
        // There's no API to reset the global scheduler!
        // Once initialized, it persists for the entire process lifetime
        
        let stats_before = scheduler_stats();
        let before_count = stats_before.tasks_scheduled.load(Ordering::Relaxed);
        
        // Schedule a task
        rust_routines::go!(async {
            "This task affects global state"
        });
        
        // Try to "reset" - but there's no way to do it!
        // The GLOBAL_SCHEDULER is a static that can't be reset
        
        // Schedule another task
        rust_routines::go!(async {
            "This task sees contaminated state"
        });
        
        let stats_after = scheduler_stats();
        let after_count = stats_after.tasks_scheduled.load(Ordering::Relaxed);
        
        // We scheduled 2 tasks, but the count includes ALL previous tests
        let our_tasks = after_count - before_count;
        assert!(our_tasks >= 2, "Expected at least 2 tasks, got {}", our_tasks);
        
        println!("No reset possible - global count is now: {}", after_count);
    });
}

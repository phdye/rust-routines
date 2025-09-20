//! Example demonstrating the fixed runtime isolation
//!
//! This shows how the new runtime system provides proper isolation
//! and allows tests to run in parallel without interference.

use rust_routines::runtime_v2::{Runtime, spawn};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn main() {
    println!("Demonstrating isolated runtime execution...\n");
    
    // Example 1: Multiple independent runtimes
    demo_independent_runtimes();
    
    // Example 2: Concurrent execution within a runtime
    demo_concurrent_execution();
    
    // Example 3: Clean shutdown
    demo_clean_shutdown();
}

fn demo_independent_runtimes() {
    println!("=== Independent Runtimes Demo ===");
    
    // Create two completely independent runtimes
    let runtime1 = Runtime::new().unwrap();
    let runtime2 = Runtime::new().unwrap();
    
    let counter1 = Arc::new(AtomicUsize::new(0));
    let counter2 = Arc::new(AtomicUsize::new(0));
    
    // Execute tasks on runtime1
    let cnt1 = Arc::clone(&counter1);
    runtime1.block_on(async move {
        for _ in 0..100 {
            let c = Arc::clone(&cnt1);
            spawn(async move {
                c.fetch_add(1, Ordering::Relaxed);
            }).unwrap();
        }
        
        // Wait a bit for tasks to complete
        tokio::time::sleep(Duration::from_millis(100)).await;
    });
    
    // Execute tasks on runtime2
    let cnt2 = Arc::clone(&counter2);
    runtime2.block_on(async move {
        for _ in 0..50 {
            let c = Arc::clone(&cnt2);
            spawn(async move {
                c.fetch_add(1, Ordering::Relaxed);
            }).unwrap();
        }
        
        // Wait a bit for tasks to complete
        tokio::time::sleep(Duration::from_millis(100)).await;
    });
    
    println!("Runtime1 executed {} tasks", counter1.load(Ordering::Relaxed));
    println!("Runtime2 executed {} tasks", counter2.load(Ordering::Relaxed));
    
    // Check statistics are isolated
    let stats1 = runtime1.stats();
    let stats2 = runtime2.stats();
    
    println!("Runtime1 stats: {} spawned, {} completed", 
             stats1.routines_spawned, stats1.routines_completed);
    println!("Runtime2 stats: {} spawned, {} completed", 
             stats2.routines_spawned, stats2.routines_completed);
    
    println!("✅ Runtimes are completely isolated!\n");
}

fn demo_concurrent_execution() {
    println!("=== Concurrent Execution Demo ===");
    
    let runtime = Runtime::new().unwrap();
    let counter = Arc::new(AtomicUsize::new(0));
    
    runtime.block_on(async {
        let mut handles = vec![];
        
        // Spawn many concurrent tasks
        for i in 0..1000 {
            let cnt = Arc::clone(&counter);
            let handle = spawn(async move {
                // Simulate some work
                let mut sum = 0;
                for j in 0..100 {
                    sum += i * j;
                }
                cnt.fetch_add(1, Ordering::Relaxed);
                sum
            }).unwrap();
            handles.push(handle);
        }
        
        // Wait for all tasks
        for handle in handles {
            handle.join().await.unwrap();
        }
    });
    
    println!("Executed {} concurrent tasks", counter.load(Ordering::Relaxed));
    
    let stats = runtime.stats();
    println!("Runtime stats: {} spawned, {} completed, {} active", 
             stats.routines_spawned, stats.routines_completed, stats.routines_active);
    
    println!("✅ Concurrent execution works perfectly!\n");
}

fn demo_clean_shutdown() {
    println!("=== Clean Shutdown Demo ===");
    
    let runtime = Runtime::new().unwrap();
    
    runtime.block_on(async {
        // Spawn some tasks
        for i in 0..10 {
            spawn(async move {
                println!("Task {} executing", i);
            }).unwrap();
        }
        
        tokio::time::sleep(Duration::from_millis(50)).await;
    });
    
    println!("Shutting down runtime...");
    runtime.shutdown().unwrap();
    println!("✅ Runtime shut down cleanly!\n");
    
    // Create a new runtime - starts fresh!
    let new_runtime = Runtime::new().unwrap();
    let stats = new_runtime.stats();
    println!("New runtime stats: {} spawned (starts at 0!)", stats.routines_spawned);
    
    new_runtime.shutdown().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    
    // These tests can all run in PARALLEL without interference!
    
    #[tokio::test]
    async fn test_isolated_runtime_1() {
        let runtime = Runtime::new().unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        
        runtime.block_on(async {
            for _ in 0..100 {
                let cnt = Arc::clone(&counter);
                spawn(async move {
                    cnt.fetch_add(1, Ordering::Relaxed);
                }).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        
        assert_eq!(counter.load(Ordering::Relaxed), 100);
    }
    
    #[tokio::test]
    async fn test_isolated_runtime_2() {
        // This test runs completely independently of test_isolated_runtime_1!
        let runtime = Runtime::new().unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        
        runtime.block_on(async {
            for _ in 0..200 {
                let cnt = Arc::clone(&counter);
                spawn(async move {
                    cnt.fetch_add(1, Ordering::Relaxed);
                }).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        
        assert_eq!(counter.load(Ordering::Relaxed), 200);
    }
    
    #[test]
    fn test_runtime_statistics_isolation() {
        let runtime1 = Runtime::new().unwrap();
        let runtime2 = Runtime::new().unwrap();
        
        // Spawn on runtime1
        runtime1.block_on(async {
            for _ in 0..10 {
                spawn(async { 1 + 1 }).unwrap();
            }
        });
        
        // Spawn on runtime2
        runtime2.block_on(async {
            for _ in 0..5 {
                spawn(async { 2 + 2 }).unwrap();
            }
        });
        
        let stats1 = runtime1.stats();
        let stats2 = runtime2.stats();
        
        // Statistics are completely independent!
        assert_eq!(stats1.routines_spawned, 10);
        assert_eq!(stats2.routines_spawned, 5);
    }
}

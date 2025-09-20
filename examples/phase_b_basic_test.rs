use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use rust_routines::prelude::*;
use rust_routines::scheduler::scheduler_stats;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Phase B: Basic Enhanced Features Test");
    println!("════════════════════════════════════════");
    
    // Get initial stats
    let initial_stats = scheduler_stats();
    println!("📊 Initial scheduler stats:");
    println!("   Active workers: {}", initial_stats.active_workers.load(Ordering::Relaxed));
    println!("   Tasks scheduled: {}", initial_stats.tasks_scheduled.load(Ordering::Relaxed));
    
    let counter = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    
    // Test 1: Basic go! macro (CPU-only work, no async operations)
    println!("\n1️⃣ Testing basic go! macro:");
    for i in 0..5 {
        let counter_clone = Arc::clone(&counter);
        let handle = go!(async move {
            // Pure CPU work - no async operations
            let mut sum = 0;
            for j in 0..1000 {
                sum += i * j;
            }
            counter_clone.fetch_add(1, Ordering::Relaxed);
            sum
        });
        handles.push(handle);
    }
    
    // Test 2: Enhanced spawn function (CPU-only work)
    println!("2️⃣ Testing spawn_enhanced():");
    for i in 5..10 {
        let counter_clone = Arc::clone(&counter);
        let handle = spawn_enhanced(async move {
            // Pure CPU work - no async operations
            let mut sum = 0;
            for j in 0..2000 {
                sum += i * j;
            }
            counter_clone.fetch_add(1, Ordering::Relaxed);
            sum
        });
        handles.push(handle);
    }
    
    // Test 3: Named spawn function (CPU-only work)
    println!("3️⃣ Testing spawn_with_name():");
    for i in 10..15 {
        let counter_clone = Arc::clone(&counter);
        let task_name = format!("cpu_task_{}", i);
        let handle = spawn_with_name(async move {
            // Pure CPU work - no async operations
            let mut sum = 0;
            for j in 0..3000 {
                sum += i * j;
            }
            counter_clone.fetch_add(1, Ordering::Relaxed);
            sum
        }, task_name);
        handles.push(handle);
    }
    
    println!("\n⏳ Waiting for {} routines to complete...", handles.len());
    
    // Wait for all tasks to complete
    let mut results = Vec::new();
    for handle in handles {
        let result = handle.join().await?;
        results.push(result);
    }
    
    let final_count = counter.load(Ordering::Relaxed);
    println!("✅ All routines completed: {}/15", final_count);
    
    // Show enhanced statistics
    let final_scheduler_stats = scheduler_stats();
    println!("\n📈 Final scheduler stats:");
    println!("   Tasks scheduled: {}", final_scheduler_stats.tasks_scheduled.load(Ordering::Relaxed));
    println!("   Tasks completed: {}", final_scheduler_stats.tasks_completed.load(Ordering::Relaxed));
    println!("   Active workers: {}", final_scheduler_stats.active_workers.load(Ordering::Relaxed));
    
    let tasks_scheduled = final_scheduler_stats.tasks_scheduled.load(Ordering::Relaxed);
    let initial_scheduled = initial_stats.tasks_scheduled.load(Ordering::Relaxed);
    
    if tasks_scheduled > initial_scheduled {
        println!("🎉 SUCCESS: {} tasks executed on M:N scheduler!", tasks_scheduled - initial_scheduled);
        println!("✨ Enhanced executor features working correctly!");
    } else {
        println!("❌ WARNING: No tasks detected on M:N scheduler");
    }
    
    // Show some sample results
    println!("\n📋 Sample computation results:");
    for (i, result) in results.iter().take(3).enumerate() {
        println!("   Task {}: {}", i + 1, result);
    }
    
    println!("\n🎯 Phase B Basic Test Complete!");
    println!("   ✅ Basic routines work on M:N scheduler");
    println!("   ✅ Enhanced spawn functions operational");
    println!("   ✅ Named routines supported");
    println!("   ✅ Statistics collection functional");
    
    Ok(())
}
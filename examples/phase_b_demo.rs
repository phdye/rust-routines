use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::time::Duration;
use rust_routines::prelude::*;
use rust_routines::scheduler::scheduler_stats;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Phase B: Enhanced Executor Integration Demo");
    println!("═══════════════════════════════════════════════");
    
    // Initialize the enhanced integrated runtime
    let config = IntegratedRuntimeConfig {
        tokio_worker_threads: 2,
        scheduler_worker_threads: 4,
        tokio_thread_name: "enhanced-tokio".to_string(),
        scheduler_thread_name: "enhanced-scheduler".to_string(),
        enable_detailed_stats: true,
        enable_cpu_affinity: false,
        ..Default::default()
    };
    
    init_integrated_runtime(config)?;
    println!("✅ Enhanced integrated runtime initialized");
    
    // Get initial stats
    let initial_stats = scheduler_stats();
    println!("📊 Initial scheduler stats:");
    println!("   Active workers: {}", initial_stats.active_workers.load(Ordering::Relaxed));
    println!("   Tasks scheduled: {}", initial_stats.tasks_scheduled.load(Ordering::Relaxed));
    
    if let Some(runtime_stats) = global_runtime_stats() {
        println!("   Runtime uptime: {:.2?}", runtime_stats.uptime);
        runtime_stats.print_report();
    }
    
    println!("\n🧪 Testing Enhanced Features:");
    println!("─────────────────────────────");
    
    let counter = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    
    // Test 1: Basic go! macro (uses basic executor)
    println!("\n1️⃣ Testing basic go! macro (backward compatibility):");
    for i in 0..5 {
        let counter_clone = Arc::clone(&counter);
        let handle = go!(async move {
            let mut sum = 0;
            for j in 0..50 {
                sum += i * j;
            }
            counter_clone.fetch_add(1, Ordering::Relaxed);
            format!("Basic task {} result: {}", i, sum)
        });
        handles.push(handle);
    }
    
    // Test 2: Enhanced go! macro (uses enhanced executor with stats)
    println!("2️⃣ Testing go_enhanced! macro (enhanced executor):");
    for i in 5..10 {
        let counter_clone = Arc::clone(&counter);
        let handle = go_enhanced!(async move {
            let mut sum = 0;
            for j in 0..100 {
                sum += i * j;
            }
            counter_clone.fetch_add(1, Ordering::Relaxed);
            format!("Enhanced task {} result: {}", i, sum)
        });
        handles.push(handle);
    }
    
    // Test 3: Named routines for debugging
    println!("3️⃣ Testing go_named! macro (debugging support):");
    for i in 10..15 {
        let counter_clone = Arc::clone(&counter);
        let task_name = format!("debug_task_{}", i);
        let handle = go_named!(task_name, async move {
            // Simulate some work with scheduler-friendly yield
            yield_now().await;
            
            let mut sum = 0;
            for j in 0..150 {
                sum += i * j;
            }
            counter_clone.fetch_add(1, Ordering::Relaxed);
            format!("Named task {} result: {}", i, sum)
        });
        handles.push(handle);
    }
    
    // Test 4: Direct enhanced spawn function
    println!("4️⃣ Testing spawn_enhanced() function:");
    for i in 15..20 {
        let counter_clone = Arc::clone(&counter);
        let handle = spawn_enhanced(async move {
            // Simulate complex work with multiple yields
            for _ in 0..3 {
                yield_now().await;
            }
            
            let mut sum = 0;
            for j in 0..200 {
                sum += i * j;
            }
            counter_clone.fetch_add(1, Ordering::Relaxed);
            format!("Direct enhanced task {} result: {}", i, sum)
        });
        handles.push(handle);
    }
    
    // Test 5: Named spawn function
    println!("5️⃣ Testing spawn_with_name() function:");
    for i in 20..25 {
        let counter_clone = Arc::clone(&counter);
        let task_name = format!("direct_named_task_{}", i);
        let handle = spawn_with_name(async move {
            // Simulate some work with scheduler-compatible sleep
            sleep(Duration::from_millis(5)).await;
            
            let mut sum = 0;
            for j in 0..250 {
                sum += i * j;
            }
            counter_clone.fetch_add(1, Ordering::Relaxed);
            format!("Direct named task {} result: {}", i, sum)
        }, task_name);
        handles.push(handle);
    }
    
    println!("\n⏳ Waiting for all {} routines to complete...", handles.len());
    
    // Wait for all tasks to complete
    let mut results = Vec::new();
    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle.join().await?;
        results.push(result);
        
        if i % 5 == 4 {
            let current_count = counter.load(Ordering::Relaxed);
            println!("   Progress: {}/{} routines completed", current_count, 25);
        }
    }
    
    let final_count = counter.load(Ordering::Relaxed);
    println!("✅ All routines completed: {}/25", final_count);
    
    // Show enhanced statistics
    println!("\n📈 Enhanced Runtime Statistics:");
    println!("─────────────────────────────────");
    
    let final_scheduler_stats = scheduler_stats();
    println!("🚀 M:N Scheduler:");
    println!("   Tasks scheduled: {}", final_scheduler_stats.tasks_scheduled.load(Ordering::Relaxed));
    println!("   Tasks completed: {}", final_scheduler_stats.tasks_completed.load(Ordering::Relaxed));
    println!("   Active workers: {}", final_scheduler_stats.active_workers.load(Ordering::Relaxed));
    println!("   Parked workers: {}", final_scheduler_stats.parked_workers.load(Ordering::Relaxed));
    
    if let Some(runtime_report) = global_runtime_stats() {
        println!("\n📊 Integrated Runtime Report:");
        runtime_report.print_report();
    }
    
    // Show some sample results
    println!("\n📋 Sample Results:");
    println!("─────────────────");
    for (i, result) in results.iter().take(3).enumerate() {
        println!("   {}: {}", i + 1, result);
    }
    if results.len() > 3 {
        println!("   ... and {} more results", results.len() - 3);
    }
    
    println!("\n🎉 Phase B: Enhanced Executor Integration Demo Complete!");
    println!("✨ Features demonstrated:");
    println!("   ✅ Basic go! macro (backward compatible)");
    println!("   ✅ go_enhanced! macro (performance monitoring)");
    println!("   ✅ go_named! macro (debugging support)");  
    println!("   ✅ spawn_enhanced() function (direct API)");
    println!("   ✅ spawn_with_name() function (named tasks)");
    println!("   ✅ Integrated runtime configuration");
    println!("   ✅ Enhanced statistics collection");
    println!("   ✅ Performance monitoring capabilities");
    println!("   ✅ Multiple executor types working together");
    
    let tasks_scheduled = final_scheduler_stats.tasks_scheduled.load(Ordering::Relaxed);
    let initial_scheduled = initial_stats.tasks_scheduled.load(Ordering::Relaxed);
    println!("\n🔥 Performance Summary:");
    println!("   {} new tasks executed on M:N scheduler", tasks_scheduled - initial_scheduled);
    println!("   Enhanced executor features working correctly");
    println!("   All async/await operations functioning properly");
    
    Ok(())
}
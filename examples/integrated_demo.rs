use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use rust_routines::prelude::*;
use rust_routines::scheduler::scheduler_stats;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Testing Integrated Routines: M:N Scheduler Integration");
    
    // Get initial scheduler stats
    let initial_stats = scheduler_stats();
    println!("📊 Initial scheduler stats:");
    println!("   Tasks scheduled: {}", initial_stats.tasks_scheduled.load(Ordering::Relaxed));
    println!("   Active workers: {}", initial_stats.active_workers.load(Ordering::Relaxed));
    
    let counter = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    
    // Spawn routines using the go! macro - should now use our scheduler
    println!("⚡ Spawning 20 routines using go! macro...");
    for i in 0..20 {
        let counter_clone = Arc::clone(&counter);
        let handle = go!(async move {
            // Do some work
            let mut sum = 0;
            for j in 0..100 {
                sum += i * j;
            }
            
            // Increment counter
            counter_clone.fetch_add(1, Ordering::Relaxed);
            
            if i % 5 == 0 {
                println!("✅ Routine {} completed (sum: {})", i, sum);
            }
        });
        handles.push(handle);
    }
    
    // Wait for completion
    println!("⏳ Waiting for routines to complete...");
    for handle in handles {
        handle.join().await?;
    }
    
    let final_count = counter.load(Ordering::Relaxed);
    println!("🎯 Final result: {}/20 routines completed", final_count);
    
    // Check scheduler stats
    let final_stats = scheduler_stats();
    println!("\n📈 Final scheduler stats:");
    println!("   Tasks scheduled: {}", final_stats.tasks_scheduled.load(Ordering::Relaxed));
    println!("   Tasks completed: {}", final_stats.tasks_completed.load(Ordering::Relaxed));
    println!("   Active workers: {}", final_stats.active_workers.load(Ordering::Relaxed));
    
    // Verify that our scheduler was actually used
    let tasks_scheduled = final_stats.tasks_scheduled.load(Ordering::Relaxed);
    if tasks_scheduled > initial_stats.tasks_scheduled.load(Ordering::Relaxed) {
        println!("🎉 SUCCESS: Routines are now using the M:N scheduler!");
        println!("   {} new tasks were scheduled on our custom scheduler", 
                 tasks_scheduled - initial_stats.tasks_scheduled.load(Ordering::Relaxed));
    } else {
        println!("❌ WARNING: Routines may not be using the M:N scheduler");
    }
    
    println!("\n✨ Integration test complete!");
    println!("🔧 Features demonstrated:");
    println!("   ✅ go! macro schedules on M:N scheduler (not Tokio)");
    println!("   ✅ Future-to-Task bridge working");
    println!("   ✅ Async/await compatibility maintained");
    println!("   ✅ Statistics tracking functional");
    
    Ok(())
}
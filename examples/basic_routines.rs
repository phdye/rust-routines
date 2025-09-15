//! Basic routine spawning and joining example
//!
//! Demonstrates the fundamental operations of creating and managing routines.

use rust_routines::prelude::*;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Basic Routines Example ===\n");

    // Example 1: Simple routine that returns a value
    println!("1. Simple routine with return value:");
    let handle = go!(async {
        println!("   Routine is running...");
        sleep(Duration::from_millis(100)).await;
        42
    });

    let result = handle.join().await?;
    println!("   Routine returned: {}\n", result);

    // Example 2: Multiple routines running concurrently
    println!("2. Multiple concurrent routines:");
    let mut handles = Vec::new();

    for i in 1..=5 {
        let handle = go!(async move {
            let delay = i * 50;
            println!("   Routine {} starting (delay: {}ms)", i, delay);
            sleep(Duration::from_millis(delay)).await;
            println!("   Routine {} completed", i);
            i * 10
        });
        handles.push(handle);
    }

    // Wait for all routines to complete
    let mut results = Vec::new();
    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle.join().await?;
        results.push(result);
        println!("   Collected result from routine {}: {}", i + 1, result);
    }

    println!("   All results: {:?}\n", results);

    // Example 3: Routine with error handling
    println!("3. Error handling in routines:");
    let handle = go!(async {
        sleep(Duration::from_millis(50)).await;
        if rand::random::<bool>() {
            Ok("Success!")
        } else {
            Err("Something went wrong")
        }
    });

    match handle.join().await? {
        Ok(message) => println!("   Routine succeeded: {}", message),
        Err(error) => println!("   Routine failed: {}", error),
    }

    // Example 4: Cooperative yielding
    println!("\n4. Cooperative yielding:");
    let handle = go!(async {
        for i in 1..=3 {
            println!("   Working on task {}...", i);
            yield_now().await; // Allow other routines to run
            sleep(Duration::from_millis(10)).await;
        }
        "All tasks completed"
    });

    let result = handle.join().await?;
    println!("   {}", result);

    println!("\n=== Example completed successfully! ===");
    Ok(())
}

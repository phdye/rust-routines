//! Integration tests for RustRoutines
//!
//! These tests verify that the library works correctly in realistic scenarios.

use rust_routines::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_producer_consumer_pattern() {
    let (tx, mut rx) = channel::<i32>();
    let counter = Arc::new(AtomicUsize::new(0));
    
    // Producer
    let tx_clone = tx.clone();
    let producer = go!(async move {
        for i in 0..100 {
            tx_clone.send(i).await.unwrap();
        }
    });
    
    // Consumer
    let counter_clone = Arc::clone(&counter);
    let consumer = go!(async move {
        while let Ok(value) = rx.recv().await {
            counter_clone.fetch_add(value as usize, Ordering::Relaxed);
        }
    });
    
    // Wait for producer to finish
    producer.join().await.unwrap();
    
    // Close the channel by dropping all senders
    drop(tx);
    
    // Wait for consumer to finish
    consumer.join().await.unwrap();
    
    // Verify the sum (0 + 1 + ... + 99 = 4950)
    assert_eq!(counter.load(Ordering::Relaxed), 4950);
}

#[tokio::test]
async fn test_fan_out_fan_in_pattern() {
    let (work_tx, work_rx) = channel::<usize>();
    let (result_tx, mut result_rx) = channel::<usize>();
    
    // Wrap the work receiver in Arc<Mutex<>> to share among workers
    let work_rx = Arc::new(tokio::sync::Mutex::new(work_rx));
    
    // Start workers
    let num_workers = 5;
    let mut workers = Vec::new();
    
    for _worker_id in 0..num_workers {
        let work_rx_clone = Arc::clone(&work_rx);
        let result_tx_clone = result_tx.clone();
        
        let worker = go!(async move {
            loop {
                let work_result = {
                    let mut rx = work_rx_clone.lock().await;
                    rx.recv().await
                };
                match work_result {
                    Ok(work) => {
                        // Simulate work: square the number
                        let result = work * work;
                        result_tx_clone.send(result).await.unwrap();
                    }
                    Err(_) => break, // Channel closed
                }
            }
        });
        workers.push(worker);
    }
    
    // Send work
    let work_count = 50;
    let sender = go!(async move {
        for i in 1..=work_count {
            work_tx.send(i).await.unwrap();
        }
    });
    
    // Collect results
    let collector = go!(async move {
        let mut results = Vec::new();
        for _ in 0..work_count {
            let result = result_rx.recv().await.unwrap();
            results.push(result);
        }
        results
    });
    
    // Wait for sender to finish
    sender.join().await.unwrap();
    
    // Close work channel
    drop(work_rx);
    
    // Wait for workers to finish
    for worker in workers {
        worker.join().await.unwrap();
    }
    
    // Close result channel
    drop(result_tx);
    
    // Get results
    let mut results = collector.join().await.unwrap();
    results.sort();
    
    // Verify results (squares of 1..=50)
    let expected: Vec<usize> = (1..=work_count).map(|i| i * i).collect();
    assert_eq!(results, expected);
}

#[tokio::test]
async fn test_select_with_multiple_channels() {
    let (tx1, rx1) = channel::<String>();
    let (tx2, rx2) = channel::<i32>();
    let (tx3, rx3) = channel::<bool>();
    
    // Send on different channels with delays
    go!(async move {
        sleep(Duration::from_millis(10)).await;
        tx1.send("hello".to_string()).await.unwrap();
    });
    
    go!(async move {
        sleep(Duration::from_millis(20)).await;
        tx2.send(42).await.unwrap();
    });
    
    go!(async move {
        sleep(Duration::from_millis(30)).await;
        tx3.send(true).await.unwrap();
    });
    
    // Use select to receive from first available channel
    let result = select()
        .recv(rx1, |msg| format!("string: {}", msg))
        .recv(rx2, |num| format!("number: {}", num))
        .recv(rx3, |flag| format!("boolean: {}", flag))
        .timeout(Duration::from_millis(100))
        .run()
        .await
        .unwrap();
    
    // Should receive from the first channel (shortest delay)
    assert_eq!(result, "string: hello");
}

#[tokio::test]
async fn test_select_timeout() {
    let (_tx, rx) = channel::<i32>();
    
    let start = Instant::now();
    let result = select()
        .recv(rx, |value| value)
        .timeout(Duration::from_millis(50))
        .run()
        .await;
    
    let elapsed = start.elapsed();
    
    // Should timeout
    assert!(matches!(result, Err(Error::Timeout)));
    
    // Should take approximately the timeout duration
    assert!(elapsed >= Duration::from_millis(45));
    assert!(elapsed <= Duration::from_millis(100));
}

#[tokio::test]
async fn test_select_default_case() {
    let (_tx, rx) = channel::<i32>();
    
    let result = select()
        .recv(rx, |value| value.to_string())
        .default(|| "default".to_string())
        .run()
        .await
        .unwrap();
    
    // Should use default case since channel is empty
    assert_eq!(result, "default");
}

#[tokio::test]
async fn test_routine_error_handling() {
    // Test that routine panics are handled gracefully
    let handle = go!(async {
        sleep(Duration::from_millis(10)).await;
        std::result::Result::<i32, &str>::Err("simulated error")
    });
    
    let result = handle.join().await.unwrap();
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "simulated error");
}

#[tokio::test]
async fn test_concurrent_channel_operations() {
    let (tx, mut rx) = channel::<usize>();
    let num_senders = 10;
    let messages_per_sender = 100;
    
    // Start multiple senders
    let mut senders = Vec::new();
    for sender_id in 0..num_senders {
        let tx_clone = tx.clone();
        let sender = go!(async move {
            for i in 0..messages_per_sender {
                let message = sender_id * messages_per_sender + i;
                tx_clone.send(message).await.unwrap();
            }
        });
        senders.push(sender);
    }
    
    // Start receiver
    let receiver = go!(async move {
        let mut received = Vec::new();
        let total_messages = num_senders * messages_per_sender;
        
        for _ in 0..total_messages {
            let message = rx.recv().await.unwrap();
            received.push(message);
        }
        
        received
    });
    
    // Wait for all senders to finish
    for sender in senders {
        sender.join().await.unwrap();
    }
    
    // Close the channel
    drop(tx);
    
    // Get received messages
    let mut received = receiver.join().await.unwrap();
    received.sort();
    
    // Verify all messages were received
    let expected: Vec<usize> = (0..(num_senders * messages_per_sender)).collect();
    assert_eq!(received, expected);
}

#[tokio::test]
async fn test_routine_yield_cooperation() {
    let shared_counter = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    
    // Start multiple routines that increment a counter
    for _i in 0..10 {
        let counter = Arc::clone(&shared_counter);
        let handle = go!(async move {
            for _j in 0..100 {
                counter.fetch_add(1, Ordering::Relaxed);
                yield_now().await; // Cooperatively yield
            }
        });
        handles.push(handle);
    }
    
    // Wait for all routines to complete
    for handle in handles {
        handle.join().await.unwrap();
    }
    
    // Verify the final count
    assert_eq!(shared_counter.load(Ordering::Relaxed), 1000);
}

//! Select implementation for RustRoutines
//!
//! Provides Go-like select functionality for multiplexing channel operations.
//! Uses crossbeam's select capabilities for pure M:N threading without async.

use crate::error::{Error, Result};
use crate::channel::Receiver;
use crossbeam::channel::Select as CbSelect;
use std::time::{Duration, Instant};
use rand::seq::SliceRandom;
use rand::{SeedableRng, rngs::StdRng};

/// Simplified select for two receivers (common case)
pub fn select_recv<T>(rx1: &Receiver<T>, rx2: &Receiver<T>) -> Result<(usize, T)> {
    // Use crossbeam's Select for fair selection
    let mut sel = CbSelect::new();
    let idx1 = sel.recv(rx1.inner_ref());
    let _idx2 = sel.recv(rx2.inner_ref());
    
    let oper = sel.select();
    let selected_index = oper.index();
    
    // Complete the operation by actually receiving
    let result = if selected_index == idx1 {
        match oper.recv(rx1.inner_ref()) {
            Ok(val) => Ok((0, val)),
            Err(_) => Err(Error::ChannelClosed),
        }
    } else {
        match oper.recv(rx2.inner_ref()) {
            Ok(val) => Ok((1, val)),
            Err(_) => Err(Error::ChannelClosed),
        }
    };
    
    result
}

/// Select with timeout
pub fn select_timeout<T>(rx: &Receiver<T>, timeout: Duration) -> Result<T> {
    rx.recv_timeout(timeout)
}

/// Multi-way select with fairness
pub fn select_fair<T>(receivers: Vec<&Receiver<T>>) -> Result<(usize, T)> {
    if receivers.is_empty() {
        return Err(Error::RuntimeError {
            reason: "No receivers provided".to_string()
        });
    }
    
    // Randomize for fairness
    let mut rng = StdRng::from_entropy();
    let mut indices: Vec<usize> = (0..receivers.len()).collect();
    indices.shuffle(&mut rng);
    
    // Try each receiver in random order (non-blocking)
    for &idx in &indices {
        if let Ok(val) = receivers[idx].try_recv() {
            return Ok((idx, val));
        }
    }
    
    // If none ready, block on all using crossbeam's Select
    let mut sel = CbSelect::new();
    let mut cb_indices = Vec::new();
    for rx in &receivers {
        cb_indices.push(sel.recv(rx.inner_ref()));
    }
    
    let oper = sel.select();
    let selected = oper.index();
    
    // Find which receiver index was selected and complete the operation
    for (i, &cb_idx) in cb_indices.iter().enumerate() {
        if selected == cb_idx {
            // Must complete the operation through the SelectedOperation
            match oper.recv(receivers[i].inner_ref()) {
                Ok(val) => return Ok((i, val)),
                Err(_) => return Err(Error::ChannelClosed),
            }
        }
    }
    
    Err(Error::RuntimeError {
        reason: "Select operation failed".to_string()
    })
}

/// Blocking select that waits for any of the channels to have data
pub fn select_blocking<T>(receivers: &[&Receiver<T>]) -> Result<(usize, T)> {
    if receivers.is_empty() {
        return Err(Error::RuntimeError {
            reason: "No receivers provided".to_string()
        });
    }
    
    let mut sel = CbSelect::new();
    let mut indices = Vec::new();
    
    for rx in receivers {
        indices.push(sel.recv(rx.inner_ref()));
    }
    
    // This will block until one channel is ready
    let oper = sel.select();
    let selected_index = oper.index();
    
    // Find which receiver was selected and complete the operation
    for (i, &idx) in indices.iter().enumerate() {
        if selected_index == idx {
            // Must complete the operation through the SelectedOperation
            match oper.recv(receivers[i].inner_ref()) {
                Ok(val) => return Ok((i, val)),
                Err(_) => return Err(Error::ChannelClosed),
            }
        }
    }
    
    Err(Error::RuntimeError {
        reason: "Select operation failed".to_string()
    })
}

/// Non-blocking select that returns immediately
pub fn select_try<T>(receivers: &[&Receiver<T>]) -> Result<(usize, T)> {
    for (i, rx) in receivers.iter().enumerate() {
        if let Ok(val) = rx.try_recv() {
            return Ok((i, val));
        }
    }
    Err(Error::ChannelEmpty)
}

/// Select with timeout - waits up to the specified duration
pub fn select_with_timeout<T>(receivers: &[&Receiver<T>], timeout: Duration) -> Result<(usize, T)> {
    let deadline = Instant::now() + timeout;
    
    // Try non-blocking first
    if let Ok(result) = select_try(receivers) {
        return Ok(result);
    }
    
    // Poll with small sleeps until timeout
    while Instant::now() < deadline {
        if let Ok(result) = select_try(receivers) {
            return Ok(result);
        }
        std::thread::sleep(Duration::from_micros(100));
    }
    
    Err(Error::Timeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{bounded_channel, channel};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_select_recv() {
        let (tx1, rx1) = channel::<i32>();
        let (tx2, rx2) = channel::<i32>();
        
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            tx1.send(42).unwrap();
        });
        
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            tx2.send(100).unwrap();
        });
        
        let (idx, val) = select_recv(&rx1, &rx2).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(val, 42);
    }

    #[test]
    fn test_select_timeout() {
        let (_tx, rx) = channel::<i32>();
        
        let result = select_timeout(&rx, Duration::from_millis(10));
        assert!(result.is_err());
        match result {
            Err(Error::Timeout) => (),
            _ => panic!("Expected timeout error"),
        }
    }

    #[test]
    fn test_select_fair() {
        let (tx1, rx1) = bounded_channel::<i32>(10);
        let (tx2, rx2) = bounded_channel::<i32>(10);
        let (tx3, rx3) = bounded_channel::<i32>(10);
        
        // Send to all channels
        tx1.send(1).unwrap();
        tx2.send(2).unwrap();
        tx3.send(3).unwrap();
        
        let receivers = vec![&rx1, &rx2, &rx3];
        
        // Should get one value
        let (idx, val) = select_fair(receivers).unwrap();
        assert!(idx < 3);
        assert!(val >= 1 && val <= 3);
    }

    #[test]
    fn test_select_blocking() {
        let (tx1, rx1) = channel::<i32>();
        let (tx2, rx2) = channel::<i32>();
        
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            tx1.send(42).unwrap();
        });
        
        let receivers = vec![&rx1, &rx2];
        let (idx, val) = select_blocking(&receivers).unwrap();
        
        assert_eq!(idx, 0);
        assert_eq!(val, 42);
    }

    #[test]
    fn test_select_try() {
        let (tx1, rx1) = bounded_channel::<i32>(1);
        let (_tx2, rx2) = bounded_channel::<i32>(1);
        
        // Initially empty
        let receivers = vec![&rx1, &rx2];
        assert!(select_try(&receivers).is_err());
        
        // Send to first channel
        tx1.send(100).unwrap();
        
        let (idx, val) = select_try(&receivers).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(val, 100);
    }

    #[test]
    fn test_select_with_timeout() {
        let (tx, rx1) = channel::<i32>();
        let (_tx2, rx2) = channel::<i32>();
        
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(5));
            tx.send(42).unwrap();
        });
        
        let receivers = vec![&rx1, &rx2];
        let (idx, val) = select_with_timeout(&receivers, Duration::from_millis(20)).unwrap();
        
        assert_eq!(idx, 0);
        assert_eq!(val, 42);
    }

    #[test]
    fn test_select_with_timeout_expires() {
        let (_tx1, rx1) = channel::<i32>();
        let (_tx2, rx2) = channel::<i32>();
        
        let receivers = vec![&rx1, &rx2];
        let result = select_with_timeout(&receivers, Duration::from_millis(10));
        
        assert!(result.is_err());
        match result {
            Err(Error::Timeout) => (),
            _ => panic!("Expected timeout error"),
        }
    }

    #[test]
    fn test_multi_select() {
        let channels: Vec<_> = (0..5).map(|_| bounded_channel::<i32>(1)).collect();
        
        // Send to channel 2
        channels[2].0.send(42).unwrap();
        
        let receivers: Vec<_> = channels.iter().map(|(_, rx)| rx).collect();
        let (idx, val) = select_fair(receivers).unwrap();
        
        assert_eq!(idx, 2);
        assert_eq!(val, 42);
    }

    #[test]
    fn test_select_fairness() {
        let (tx1, rx1) = bounded_channel::<i32>(100);
        let (tx2, rx2) = bounded_channel::<i32>(100);
        
        // Fill both channels
        for i in 0..100 {
            tx1.send(i).unwrap();
            tx2.send(i + 1000).unwrap();
        }
        
        let mut count1 = 0;
        let mut count2 = 0;
        
        // Select 100 times
        for _ in 0..100 {
            let receivers = vec![&rx1, &rx2];
            let (idx, _) = select_fair(receivers).unwrap();
            if idx == 0 {
                count1 += 1;
            } else {
                count2 += 1;
            }
        }
        
        // Should be roughly fair (not all from one channel)
        assert!(count1 > 20);
        assert!(count2 > 20);
    }
}

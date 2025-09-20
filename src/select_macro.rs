//! Select macro implementation for RustRoutines
//!
//! Provides a Go-like select! macro for multiplexing channel operations.
//! This macro works with rust-routines synchronous channels using crossbeam.

/// Select macro for multiplexing channel operations
/// 
/// This macro provides Go-like select functionality for rust-routines channels.
/// It randomly selects from ready operations, providing fairness.
/// 
/// # Examples
/// 
/// ```no_run
/// use rust_routines::prelude::*;
/// use rust_routines::rr_select;
/// use std::time::Duration;
/// 
/// let (tx1, rx1) = channel::<i32>();
/// let (tx2, rx2) = channel::<String>();
/// 
/// // Send some data
/// tx1.send(42).unwrap();
/// 
/// rr_select! {
///     val = rx1 => {
///         println!("Received int: {:?}", val);
///     },
///     msg = rx2 => {
///         println!("Received string: {:?}", msg);
///     },
///     default => {
///         println!("No data ready");
///     }
/// }
/// ```
#[macro_export]
macro_rules! rr_select {
    // Single receiver without default or timeout - just receive normally
    (
        $var:ident = $rx:expr => $body:block $(,)?
    ) => {{
        let $var = $rx.recv();
        $body
    }};
    
    // Single receiver with default
    (
        $var:ident = $rx:expr => $body:block,
        default => $default_body:block $(,)?
    ) => {{
        match $rx.try_recv() {
            Ok($var) => $body,
            Err(_) => $default_body
        }
    }};
    
    // Single receiver with timeout
    (
        $var:ident = $rx:expr => $body:block,
        timeout($duration:expr) => $timeout_body:block $(,)?
    ) => {{
        match $rx.recv_timeout($duration) {
            Ok($var) => $body,
            Err(_) => $timeout_body
        }
    }};
    
    // Two receivers without default - use crossbeam's select
    (
        $var1:ident = $rx1:expr => $body1:block,
        $var2:ident = $rx2:expr => $body2:block $(,)?
    ) => {{
        use crossbeam::channel::Select;
        
        let mut sel = Select::new();
        let idx1 = sel.recv($rx1.inner_ref());
        let idx2 = sel.recv($rx2.inner_ref());
        
        let oper = sel.select();
        let index = oper.index();
        
        if index == idx1 {
            let $var1 = oper.recv($rx1.inner_ref()).unwrap();
            $body1
        } else {
            let $var2 = oper.recv($rx2.inner_ref()).unwrap();
            $body2
        }
    }};
    
    // Two receivers with default - try non-blocking first
    (
        $var1:ident = $rx1:expr => $body1:block,
        $var2:ident = $rx2:expr => $body2:block,
        default => $default_body:block $(,)?
    ) => {{
        use crossbeam::channel::Select;
        
        // Try non-blocking receives first
        if let Ok($var1) = $rx1.try_recv() {
            $body1
        } else if let Ok($var2) = $rx2.try_recv() {
            $body2
        } else {
            $default_body
        }
    }};
    
    // Three receivers without default
    (
        $var1:ident = $rx1:expr => $body1:block,
        $var2:ident = $rx2:expr => $body2:block,
        $var3:ident = $rx3:expr => $body3:block $(,)?
    ) => {{
        use crossbeam::channel::Select;
        
        let mut sel = Select::new();
        let idx1 = sel.recv($rx1.inner_ref());
        let idx2 = sel.recv($rx2.inner_ref());
        let idx3 = sel.recv($rx3.inner_ref());
        
        let oper = sel.select();
        let index = oper.index();
        
        if index == idx1 {
            let $var1 = oper.recv($rx1.inner_ref()).unwrap();
            $body1
        } else if index == idx2 {
            let $var2 = oper.recv($rx2.inner_ref()).unwrap();
            $body2
        } else {
            let $var3 = oper.recv($rx3.inner_ref()).unwrap();
            $body3
        }
    }};
    
    // Three receivers with default
    (
        $var1:ident = $rx1:expr => $body1:block,
        $var2:ident = $rx2:expr => $body2:block,
        $var3:ident = $rx3:expr => $body3:block,
        default => $default_body:block $(,)?
    ) => {{
        use crossbeam::channel::Select;
        
        // Try non-blocking receives first for fairness
        let mut rng = rand::thread_rng();
        let mut order = vec![0, 1, 2];
        use rand::seq::SliceRandom;
        order.shuffle(&mut rng);
        
        let mut handled = false;
        for &idx in &order {
            match idx {
                0 => {
                    if let Ok($var1) = $rx1.try_recv() {
                        $body1
                        handled = true;
                        break;
                    }
                },
                1 => {
                    if let Ok($var2) = $rx2.try_recv() {
                        $body2
                        handled = true;
                        break;
                    }
                },
                2 => {
                    if let Ok($var3) = $rx3.try_recv() {
                        $body3
                        handled = true;
                        break;
                    }
                },
                _ => unreachable!(),
            }
        }
        
        if !handled {
            $default_body
        }
    }};
}

/// Simplified select macro with common patterns
#[macro_export]
macro_rules! select_timeout {
    ($timeout:expr, {
        $($var:ident = $rx:expr => $body:block),+ $(,)?
    }) => {{
        use std::time::Instant;
        use crossbeam::channel::Select;
        
        let deadline = Instant::now() + $timeout;
        let mut result = None;
        
        while result.is_none() && Instant::now() < deadline {
            // Try non-blocking receives
            $(
                if let Ok($var) = $rx.try_recv() {
                    result = Some({ $body });
                    break;
                }
            )+
            
            // Small sleep to avoid busy waiting
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        
        result
    }};
}

/// Select macro that returns the value from the selected channel
#[macro_export]
macro_rules! select_value {
    ($($rx:expr),+ $(,)?) => {{
        use crossbeam::channel::Select;
        
        let mut sel = Select::new();
        let receivers = vec![$($rx),+];
        let mut indices = Vec::new();
        
        for rx in &receivers {
            indices.push(sel.recv(rx.inner_ref()));
        }
        
        let oper = sel.select();
        let index = oper.index();
        
        // Find which receiver was selected
        for (i, &idx) in indices.iter().enumerate() {
            if index == idx {
                let val = oper.recv(receivers[i].inner_ref()).unwrap();
                return (i, val);
            }
        }
        
        unreachable!()
    }};
}

#[cfg(test)]
mod tests {
    use crate::channel::{channel, bounded_channel};
    use std::thread;
    use std::time::Duration;
    
    #[test]
    fn test_rr_select_two_channels() {
        let (tx1, rx1) = channel::<i32>();
        let (tx2, rx2) = channel::<i32>();
        
        thread::spawn(move || {
            tx1.send(42).unwrap();
        });
        
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            tx2.send(100).unwrap();
        });
        
        thread::sleep(Duration::from_millis(5)); // Let first sender run
        
        let mut result = 0;
        rr_select! {
            val = rx1 => {
                result = val;
            },
            val = rx2 => {
                result = val;
            }
        }
        
        assert_eq!(result, 42);
    }
    
    #[test]
    fn test_rr_select_default() {
        let (_tx1, rx1) = channel::<i32>();
        let (_tx2, rx2) = channel::<String>();
        
        let mut result = String::new();
        
        rr_select! {
            _val = rx1 => {
                result = "Got int".to_string();
            },
            _val = rx2 => {
                result = "Got string".to_string();
            },
            default => {
                result = "Nothing ready".to_string();
            }
        }
        
        assert_eq!(result, "Nothing ready");
    }
    
    #[test]
    fn test_rr_select_timeout() {
        let (_tx, rx) = channel::<i32>();
        
        let mut timed_out = false;
        
        rr_select! {
            _val = rx => {
                timed_out = false;
            },
            timeout(Duration::from_millis(10)) => {
                timed_out = true;
            }
        }
        
        assert!(timed_out);
    }
    
    #[test]
    fn test_select_timeout_macro() {
        let (tx, rx) = channel::<i32>();
        
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(5));
            tx.send(42).unwrap();
        });
        
        let result = select_timeout!(Duration::from_millis(20), {
            val = rx => { val * 2 }
        });
        
        assert_eq!(result, Some(84));
    }
    
    #[test]
    fn test_three_channel_select() {
        let (tx1, rx1) = bounded_channel::<i32>(1);
        let (tx2, rx2) = bounded_channel::<i32>(1);
        let (tx3, rx3) = bounded_channel::<i32>(1);
        
        tx2.send(200).unwrap();
        
        let mut result = 0;
        
        rr_select! {
            val = rx1 => {
                result = val;
            },
            val = rx2 => {
                result = val;
            },
            val = rx3 => {
                result = val;
            },
            default => {
                result = -1;
            }
        }
        
        assert_eq!(result, 200);
    }
}

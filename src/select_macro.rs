//! Select macro implementation for RustRoutines
//!
//! Provides a Go-like select! macro for multiplexing channel operations.
//! This macro is designed to work specifically with rust-routines channels
//! and doesn't require FusedFuture like the futures::select! macro.

/// Select macro for multiplexing channel operations
/// 
/// This macro provides Go-like select functionality for rust-routines channels.
/// It randomly selects from ready operations, providing fairness.
/// 
/// # Examples
/// 
/// ```no_run
/// use rust_routines::prelude::*;
/// use rust_routines::select; // Import the macro explicitly
/// use std::time::Duration;
/// 
/// # #[tokio::main]
/// # async fn main() {
/// let (tx1, mut rx1) = channel::<i32>();
/// let (_tx2, mut rx2) = channel::<String>();
/// 
/// // Send some data for the example to work
/// tx1.send(42).await.unwrap();
/// 
/// select! {
///     val = rx1.recv() => {
///         println!("Received int: {:?}", val);
///     },
///     msg = rx2.recv() => {
///         println!("Received string: {:?}", msg);
///     }
/// }
/// # }
/// ```
#[macro_export]
macro_rules! select {
    // Base case with just default
    (default => $default_body:block) => {{
        $default_body
    }};
    
    // Single operation with default
    ($pattern:pat = $expr:expr => $body:block, default => $default_body:block $(,)?) => {{
        // Try the operation once, fall back to default if not ready
        use std::task::{Context, Poll};
        use futures::FutureExt;
        
        let mut future = Box::pin($expr);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        
        match future.poll_unpin(&mut cx) {
            Poll::Ready($pattern) => {
                $body
            }
            Poll::Pending => {
                $default_body
            }
        }
    }};
    
    // Single operation without default - just await it
    ($pattern:pat = $expr:expr => $body:block $(,)?) => {{
        let $pattern = $expr.await;
        $body
    }};
    
    // Two operations with default
    (
        $pattern1:pat = $expr1:expr => $body1:block,
        $pattern2:pat = $expr2:expr => $body2:block,
        default => $default_body:block $(,)?
    ) => {{
        use std::task::{Context, Poll};
        use futures::FutureExt;
        
        let mut future1 = Box::pin($expr1);
        let mut future2 = Box::pin($expr2);
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);
        
        // Try first operation
        match future1.poll_unpin(&mut cx) {
            Poll::Ready($pattern1) => {
                $body1
            }
            Poll::Pending => {
                // Try second operation
                match future2.poll_unpin(&mut cx) {
                    Poll::Ready($pattern2) => {
                        $body2
                    }
                    Poll::Pending => {
                        $default_body
                    }
                }
            }
        }
    }};
    
    // Two operations without default
    (
        $pattern1:pat = $expr1:expr => $body1:block,
        $pattern2:pat = $expr2:expr => $body2:block $(,)?
    ) => {{
        // Use tokio::select! for proper async multiplexing
        tokio::select! {
            $pattern1 = $expr1 => {
                $body1
            }
            $pattern2 = $expr2 => {
                $body2
            }
        }
    }};
}

/// Helper macro to count the number of arms in select!
#[macro_export]
#[doc(hidden)]
macro_rules! count_arms {
    ($first:expr) => { 1 };
    ($first:expr, $($rest:expr),+) => { 1 + $crate::count_arms!($($rest),+) };
}

/// Helper macro to get the index of an arm
#[macro_export]
#[doc(hidden)]
macro_rules! arm_index {
    ($first:expr, $target:expr) => {
        0
    };
    ($first:expr, $($rest:expr),+, $target:expr) => {
        1 + $crate::arm_index!($($rest),+, $target)
    };
}

/// Alternative select implementation using async/await
/// This version properly integrates with the async runtime
#[macro_export]
macro_rules! select_async {
    // Pattern: multiple receive branches with optional default and timeout
    (
        $($var:ident = $rx:ident.recv() => $body:block),* $(,)?
        $(, timeout = $timeout_duration:expr => $timeout_body:block)?
        $(, default => $default_body:block)?
    ) => {{
        use std::future::Future;
        use std::pin::Pin;
        use std::task::{Context, Poll};
        
        // Create a custom future that polls all branches
        struct SelectFuture {
            completed: bool,
        }
        
        impl Future for SelectFuture {
            type Output = ();
            
            fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                if self.completed {
                    return Poll::Ready(());
                }
                
                // Try polling each receiver
                $(
                    {
                        let mut recv_fut = Box::pin($rx.recv());
                        match recv_fut.as_mut().poll(cx) {
                            Poll::Ready(result) => {
                                self.completed = true;
                                let $var = result;
                                $body
                                return Poll::Ready(());
                            }
                            Poll::Pending => {}
                        }
                    }
                )*
                
                // Check timeout if specified
                $(
                    {
                        let mut timeout_fut = Box::pin($crate::routine::sleep($timeout_duration));
                        match timeout_fut.as_mut().poll(cx) {
                            Poll::Ready(_) => {
                                self.completed = true;
                                $timeout_body
                                return Poll::Ready(());
                            }
                            Poll::Pending => {}
                        }
                    }
                )?
                
                // If nothing is ready and we have a default case
                $(
                    if true {  // Always execute default if we get here
                        self.completed = true;
                        $default_body
                        return Poll::Ready(());
                    }
                )?
                
                Poll::Pending
            }
        }
        
        SelectFuture { completed: false }.await
    }};
}

/// Simplified select macro for rust-routines
/// This version uses a simpler polling approach
#[macro_export]
macro_rules! rr_select {
    // Two receivers with optional default
    (
        $var1:ident = $rx1:expr => $body1:block,
        $var2:ident = $rx2:expr => $body2:block
        $(, default => $default_body:block)?
    ) => {{
        // Try both receivers in a loop until one succeeds
        loop {
            let mut done = false;
            
            // Try first receiver
            match $rx1.try_recv() {
                Ok($var1) => {
                    done = true;
                    $body1
                    break;
                }
                Err(_) => {}
            }
            
            // Try second receiver if first wasn't ready
            if !done {
                match $rx2.try_recv() {
                    Ok($var2) => {
                        done = true;
                        $body2
                        break;
                    }
                    Err(_) => {}
                }
            }
            
            // Execute default if nothing was ready and default is provided
            $(
                if !done {
                    $default_body
                    break;
                }
            )?
            
            // If no default case and nothing ready, yield and try again
            #[allow(unused_assignments)]
            if !done {
                $crate::routine::yield_now().await;
            }
        }
    }};
    
    // Single receiver with default
    (
        $var:ident = $rx:expr => $body:block,
        default => $default_body:block
    ) => {{
        match $rx.try_recv() {
            Ok($var) => $body,
            Err(_) => $default_body
        }
    }};
    
    // Single receiver with timeout
    (
        $var:ident = $rx:expr => $body:block,
        timeout($duration:expr) => $timeout_body:block
    ) => {{
        // For timeout, we need to use async polling
        use std::time::Instant;
        
        let start = Instant::now();
        let duration = $duration;
        
        // Create a future that resolves to the result
        async {
            loop {
                // Try to receive
                if let Ok($var) = $rx.try_recv() {
                    $body
                    break;
                }
                
                // Check timeout
                if start.elapsed() >= duration {
                    $timeout_body
                    break;
                }
                
                // Small yield to avoid busy waiting
                $crate::routine::yield_now().await;
            }
        }.await
    }};
}

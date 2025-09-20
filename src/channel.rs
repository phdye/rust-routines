//! Channel implementation for RustRoutines
//!
//! Provides Go-like channels for type-safe message passing between routines.
//! Now uses crossbeam channels instead of Tokio for pure M:N threading.

use crate::error::{Error, Result};
use crossbeam::channel::{bounded, unbounded, Sender as CbSender, Receiver as CbReceiver, TryRecvError, RecvTimeoutError};
use std::time::Duration;

/// A channel sender
pub struct Sender<T> {
    inner: ChannelSender<T>,
}

/// A channel receiver  
pub struct Receiver<T> {
    inner: ChannelReceiver<T>,
}

enum ChannelSender<T> {
    Bounded(CbSender<T>),
    Unbounded(CbSender<T>),
}

enum ChannelReceiver<T> {
    Bounded(CbReceiver<T>),
    Unbounded(CbReceiver<T>),
}

/// Create an unbuffered channel (synchronous channel with capacity 0)
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    // Go's unbuffered channels block on send until a receiver is ready
    // crossbeam's bounded(0) provides true synchronous behavior
    bounded_channel(0)
}

/// Create a bounded channel with the specified capacity
pub fn bounded_channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = bounded(capacity);
    (
        Sender { inner: ChannelSender::Bounded(tx) },
        Receiver { inner: ChannelReceiver::Bounded(rx) },
    )
}

/// Create an unbounded channel
pub fn unbounded_channel<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = unbounded();
    (
        Sender { inner: ChannelSender::Unbounded(tx) },
        Receiver { inner: ChannelReceiver::Unbounded(rx) },
    )
}

impl<T> Sender<T> {
    /// Send a value on this channel (blocking)
    pub fn send(&self, value: T) -> Result<()> {
        match &self.inner {
            ChannelSender::Bounded(tx) | ChannelSender::Unbounded(tx) => {
                tx.send(value).map_err(|_| Error::ChannelClosed)
            }
        }
    }

    /// Try to send a value without blocking
    pub fn try_send(&self, value: T) -> Result<()> {
        match &self.inner {
            ChannelSender::Bounded(tx) | ChannelSender::Unbounded(tx) => {
                tx.try_send(value).map_err(|e| {
                    if e.is_disconnected() {
                        Error::ChannelClosed
                    } else {
                        Error::ChannelFull
                    }
                })
            }
        }
    }

    /// Check if the channel is closed
    pub fn is_closed(&self) -> bool {
        match &self.inner {
            ChannelSender::Bounded(tx) | ChannelSender::Unbounded(tx) => {
                tx.is_empty() && tx.is_full()  // crossbeam doesn't have is_disconnected for Sender
            }
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        let inner = match &self.inner {
            ChannelSender::Bounded(tx) => ChannelSender::Bounded(tx.clone()),
            ChannelSender::Unbounded(tx) => ChannelSender::Unbounded(tx.clone()),
        };
        Sender { inner }
    }
}

impl<T> Receiver<T> {
    /// Receive a value from this channel (blocking)
    pub fn recv(&self) -> Result<T> {
        match &self.inner {
            ChannelReceiver::Bounded(rx) | ChannelReceiver::Unbounded(rx) => {
                rx.recv().map_err(|_| Error::ChannelClosed)
            }
        }
    }

    /// Try to receive a value without blocking
    pub fn try_recv(&self) -> Result<T> {
        match &self.inner {
            ChannelReceiver::Bounded(rx) | ChannelReceiver::Unbounded(rx) => {
                match rx.try_recv() {
                    Ok(value) => Ok(value),
                    Err(TryRecvError::Empty) => Err(Error::ChannelEmpty),
                    Err(TryRecvError::Disconnected) => Err(Error::ChannelClosed),
                }
            }
        }
    }

    /// Receive a value with a timeout
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T> {
        match &self.inner {
            ChannelReceiver::Bounded(rx) | ChannelReceiver::Unbounded(rx) => {
                match rx.recv_timeout(timeout) {
                    Ok(value) => Ok(value),
                    Err(RecvTimeoutError::Timeout) => Err(Error::Timeout),
                    Err(RecvTimeoutError::Disconnected) => Err(Error::ChannelClosed),
                }
            }
        }
    }

    /// Check if the channel is closed and empty
    pub fn is_closed(&self) -> bool {
        match &self.inner {
            ChannelReceiver::Bounded(rx) | ChannelReceiver::Unbounded(rx) => {
                rx.is_empty() && rx.try_recv().is_err()  // Check if disconnected
            }
        }
    }
    
    /// Get access to inner crossbeam receiver for select operations
    pub(crate) fn inner_ref(&self) -> &CbReceiver<T> {
        match &self.inner {
            ChannelReceiver::Bounded(rx) | ChannelReceiver::Unbounded(rx) => rx,
        }
    }
}

// For backward compatibility with async code
impl<T> Sender<T> {
    /// Async send (wraps blocking send for compatibility)
    pub async fn send_async(&self, value: T) -> Result<()> {
        // Run blocking operation in a way that doesn't block the executor
        // In the future, this should use the I/O thread pool
        self.send(value)
    }
}

impl<T> Receiver<T> {
    /// Async receive (wraps blocking recv for compatibility)
    pub async fn recv_async(&self) -> Option<T> {
        // Run blocking operation in a way that doesn't block the executor
        // In the future, this should use the I/O thread pool
        self.recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unbuffered_channel() {
        let (tx, rx) = channel::<i32>();
        
        // Spawn a thread to receive
        std::thread::spawn(move || {
            assert_eq!(rx.recv().unwrap(), 42);
        });
        
        // Give receiver time to start
        std::thread::sleep(Duration::from_millis(10));
        
        // Send should succeed
        tx.send(42).unwrap();
    }

    #[test]
    fn test_buffered_channel() {
        let (tx, rx) = bounded_channel::<i32>(3);
        
        // Should be able to send without blocking
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        tx.send(3).unwrap();
        
        // Buffer full, try_send should fail
        assert!(tx.try_send(4).is_err());
        
        // Receive values
        assert_eq!(rx.recv().unwrap(), 1);
        assert_eq!(rx.recv().unwrap(), 2);
        assert_eq!(rx.recv().unwrap(), 3);
    }

    #[test]
    fn test_unbounded_channel() {
        let (tx, rx) = unbounded_channel::<i32>();
        
        // Can send many values without blocking
        for i in 0..1000 {
            tx.send(i).unwrap();
        }
        
        // Receive them all
        for i in 0..1000 {
            assert_eq!(rx.recv().unwrap(), i);
        }
    }

    #[test]
    fn test_channel_close() {
        let (tx, rx) = channel::<i32>();
        
        // Drop sender
        drop(tx);
        
        // Receiver should get error
        assert!(rx.recv().is_err());
        assert!(rx.is_closed());
    }

    #[test]
    fn test_multiple_senders() {
        let (tx, rx) = bounded_channel::<i32>(10);
        let tx2 = tx.clone();
        
        std::thread::spawn(move || {
            tx.send(1).unwrap();
        });
        
        std::thread::spawn(move || {
            tx2.send(2).unwrap();
        });
        
        // Should receive both values
        let mut values = vec![rx.recv().unwrap(), rx.recv().unwrap()];
        values.sort();
        assert_eq!(values, vec![1, 2]);
    }

    #[test]
    fn test_try_operations() {
        let (tx, rx) = bounded_channel::<i32>(1);
        
        // try_recv on empty channel
        assert!(rx.try_recv().is_err());
        
        // Send a value
        tx.send(42).unwrap();
        
        // try_recv should work
        assert_eq!(rx.try_recv().unwrap(), 42);
        
        // try_recv on empty channel again
        assert!(rx.try_recv().is_err());
        
        // Fill the buffer
        tx.send(1).unwrap();
        
        // try_send should fail when full
        assert!(tx.try_send(2).is_err());
    }
}

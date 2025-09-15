//! Channel implementation for RustRoutines
//!
//! Provides Go-like channels for type-safe message passing between routines.

use crate::error::{Error, Result};
use tokio::sync::mpsc;

/// A channel sender
pub struct Sender<T> {
    inner: ChannelSender<T>,
}

/// A channel receiver  
pub struct Receiver<T> {
    inner: ChannelReceiver<T>,
}

enum ChannelSender<T> {
    Unbuffered(mpsc::UnboundedSender<T>),
    Buffered(mpsc::Sender<T>),
}

enum ChannelReceiver<T> {
    Unbuffered(mpsc::UnboundedReceiver<T>),
    Buffered(mpsc::Receiver<T>),
}

/// Create an unbuffered channel (actually uses a small buffer for Go-like semantics)
pub fn channel<T>() -> (Sender<T>, Receiver<T>) {
    // Go's unbuffered channels block on send until a receiver is ready
    // We simulate this with a buffer of 0 (synchronous channel)
    bounded_channel(0)
}

/// Create a bounded channel with the specified capacity
pub fn bounded_channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    if capacity == 0 {
        // Special case: unbuffered channel (synchronous)
        // Use buffer size of 1 for now as Tokio doesn't support true unbuffered
        let (tx, rx) = mpsc::channel(1);
        (
            Sender { inner: ChannelSender::Buffered(tx) },
            Receiver { inner: ChannelReceiver::Buffered(rx) },
        )
    } else {
        let (tx, rx) = mpsc::channel(capacity);
        (
            Sender { inner: ChannelSender::Buffered(tx) },
            Receiver { inner: ChannelReceiver::Buffered(rx) },
        )
    }
}

/// Create an unbounded channel
pub fn unbounded_channel<T>() -> (Sender<T>, Receiver<T>) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        Sender { inner: ChannelSender::Unbuffered(tx) },
        Receiver { inner: ChannelReceiver::Unbuffered(rx) },
    )
}

impl<T> Sender<T> {
    /// Send a value on the channel
    pub async fn send(&self, value: T) -> Result<()> {
        match &self.inner {
            ChannelSender::Unbuffered(tx) => {
                tx.send(value)
                    .map_err(|_| Error::ChannelClosed)
            }
            ChannelSender::Buffered(tx) => {
                tx.send(value).await
                    .map_err(|_| Error::ChannelClosed)
            }
        }
    }
    
    /// Try to send a value without blocking
    pub fn try_send(&self, value: T) -> Result<()> {
        match &self.inner {
            ChannelSender::Unbuffered(tx) => {
                tx.send(value)
                    .map_err(|_| Error::ChannelClosed)
            }
            ChannelSender::Buffered(tx) => {
                tx.try_send(value)
                    .map_err(|e| match e {
                        mpsc::error::TrySendError::Closed(_) => Error::ChannelClosed,
                        mpsc::error::TrySendError::Full(_) => Error::SendError { 
                            reason: "Channel buffer is full".to_string() 
                        },
                    })
            }
        }
    }
    
    /// Check if the channel is closed
    pub fn is_closed(&self) -> bool {
        match &self.inner {
            ChannelSender::Unbuffered(tx) => tx.is_closed(),
            ChannelSender::Buffered(tx) => tx.is_closed(),
        }
    }
    
    /// Get the channel capacity (returns None for unbounded channels)
    pub fn capacity(&self) -> Option<usize> {
        match &self.inner {
            ChannelSender::Unbuffered(_) => None,
            ChannelSender::Buffered(tx) => Some(tx.capacity()),
        }
    }
}

impl<T> Receiver<T> {
    /// Receive a value from the channel
    pub async fn recv(&mut self) -> Result<T> {
        match &mut self.inner {
            ChannelReceiver::Unbuffered(rx) => {
                rx.recv().await
                    .ok_or(Error::ChannelClosed)
            }
            ChannelReceiver::Buffered(rx) => {
                rx.recv().await
                    .ok_or(Error::ChannelClosed)
            }
        }
    }
    
    /// Try to receive a value without blocking
    pub fn try_recv(&mut self) -> Result<T> {
        match &mut self.inner {
            ChannelReceiver::Unbuffered(rx) => {
                match rx.try_recv() {
                    Ok(value) => Ok(value),
                    Err(mpsc::error::TryRecvError::Empty) => {
                        Err(Error::RecvError { reason: "Channel is empty".to_string() })
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        Err(Error::ChannelClosed)
                    }
                }
            }
            ChannelReceiver::Buffered(rx) => {
                match rx.try_recv() {
                    Ok(value) => Ok(value),
                    Err(mpsc::error::TryRecvError::Empty) => {
                        Err(Error::RecvError { reason: "Channel is empty".to_string() })
                    }
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        Err(Error::ChannelClosed)
                    }
                }
            }
        }
    }
    
    /// Close the receiver, preventing any further receives
    pub fn close(&mut self) {
        match &mut self.inner {
            ChannelReceiver::Unbuffered(rx) => rx.close(),
            ChannelReceiver::Buffered(rx) => rx.close(),
        }
    }
    
    /// Check if the channel is empty
    pub fn is_empty(&self) -> bool {
        match &self.inner {
            ChannelReceiver::Unbuffered(rx) => rx.is_empty(),
            ChannelReceiver::Buffered(rx) => rx.is_empty(),
        }
    }
    
    /// Check if the channel is closed
    pub fn is_closed(&self) -> bool {
        match &self.inner {
            ChannelReceiver::Unbuffered(rx) => rx.is_closed(),
            ChannelReceiver::Buffered(rx) => rx.is_closed(),
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: match &self.inner {
                ChannelSender::Unbuffered(tx) => ChannelSender::Unbuffered(tx.clone()),
                ChannelSender::Buffered(tx) => ChannelSender::Buffered(tx.clone()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_unbuffered_channel() {
        let (tx, mut rx) = channel::<i32>();
        
        // Spawn a task to receive
        let handle = tokio::spawn(async move {
            rx.recv().await.unwrap()
        });
        
        // Small delay to ensure receiver is ready
        tokio::time::sleep(Duration::from_millis(10)).await;
        
        // Send should complete when receiver is ready
        tx.send(42).await.unwrap();
        
        let value = handle.await.unwrap();
        assert_eq!(value, 42);
    }
    
    #[tokio::test]
    async fn test_buffered_channel() {
        let (tx, mut rx) = bounded_channel::<i32>(3);
        
        // Should be able to send up to capacity without blocking
        tx.send(1).await.unwrap();
        tx.send(2).await.unwrap();
        tx.send(3).await.unwrap();
        
        // Should receive in order
        assert_eq!(rx.recv().await.unwrap(), 1);
        assert_eq!(rx.recv().await.unwrap(), 2);
        assert_eq!(rx.recv().await.unwrap(), 3);
    }
    
    #[tokio::test]
    async fn test_unbounded_channel() {
        let (tx, mut rx) = unbounded_channel::<String>();
        
        // Can send many values without blocking
        for i in 0..100 {
            tx.send(format!("msg-{}", i)).await.unwrap();
        }
        
        // Receive first few
        assert_eq!(rx.recv().await.unwrap(), "msg-0");
        assert_eq!(rx.recv().await.unwrap(), "msg-1");
    }
    
    #[tokio::test]
    async fn test_channel_close() {
        let (tx, mut rx) = channel::<i32>();
        
        tx.send(1).await.unwrap();
        drop(tx); // Close sender
        
        assert_eq!(rx.recv().await.unwrap(), 1);
        assert!(rx.recv().await.is_err()); // Should get ChannelClosed
    }
    
    #[tokio::test]
    async fn test_try_operations() {
        let (tx, mut rx) = bounded_channel::<i32>(2);
        
        // Try receive on empty channel
        assert!(rx.try_recv().is_err());
        
        // Fill the buffer
        tx.try_send(1).unwrap();
        tx.try_send(2).unwrap();
        
        // Buffer full, try_send should fail
        assert!(tx.try_send(3).is_err());
        
        // Try receive should work
        assert_eq!(rx.try_recv().unwrap(), 1);
    }
    
    #[tokio::test]
    async fn test_multiple_senders() {
        let (tx1, mut rx) = channel::<i32>();
        let tx2 = tx1.clone();
        
        tokio::spawn(async move {
            tx1.send(1).await.unwrap();
        });
        
        tokio::spawn(async move {
            tx2.send(2).await.unwrap();
        });
        
        // Should receive both values (order may vary)
        let mut values = vec![
            rx.recv().await.unwrap(),
            rx.recv().await.unwrap(),
        ];
        values.sort();
        assert_eq!(values, vec![1, 2]);
    }
}

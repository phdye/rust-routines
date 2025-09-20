//! Timer wheel implementation for efficient delay and timeout management
//!
//! This module provides a hierarchical timer wheel for managing routine delays
//! without using Tokio. Based on the classic Varghese & Lauck timer wheel design.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use crossbeam::channel::{bounded, Sender, Receiver};
use once_cell::sync::Lazy;

/// Timer ID for tracking individual timers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimerId(usize);

/// A timer entry in the wheel
struct TimerEntry {
    id: TimerId,
    deadline: Instant,
    callback: Box<dyn FnOnce() + Send + 'static>,
}

/// Hierarchical timer wheel for efficient O(1) timer operations
pub struct TimerWheel {
    /// Slots in the timer wheel (each slot is 10ms)
    slots: Vec<Mutex<Vec<TimerEntry>>>,
    /// Current slot index
    current_slot: AtomicUsize,
    /// Number of slots in the wheel
    num_slots: usize,
    /// Slot duration in milliseconds
    slot_duration_ms: u64,
    /// Timer ID counter
    next_timer_id: AtomicUsize,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
    /// Worker thread handle
    worker_thread: Option<thread::JoinHandle<()>>,
    /// Channel for adding new timers
    timer_sender: Sender<TimerEntry>,
    timer_receiver: Arc<Mutex<Receiver<TimerEntry>>>,
}

impl TimerWheel {
    /// Create a new timer wheel with default configuration
    pub fn new() -> Arc<Self> {
        Self::with_config(256, 10) // 256 slots, 10ms each = 2.56 second wheel
    }
    
    /// Create a timer wheel with custom configuration
    pub fn with_config(num_slots: usize, slot_duration_ms: u64) -> Arc<Self> {
        let mut slots = Vec::with_capacity(num_slots);
        for _ in 0..num_slots {
            slots.push(Mutex::new(Vec::new()));
        }
        
        let (timer_sender, timer_receiver) = bounded(1024);
        let shutdown = Arc::new(AtomicBool::new(false));
        
        let wheel = Arc::new(TimerWheel {
            slots,
            current_slot: AtomicUsize::new(0),
            num_slots,
            slot_duration_ms,
            next_timer_id: AtomicUsize::new(0),
            shutdown: shutdown.clone(),
            worker_thread: None,
            timer_sender,
            timer_receiver: Arc::new(Mutex::new(timer_receiver)),
        });
        
        // Start the worker thread
        let wheel_clone = Arc::clone(&wheel);
        thread::Builder::new()
            .name("timer-wheel".to_string())
            .spawn(move || {
                wheel_clone.worker_loop();
            })
            .expect("Failed to spawn timer wheel thread");
        
        wheel
    }
    
    /// Schedule a timer to fire after the specified duration
    pub fn schedule(&self, duration: Duration, callback: impl FnOnce() + Send + 'static) -> TimerId {
        let id = TimerId(self.next_timer_id.fetch_add(1, Ordering::Relaxed));
        let deadline = Instant::now() + duration;
        
        let entry = TimerEntry {
            id,
            deadline,
            callback: Box::new(callback),
        };
        
        // Send to worker thread for processing
        let _ = self.timer_sender.send(entry);
        
        id
    }
    
    /// Cancel a scheduled timer
    pub fn cancel(&self, timer_id: TimerId) -> bool {
        // Search through all slots for the timer
        for slot in &self.slots {
            let mut entries = slot.lock();
            if let Some(pos) = entries.iter().position(|e| e.id == timer_id) {
                entries.remove(pos);
                return true;
            }
        }
        false
    }
    
    /// Worker thread main loop
    fn worker_loop(self: &Arc<Self>) {
        let tick_duration = Duration::from_millis(self.slot_duration_ms);
        let mut last_tick = Instant::now();
        
        while !self.shutdown.load(Ordering::Relaxed) {
            // Process new timer requests
            {
                let receiver = self.timer_receiver.lock();
                while let Ok(entry) = receiver.try_recv() {
                    self.insert_timer(entry);
                }
            }
            
            // Check if we need to advance the wheel
            let now = Instant::now();
            while last_tick + tick_duration <= now {
                self.tick();
                last_tick += tick_duration;
            }
            
            // Sleep until next tick
            let sleep_duration = (last_tick + tick_duration).saturating_duration_since(now);
            if sleep_duration > Duration::from_micros(100) {
                thread::sleep(sleep_duration);
            }
        }
    }
    
    /// Insert a timer into the appropriate slot
    fn insert_timer(&self, entry: TimerEntry) {
        let now = Instant::now();
        let delay_ms = entry.deadline.saturating_duration_since(now).as_millis() as u64;
        
        // Calculate which slot this timer belongs in
        let ticks_away = (delay_ms / self.slot_duration_ms) as usize;
        let current = self.current_slot.load(Ordering::Relaxed);
        let slot_index = (current + ticks_away) % self.num_slots;
        
        // Insert into the appropriate slot
        let mut slot = self.slots[slot_index].lock();
        slot.push(entry);
    }
    
    /// Advance the timer wheel by one tick
    fn tick(&self) {
        let current = self.current_slot.load(Ordering::Relaxed);
        let now = Instant::now();
        
        // Process all timers in the current slot
        let mut expired_timers = Vec::new();
        {
            let mut slot = self.slots[current].lock();
            let mut i = 0;
            while i < slot.len() {
                if slot[i].deadline <= now {
                    expired_timers.push(slot.remove(i));
                } else {
                    i += 1;
                }
            }
        }
        
        // Execute expired timer callbacks
        for timer in expired_timers {
            (timer.callback)();
        }
        
        // Advance to next slot
        self.current_slot.store((current + 1) % self.num_slots, Ordering::Relaxed);
    }
    
    /// Shutdown the timer wheel
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

/// Global timer wheel instance
pub static GLOBAL_TIMER_WHEEL: Lazy<Arc<TimerWheel>> = Lazy::new(|| {
    TimerWheel::new()
});

/// Schedule a delay for the current routine
pub fn delay(duration: Duration) {
    let (tx, rx) = bounded(0);
    
    GLOBAL_TIMER_WHEEL.schedule(duration, move || {
        let _ = tx.send(());
    });
    
    // Block until timer fires
    let _ = rx.recv();
}

/// Schedule a timeout operation
pub fn timeout<T, F>(duration: Duration, f: F) -> Result<T, ()>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = bounded(1);
    
    // Run the operation in parallel
    thread::spawn(move || {
        let result = f();
        let _ = tx.send(result);
    });
    
    // Wait with timeout
    match rx.recv_timeout(duration) {
        Ok(result) => Ok(result),
        Err(_) => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    
    #[test]
    fn test_timer_wheel_creation() {
        let wheel = TimerWheel::new();
        assert_eq!(wheel.num_slots, 256);
        assert_eq!(wheel.slot_duration_ms, 10);
    }
    
    #[test]
    fn test_schedule_timer() {
        let wheel = TimerWheel::new();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();
        
        wheel.schedule(Duration::from_millis(50), move || {
            fired_clone.store(true, Ordering::Relaxed);
        });
        
        thread::sleep(Duration::from_millis(100));
        assert!(fired.load(Ordering::Relaxed));
    }
    
    #[test]
    fn test_cancel_timer() {
        let wheel = TimerWheel::new();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();
        
        let timer_id = wheel.schedule(Duration::from_millis(100), move || {
            fired_clone.store(true, Ordering::Relaxed);
        });
        
        // Cancel before it fires
        thread::sleep(Duration::from_millis(50));
        assert!(wheel.cancel(timer_id));
        
        // Wait to ensure it doesn't fire
        thread::sleep(Duration::from_millis(100));
        assert!(!fired.load(Ordering::Relaxed));
    }
    
    #[test]
    fn test_global_delay() {
        let start = Instant::now();
        delay(Duration::from_millis(50));
        let elapsed = start.elapsed();
        
        assert!(elapsed >= Duration::from_millis(50));
        assert!(elapsed < Duration::from_millis(100));
    }
    
    #[test]
    fn test_timeout_success() {
        let result = timeout(Duration::from_millis(100), || {
            thread::sleep(Duration::from_millis(10));
            42
        });
        
        assert_eq!(result, Ok(42));
    }
    
    #[test]
    fn test_timeout_failure() {
        let result = timeout(Duration::from_millis(10), || {
            thread::sleep(Duration::from_millis(100));
            42
        });
        
        assert_eq!(result, Err(()));
    }
}

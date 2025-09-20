//! Work-stealing queue implementation
//!
//! Provides lock-free work-stealing deques for efficient task distribution.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use crossbeam::deque::{Worker as DequeWorker, Steal};
pub use crossbeam::deque::Stealer;
use std::fmt::Debug;

/// A task that can be executed by a worker
pub struct Task {
    /// The actual work to be done
    work: Box<dyn FnOnce() + Send + 'static>,
    /// Task priority (lower values = higher priority)
    priority: usize,
    /// Task ID for debugging
    id: usize,
}

impl Task {
    /// Create a new task
    pub fn new(work: Box<dyn FnOnce() + Send + 'static>) -> Self {
        static TASK_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);
        
        Self {
            work,
            priority: 0,
            id: TASK_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
        }
    }
    
    /// Create a new task with priority
    pub fn with_priority(work: Box<dyn FnOnce() + Send + 'static>, priority: usize) -> Self {
        static TASK_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);
        
        Self {
            work,
            priority,
            id: TASK_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
        }
    }
    
    /// Execute the task
    pub fn run(self) {
        (self.work)();
    }
    
    /// Get task ID
    pub fn id(&self) -> usize {
        self.id
    }
    
    /// Get task priority
    pub fn priority(&self) -> usize {
        self.priority
    }
    
    /// Create a dummy task for testing
    #[cfg(test)]
    pub fn dummy() -> Self {
        Self::new(Box::new(|| {}))
    }
}

impl Clone for Task {
    fn clone(&self) -> Self {
        // For testing purposes - creates a new task with same priority
        // In production, tasks shouldn't be cloned
        Self {
            work: Box::new(|| {}),
            priority: self.priority,
            id: self.id,
        }
    }
}

impl Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
            .field("id", &self.id)
            .field("priority", &self.priority)
            .finish()
    }
}

/// Work queue that supports work-stealing
pub struct WorkQueue {
    /// The worker side of the deque (for the owning thread)
    worker: DequeWorker<Task>,
    /// The stealer side (for other threads to steal from)
    /// Currently stored for future self-monitoring features
    #[allow(dead_code)]
    stealer: Stealer<Task>,
}

impl WorkQueue {
    /// Create a new work queue
    pub fn new() -> (Self, Stealer<Task>) {
        let worker = DequeWorker::new_fifo();
        let stealer = worker.stealer();
        let stealer_clone = stealer.clone();
        
        (
            Self {
                worker,
                stealer,
            },
            stealer_clone,
        )
    }
    
    /// Push a task to the local end of the queue
    pub fn push(&self, task: Task) {
        self.worker.push(task);
    }
    
    /// Pop a task from the local end of the queue
    pub fn pop(&self) -> Option<Task> {
        self.worker.pop()
    }
    
    /// Try to steal a task from another queue
    pub fn steal_from(&self, stealer: &Stealer<Task>) -> Option<Task> {
        loop {
            match stealer.steal() {
                Steal::Success(task) => return Some(task),
                Steal::Empty => return None,
                Steal::Retry => continue,
            }
        }
    }
    
    /// Try to steal a batch of tasks from another queue
    pub fn steal_batch_from(&self, stealer: &Stealer<Task>, max_steal: usize) -> Vec<Task> {
        let mut stolen = Vec::new();
        
        // Use steal_batch_and_pop for better performance
        loop {
            match stealer.steal_batch(&self.worker) {
                Steal::Success(()) => {
                    // Successfully stole some tasks, now pop them
                    for _ in 0..max_steal {
                        if let Some(task) = self.worker.pop() {
                            stolen.push(task);
                        } else {
                            break;
                        }
                    }
                    return stolen;
                }
                Steal::Empty => return stolen,
                Steal::Retry => continue,
            }
        }
    }
    
    /// Check if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.worker.is_empty()
    }
    
    /// Get the approximate length of the queue
    pub fn len(&self) -> usize {
        self.worker.len()
    }
}

impl Default for WorkQueue {
    fn default() -> Self {
        let (queue, _) = Self::new();
        queue
    }
}

/// Global work queue for distributing initial tasks
pub struct GlobalQueue {
    /// Shared queue for tasks that haven't been assigned to a worker yet
    queue: Arc<crossbeam::queue::SegQueue<Task>>,
    /// Number of pending tasks
    pending_count: Arc<AtomicUsize>,
}

impl GlobalQueue {
    /// Create a new global queue
    pub fn new() -> Self {
        Self {
            queue: Arc::new(crossbeam::queue::SegQueue::new()),
            pending_count: Arc::new(AtomicUsize::new(0)),
        }
    }
    
    /// Push a task to the global queue
    pub fn push(&self, task: Task) {
        self.queue.push(task);
        self.pending_count.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Pop a task from the global queue
    pub fn pop(&self) -> Option<Task> {
        let task = self.queue.pop();
        if task.is_some() {
            self.pending_count.fetch_sub(1, Ordering::Relaxed);
        }
        task
    }
    
    /// Try to pop multiple tasks from the global queue
    pub fn pop_batch(&self, max_tasks: usize) -> Vec<Task> {
        let mut tasks = Vec::new();
        
        for _ in 0..max_tasks {
            if let Some(task) = self.pop() {
                tasks.push(task);
            } else {
                break;
            }
        }
        
        tasks
    }
    
    /// Check if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.pending_count.load(Ordering::Relaxed) == 0
    }
    
    /// Get the number of pending tasks
    pub fn len(&self) -> usize {
        self.pending_count.load(Ordering::Relaxed)
    }
}

impl Default for GlobalQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    
    #[test]
    fn test_task_creation() {
        let executed = Arc::new(AtomicBool::new(false));
        let executed_clone = Arc::clone(&executed);
        
        let task = Task::new(Box::new(move || {
            executed_clone.store(true, Ordering::Release);
        }));
        
        assert_eq!(task.priority(), 0);
        task.run();
        assert!(executed.load(Ordering::Acquire));
    }
    
    #[test]
    fn test_work_queue_push_pop() {
        let (queue, _stealer) = WorkQueue::new();
        
        let task1 = Task::new(Box::new(|| {}));
        let task2 = Task::new(Box::new(|| {}));
        
        queue.push(task1);
        queue.push(task2);
        
        assert!(!queue.is_empty());
        assert_eq!(queue.len(), 2);
        
        let _t1 = queue.pop().unwrap();
        let _t2 = queue.pop().unwrap();
        assert!(queue.pop().is_none());
    }
    
    #[test]
    fn test_work_stealing() {
        let (queue1, stealer1) = WorkQueue::new();
        let (queue2, _stealer2) = WorkQueue::new();
        
        // Push tasks to queue1
        for _ in 0..10 {
            queue1.push(Task::new(Box::new(|| {})));
        }
        
        // Steal from queue1 to queue2
        let stolen = queue2.steal_from(&stealer1);
        assert!(stolen.is_some());
        
        // Steal batch
        let batch = queue2.steal_batch_from(&stealer1, 5);
        assert!(!batch.is_empty());
    }
    
    #[test]
    fn test_global_queue() {
        let global_queue = GlobalQueue::new();
        
        assert!(global_queue.is_empty());
        
        for i in 0..5 {
            global_queue.push(Task::with_priority(Box::new(move || {
                println!("Task {}", i);
            }), i));
        }
        
        assert_eq!(global_queue.len(), 5);
        
        let batch = global_queue.pop_batch(3);
        assert_eq!(batch.len(), 3);
        assert_eq!(global_queue.len(), 2);
    }
}

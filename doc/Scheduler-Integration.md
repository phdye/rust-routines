# Handoff Document: M:N Scheduler Integration for RustRoutines

**Date**: September 15, 2025  
**From**: Claude (Analysis Phase)  
**To**: Claude (Implementation Phase)  
**Project**: rust-routines - GoRoutine Implementation in Rust  
**Priority**: Critical Architecture Fix  

## Executive Summary

The RustRoutines project has successfully implemented both halves of a Go-like concurrency system but they are currently **disconnected**. We have excellent channel/select semantics AND a working M:N work-stealing scheduler, but the public API uses Tokio instead of our custom scheduler. This document provides the roadmap to integrate them for true GoRoutine behavior.

## Current State Analysis

### ✅ What Works Perfectly
1. **M:N Work-Stealing Scheduler** (`src/scheduler/`)
   - 4-worker thread pool with work-stealing
   - Lock-free deques using crossbeam
   - Proper task distribution and load balancing
   - Demonstrated working in `examples/phase2_demo.rs`

2. **Go-like Channel Semantics** (`src/channel.rs`)
   - Unbuffered, buffered, and unbounded channels
   - Proper blocking/non-blocking behavior
   - Channel closing and error propagation
   - Perfect Go fidelity in tests

3. **Select Implementation** (`src/select.rs`)
   - Fair selection with randomization
   - Default cases and timeouts
   - Multi-channel operations
   - Matches Go select semantics

### ❌ Critical Problem
**The routine API (`src/routine.rs`) uses `tokio::spawn()` instead of our custom scheduler.**

```rust
// CURRENT - Wrong approach
pub fn spawn<F, T>(future: F) -> RoutineHandle<T> {
    let handle = tokio::spawn(future);  // ❌ Bypasses our scheduler
    RoutineHandle { inner: handle }
}
```

This means users get Go-like syntax but **NOT** Go-like execution characteristics (M:N threading, work-stealing, etc.).

## Integration Architecture

### Target Design
```
User Code
    ↓
routine::spawn() / go!() macro
    ↓
Future → Task Conversion
    ↓
Global M:N Scheduler (work-stealing)
    ↓
Worker Threads (N OS threads)
    ↓
Task Execution (M tasks)
```

### Key Integration Points

1. **Future-to-Task Bridge**: Convert async Futures to schedulable Tasks
2. **Handle Management**: Provide async-compatible handles for join operations
3. **Runtime Coordination**: Ensure channels/select work with custom scheduler
4. **Error Propagation**: Maintain panic safety and error handling

## Implementation Plan

### Phase A: Foundation (2-3 hours)

#### A1: Async Runtime Bridge
**File**: `src/bridge.rs` (new file)

```rust
use std::future::Future;
use std::task::{Context, Poll, Waker};
use std::pin::Pin;
use futures::channel::oneshot;
use crate::scheduler::{Task, GLOBAL_SCHEDULER};

/// Converts a Future into a Task that can be scheduled
pub struct FutureTask<T> {
    future: Pin<Box<dyn Future<Output = T> + Send + 'static>>,
    result_sender: Option<oneshot::Sender<T>>,
}

impl<T: Send + 'static> FutureTask<T> {
    pub fn new(
        future: impl Future<Output = T> + Send + 'static,
        sender: oneshot::Sender<T>
    ) -> Self {
        Self {
            future: Box::pin(future),
            result_sender: Some(sender),
        }
    }
    
    pub fn into_task(mut self) -> Task {
        Task::new(Box::new(move || {
            // Create a minimal executor context for the future
            let mut executor = LocalExecutor::new();
            let result = executor.block_on(self.future);
            
            if let Some(sender) = self.result_sender.take() {
                let _ = sender.send(result);
            }
        }))
    }
}

/// Minimal single-threaded executor for running futures in tasks
struct LocalExecutor {
    // Implementation details...
}
```

#### A2: Updated RoutineHandle
**File**: `src/routine.rs` (modify existing)

```rust
use futures::channel::oneshot;
use crate::bridge::FutureTask;
use crate::scheduler::GLOBAL_SCHEDULER;

/// A handle to a spawned routine
pub struct RoutineHandle<T> {
    receiver: oneshot::Receiver<T>,
    task_id: Option<usize>, // For debugging/monitoring
}

impl<T: Send + 'static> RoutineHandle<T> {
    /// Wait for the routine to complete
    pub async fn join(self) -> Result<T> {
        self.receiver.await
            .map_err(|_| Error::RuntimeError {
                reason: "Routine was cancelled or panicked".to_string()
            })
    }
    
    /// Abort the routine (best effort)
    pub fn abort(&self) {
        // Note: Aborting scheduled tasks is complex
        // For initial implementation, document as limitation
    }
}

/// Spawn a new routine on the M:N scheduler
pub fn spawn<F, T>(future: F) -> RoutineHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (sender, receiver) = oneshot::channel();
    let future_task = FutureTask::new(future, sender);
    let task = future_task.into_task();
    
    // Schedule on our M:N scheduler instead of Tokio
    let task_id = task.id();
    GLOBAL_SCHEDULER.schedule(task)
        .expect("Failed to schedule routine");
    
    RoutineHandle {
        receiver,
        task_id: Some(task_id),
    }
}
```

### Phase B: Executor Integration (3-4 hours)

#### B1: Minimal Future Executor
**File**: `src/executor.rs` (new file)

The challenge is running Futures in the task context. We need a minimal executor:

```rust
use std::future::Future;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;

/// A minimal executor for running futures in scheduled tasks
pub struct TaskExecutor {
    wake_queue: Arc<Mutex<VecDeque<Waker>>>,
}

impl TaskExecutor {
    pub fn new() -> Self {
        Self {
            wake_queue: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
    
    /// Block on a future within a task context
    pub fn block_on<F: Future>(&mut self, mut future: Pin<Box<F>>) -> F::Output {
        let waker = self.create_waker();
        let mut context = Context::from_waker(&waker);
        
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(result) => return result,
                Poll::Pending => {
                    // Process any wake-ups
                    let mut queue = self.wake_queue.lock().unwrap();
                    if queue.is_empty() {
                        // If no wake-ups pending, yield to scheduler
                        drop(queue);
                        std::thread::yield_now();
                    } else {
                        queue.clear(); // Clear wake-ups and try again
                    }
                }
            }
        }
    }
    
    fn create_waker(&self) -> Waker {
        // Implementation for creating a waker that adds to wake_queue
        // Details in full implementation...
    }
}
```

#### B2: Global Runtime Integration
**File**: `src/runtime.rs` (modify existing)

```rust
use once_cell::sync::Lazy;
use crate::scheduler::{Scheduler, SchedulerConfig, GLOBAL_SCHEDULER};

/// Initialize the global runtime and scheduler together
pub fn init_global_runtime(config: RuntimeConfig) -> Result<()> {
    // Start the M:N scheduler
    let scheduler_config = SchedulerConfig {
        num_workers: config.worker_threads,
        thread_name_prefix: config.thread_name.clone(),
        ..Default::default()
    };
    
    // The GLOBAL_SCHEDULER is already initialized via Lazy
    // Verify it's working
    let stats = GLOBAL_SCHEDULER.stats();
    info!("Global scheduler initialized with {} workers", 
          GLOBAL_SCHEDULER.num_workers());
    
    Ok(())
}
```

### Phase C: Testing & Validation (2-3 hours)

#### C1: Integration Tests
**File**: `tests/scheduler_integration.rs` (new file)

```rust
use rust_routines::prelude::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn test_routines_use_custom_scheduler() {
    let counter = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    
    // Spawn many routines to force scheduler usage
    for _ in 0..100 {
        let counter_clone = Arc::clone(&counter);
        let handle = go!(async move {
            counter_clone.fetch_add(1, Ordering::Relaxed);
            tokio::task::yield_now().await; // Ensure async context
        });
        handles.push(handle);
    }
    
    // Use tokio runtime only to wait for completion
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for handle in handles {
            handle.join().await.unwrap();
        }
    });
    
    assert_eq!(counter.load(Ordering::Relaxed), 100);
    
    // Verify scheduler was used
    let stats = rust_routines::scheduler::scheduler_stats();
    assert!(stats.tasks_scheduled.load(Ordering::Relaxed) >= 100);
}
```

#### C2: Performance Validation
**File**: `benches/integration_benchmark.rs` (new file)

Compare performance before/after integration to ensure no regression.

#### C3: Update Examples
**File**: `examples/integrated_demo.rs` (new file)

Demonstrate that the same examples work with the integrated scheduler.

## Critical Implementation Details

### 1. Future Execution Context
**Challenge**: Futures expect to run in an async runtime context, but our tasks run in regular threads.

**Solution**: Minimal executor that can poll futures to completion within task context.

### 2. Waker Implementation
**Challenge**: Futures need proper Waker implementation for async operations.

**Solution**: Custom waker that integrates with our scheduler's wake-up mechanisms.

### 3. Channel Integration
**Challenge**: Channels currently use Tokio's mpsc channels.

**Analysis**: This is actually fine! The channels work correctly regardless of what spawns the tasks that use them. No changes needed.

### 4. Select Integration
**Challenge**: Select operations might need scheduler awareness.

**Analysis**: Select currently uses `tokio::select!` macro, which should work correctly with our executor.

## Testing Strategy

### Unit Tests
- [ ] Future-to-Task conversion
- [ ] RoutineHandle join/abort operations
- [ ] TaskExecutor with simple futures
- [ ] Error propagation through the stack

### Integration Tests
- [ ] Routines actually run on custom scheduler (verify via statistics)
- [ ] Channel operations work with scheduled routines
- [ ] Select operations work with scheduled routines
- [ ] Performance is maintained or improved

### Validation Tests
- [ ] All existing tests continue to pass
- [ ] Phase2 demo works with integrated scheduler
- [ ] Memory usage remains bounded
- [ ] No deadlocks under high concurrency

## Potential Risks & Mitigations

### Risk 1: Future Execution Complexity
**Risk**: Futures might not poll correctly in our custom executor.
**Mitigation**: Start with simple futures, gradually add complexity. Have fallback to Tokio for debugging.

### Risk 2: Performance Regression
**Risk**: Custom executor might be slower than Tokio.
**Mitigation**: Benchmark extensively. Optimize the hot path in TaskExecutor.

### Risk 3: Async Ecosystem Compatibility
**Risk**: Some async libraries might not work with our executor.
**Mitigation**: Document limitations. Consider hybrid approach where I/O still uses Tokio.

## Success Criteria

### Phase A Complete When:
- [ ] `RoutineHandle` uses oneshot channels instead of JoinHandle
- [ ] `spawn()` function schedules on GLOBAL_SCHEDULER
- [ ] Basic integration tests pass

### Phase B Complete When:
- [ ] TaskExecutor can run simple async functions
- [ ] All existing unit tests pass
- [ ] Phase2 demo shows tasks running on custom scheduler

### Phase C Complete When:
- [ ] All integration tests pass
- [ ] Performance benchmarks meet targets
- [ ] Documentation updated to reflect true M:N threading

## Files to Modify

### New Files:
- `src/bridge.rs` - Future-to-Task conversion
- `src/executor.rs` - Minimal future executor
- `tests/scheduler_integration.rs` - Integration tests
- `examples/integrated_demo.rs` - Demo of integrated system

### Modified Files:
- `src/routine.rs` - Update spawn() to use scheduler
- `src/runtime.rs` - Global runtime coordination
- `src/lib.rs` - Export new modules
- `README.md` - Update to reflect true M:N threading

## Timeline Estimate

- **Phase A**: 2-3 hours (foundation)
- **Phase B**: 3-4 hours (executor integration)  
- **Phase C**: 2-3 hours (testing & validation)
- **Total**: 7-10 hours

## Next Steps

1. **START HERE**: Create `src/bridge.rs` with FutureTask implementation
2. **Modify** `src/routine.rs` to use the scheduler instead of tokio::spawn
3. **Test** with simple futures first, then add complexity
4. **Validate** that all existing tests still pass
5. **Benchmark** to ensure performance is maintained

## Reference Materials

- **Technical Design**: `doc/Technical-Design.md` - Architecture overview
- **Implementation Plan**: `doc/Implementation-Plan.md` - Phase 2 requirements  
- **Working Scheduler**: `src/scheduler/` - Proven M:N implementation
- **Working API**: `src/routine.rs`, `src/channel.rs` - Go-like semantics

---

**Remember**: The hard work is done - both pieces work perfectly in isolation. This is primarily a "wiring" exercise to connect them together. Focus on the simplest possible integration first, then optimize.

**Critical Success Factor**: After integration, `examples/phase2_demo.rs` should show the same task statistics but with routines scheduled via the public API instead of direct scheduler calls.
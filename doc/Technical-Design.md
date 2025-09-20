# Technical Design - RustRoutines

**Go-like Concurrency Primitives for Rust**

## Overview

RustRoutines is a Rust library that provides Go-style concurrency primitives, including goroutines (routines), channels, and select functionality. The library aims to bring the simplicity and elegance of Go's concurrency model to Rust while maintaining Rust's safety guarantees and performance characteristics.

## Architecture

### Core Components

```
┌─────────────────────────────────────────────────────┐
│                  RustRoutines                       │
├─────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │
│  │   Routines  │  │  Channels   │  │   Select    │  │
│  │             │  │             │  │             │  │
│  │ • spawn()   │  │ • channel() │  │ • select()  │  │
│  │ • go!()     │  │ • send()    │  │ • recv()    │  │
│  │ • join()    │  │ • recv()    │  │ • timeout() │  │
│  └─────────────┘  └─────────────┘  └─────────────┘  │
├─────────────────────────────────────────────────────┤
│              M:N Scheduler Runtime                  │
│  ┌─────────────────────────────────────────────────┐│
│  │ • Work-stealing scheduler (CPU pool)            ││
│  │ • I/O thread pool (blocking operations)         ││
│  │ • Task-to-routine bridge                        ││
│  │ • Statistics collection                         ││
│  │ • Timer wheel for delays                        ││
│  └─────────────────────────────────────────────────┘│
├─────────────────────────────────────────────────────┤
│               Foundation Layer                      │
│  ┌─────────────────────────────────────────────────┐│
│  │ • Crossbeam for lock-free data structures       ││
│  │ • Parking_lot for synchronization               ││
│  │ • Standard library for I/O                      ││
│  │ • Platform-specific APIs (io_uring/IOCP/kqueue) ││
│  └─────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────┘
```

## Module Design

### 1. Routine Module (`routine.rs`)

**Purpose**: Provides goroutine-like lightweight thread abstractions.

**Key Components**:
- `RoutineHandle<T>`: Handle to a spawned routine with join/abort capabilities
- `spawn()`: Creates a new routine on the M:N scheduler
- `go()`: Alias for spawn with Go-like naming
- `yield_now()`: Cooperative yielding primitive
- `sleep()`: Blocking sleep that parks the routine

**Implementation Strategy**:
- Direct integration with M:N work-stealing scheduler
- Blocking API with hidden concurrency (Go-style)
- Handles panics gracefully with proper error propagation
- Supports cancellation and timeout operations
- NO async/await - pure blocking semantics

```rust
// Example usage - Note: NO async/await
let handle = go!(|| {
    // Routine work here - blocking API
    let result = compute_result();
    result
});

let result = handle.join()?;  // Blocking join
```

### 2. Channel Module (`channel.rs`)

**Purpose**: Type-safe message passing between routines.

**Key Components**:
- `Sender<T>`: Channel sender with blocking send operations
- `Receiver<T>`: Channel receiver with blocking receive operations  
- `channel()`: Creates unbuffered channels
- `bounded_channel()`: Creates channels with specified capacity
- `unbounded_channel()`: Creates unlimited capacity channels

**Channel Types**:
1. **Unbuffered Channels**: Synchronous communication (0 capacity)
2. **Buffered Channels**: Asynchronous with bounded capacity
3. **Unbounded Channels**: Asynchronous with unlimited capacity

**Implementation Details**:
- Built on crossbeam channels for lock-free performance
- Clone-able senders for fan-out patterns
- Graceful close handling with proper error propagation
- Support for try_send/try_recv for non-blocking operations
- Blocking operations that cooperate with M:N scheduler

```rust
// Example usage - Note: NO async/await
let (tx, rx) = channel::<Message>();

go!(move || {
    tx.send(message)?;  // Blocking send
    Ok(())
});

let received = rx.recv()?;  // Blocking receive
```

### 3. Select Module (`select.rs`)

**Purpose**: Multiplexing channel operations with Go-like select semantics.

**Key Components**:
- `Select<T>`: Builder for select operations
- `select!` macro: Go-like select syntax
- Support for recv, send, default, and timeout cases

**Select Semantics**:
1. **Non-deterministic choice**: Random selection among ready operations
2. **Default case**: Immediate return if no operations are ready
3. **Timeout support**: Time-bounded select operations
4. **Fair scheduling**: Prevents starvation of operations

**Implementation Challenges**:
- Requires careful synchronization to avoid race conditions
- Must handle partial operations correctly
- Efficient polling mechanism for multiple channels
- Fair selection algorithm implementation
- Integration with M:N scheduler for blocking operations

```rust
// Example usage - Note: blocking operations
select! {
    msg = rx1.recv() => process_type_a(msg),
    msg = rx2.recv() => process_type_b(msg),
    default => println!("no message available"),
    timeout(Duration::from_secs(5)) => println!("timed out"),
}
```

### 4. Runtime Module (`runtime.rs`)

**Purpose**: M:N scheduler and runtime management.

**Key Components**:
- `RustRoutineRuntime`: Main runtime struct managing the M:N scheduler
- `RuntimeConfig`: Configuration for runtime behavior
- `RuntimeStats`: Performance and usage statistics
- Global runtime management functions
- CPU worker pool and I/O thread pool separation

**Runtime Features**:
1. **Work-stealing scheduler**: Efficient load balancing across CPU threads
2. **I/O thread pool**: Separate pool for blocking I/O operations
3. **Thread migration**: Automatic migration between CPU and I/O pools
4. **Timer wheel**: Efficient timer management for sleep/timeout operations
5. **Statistics collection**: Monitoring routine lifecycle and performance
6. **NUMA-aware scheduling**: On supported platforms

**Performance Considerations**:
- Minimal overhead for routine spawning (<1μs)
- Efficient memory usage for large numbers of routines (2-4KB per routine)
- Lock-free data structures where possible
- Thread pool auto-scaling based on workload

### 5. Scheduler Module (`scheduler.rs`)

**Purpose**: Core M:N work-stealing scheduler implementation.

**Key Components**:
- `Scheduler`: Global scheduler managing worker threads
- `Worker`: Per-thread worker with local run queue
- `Task`: Unit of work scheduled on workers
- Work-stealing deques using crossbeam

**Scheduler Design**:
1. **Local queues**: Each worker has a local deque for tasks
2. **Work stealing**: Workers steal from others when local queue empty
3. **Fair scheduling**: Randomized stealing prevents starvation
4. **Park/unpark**: Efficient thread sleeping when no work available

### 6. I/O Module (`io.rs`)

**Purpose**: Blocking I/O operations that don't block OS threads.

**Key Components**:
- `block_on_io()`: Execute blocking I/O on I/O thread pool
- File operations: read, write, metadata
- Network operations: TCP, UDP support
- Future: Native async I/O (io_uring/IOCP/kqueue)

**Implementation Strategy**:
- Detect I/O operations and migrate routine to I/O pool
- Use std::fs and std::net for initial implementation
- Platform-specific optimizations in later phases
- Automatic thread pool sizing based on I/O load

### 7. Error Module (`error.rs`)

**Purpose**: Comprehensive error handling for all operations.

**Error Types**:
- `SendError`: Channel send operation failures
- `RecvError`: Channel receive operation failures
- `ChannelClosed`: Channel closure notifications
- `SpawnError`: Routine creation failures
- `RuntimeError`: Runtime-level errors
- `Timeout`: Operation timeout errors

## Concurrency Model

### Routine Lifecycle

```
┌─────────┐    spawn()    ┌─────────┐    execute    ┌─────────┐
│ Created │ ────────────> │ Running │ ────────────> │Complete │
└─────────┘               └─────────┘               └─────────┘
                               │                         │
                            abort()                   join()
                               │                         │
                               v                         v
                          ┌─────────┐               ┌─────────┐
                          │Aborted  │               │ Joined  │
                          └─────────┘               └─────────┘
```

### Channel Communication

**Unbuffered Channels**:
- Direct synchronization between sender and receiver
- Sender blocks until receiver is ready
- Provides strong ordering guarantees

**Buffered Channels**:
- Asynchronous communication with bounded queue
- Senders block when buffer is full
- Receivers block when buffer is empty

**Channel Closure**:
- Graceful shutdown mechanism
- All pending operations complete with appropriate errors
- Resources are cleaned up automatically

### Select Operation Flow

```
1. Register all operations with the select
2. Check for immediately available operations
3. If none available:
   a. Check for default case
   b. Check for timeout
   c. Wait for any operation to become ready
4. Execute the selected operation
5. Cancel remaining operations
6. Return result
```

## Memory Management

### Zero-Copy Operations
- Channel messages use move semantics
- Minimal copying of data between routines
- Efficient handling of large data structures

### Resource Cleanup
- Automatic cleanup of routine resources on completion
- Channel cleanup when all senders/receivers are dropped
- Runtime shutdown with graceful termination

### Memory Safety
- No unsafe code in core library
- All operations are memory-safe by construction
- Panic safety with proper resource cleanup

## Performance Characteristics

### Target Benchmarks

**Routine Spawning**:
- Goal: < 1μs per routine spawn
- Memory: < 2KB per routine overhead
- Scalability: Support for millions of concurrent routines

**Channel Operations**:
- Goal: < 100ns per send/receive operation
- Throughput: > 10M messages/second per channel
- Latency: < 1μs for unbuffered channels

**Select Operations**:
- Goal: < 500ns for simple select with 2-3 channels
- Scalability: Linear performance up to 100+ channels
- Fairness: No starvation under high load

### Optimization Strategies

1. **Lock-free Data Structures**: Use crossbeam for high-performance queues
2. **Work-stealing**: Tokio's efficient task scheduler
3. **Batch Operations**: Group multiple operations for efficiency
4. **Adaptive Algorithms**: Adjust behavior based on runtime conditions

## Integration Points

### Standard Library
- Full compatibility with std::thread and std::sync
- Implements standard traits where applicable
- Follows Rust naming and API conventions
- Uses std::fs and std::net for I/O operations

### Platform APIs
- Linux: io_uring for async I/O (future enhancement)
- Windows: IOCP for completion ports (future enhancement)
- macOS: kqueue for event notification (future enhancement)
- Fallback to blocking I/O with thread migration

### Third-party Crates
- Crossbeam for lock-free data structures
- Parking_lot for efficient synchronization
- No dependency on async runtimes (Tokio, async-std, etc.)
- Optional compatibility layers in separate crates

## Error Handling Strategy

### Error Propagation
- Use Result<T, Error> for all fallible operations
- Custom error types with detailed context
- Integration with thiserror for ergonomic error handling

### Panic Safety
- All operations are panic-safe
- Resources are cleaned up on panic
- Panic boundaries at routine boundaries

### Graceful Degradation
- Fallback mechanisms for resource exhaustion
- Configurable timeouts for all operations
- Clear error messages with actionable guidance

## Testing Strategy

### Unit Tests
- Comprehensive test coverage for all modules
- Property-based testing for complex algorithms
- Stress testing for concurrent operations

### Integration Tests
- End-to-end scenarios with multiple routines
- Channel communication patterns
- Select operation combinations

### Performance Tests
- Benchmarks for all critical operations
- Memory usage profiling
- Scaling tests with large numbers of routines

### Platform Tests
- Windows, Linux, macOS compatibility
- Different Rust versions (MSRV: 1.70+)
- Various hardware configurations

## Security Considerations

### Memory Safety
- No unsafe code in public API
- All operations are bounds-checked
- Prevent buffer overflows and use-after-free

### Resource Limits
- Configurable limits on routine count
- Channel buffer size limits
- Timeout enforcement to prevent DoS

### Information Disclosure
- No sensitive information in error messages
- Proper cleanup of potentially sensitive data
- No timing-based information leaks

## Future Enhancements

### Advanced Features
1. **Structured Concurrency**: Scoped routine management
2. **Channel Patterns**: Pub/sub, worker pools, pipelines
3. **Distributed Routines**: Network-transparent communication
4. **Adaptive Runtime**: Self-tuning performance parameters

### Performance Improvements
1. **Custom Allocator**: Routine-specific memory management
2. **SIMD Optimizations**: Vectorized operations where applicable
3. **Hardware Acceleration**: GPU integration for compute-heavy routines
4. **Profiling Integration**: Built-in performance monitoring

### Ecosystem Integration
1. **Tracing Support**: Integration with tracing crate
2. **Metrics Collection**: Prometheus/OpenTelemetry integration
3. **IDE Support**: Better debugging and profiling tools
4. **Documentation**: Interactive examples and tutorials

## Implementation Phases

### Phase 1: Core Foundation (Current - COMPLETE)
- [x] Basic routine spawning
- [x] Simple channels (unbuffered)
- [x] Basic select functionality
- [x] Error handling framework
- [x] Initial runtime implementation
- [x] M:N work-stealing scheduler

### Phase 2: Scheduler Integration (IN PROGRESS)
- [ ] Connect M:N scheduler to public API
- [ ] Remove Tokio dependency completely
- [ ] Implement blocking API for all operations
- [ ] Add I/O thread pool for blocking operations
- [ ] Timer wheel for sleep/delay operations

### Phase 3: Enhanced Channels
- [ ] Buffered channels with proper capacity management
- [ ] Channel closing semantics
- [ ] Broadcast channels
- [ ] Channel iteration patterns

### Phase 4: Advanced Select
- [ ] Fair selection algorithm
- [ ] Efficient multi-channel polling
- [ ] Priority-based selection
- [ ] Complex timeout handling

### Phase 5: Performance Optimization
- [ ] Lock-free channel implementations
- [ ] Memory pool allocation
- [ ] NUMA-aware scheduling
- [ ] Adaptive runtime tuning

### Phase 6: Platform Integration
- [ ] Linux io_uring support
- [ ] Windows IOCP support
- [ ] macOS kqueue support
- [ ] Embedded system support

### Phase 7: Ecosystem Integration
- [ ] Optional Tokio compatibility layer (separate crate)
- [ ] Optional async-std compatibility (separate crate)
- [ ] Standard library extensions (io, net, fs modules)
- [ ] Framework adapters

## Conclusion

RustRoutines provides a comprehensive concurrency library that brings Go's elegant concurrency model to Rust while maintaining safety and performance. The modular design allows for incremental adoption and future enhancements while providing a solid foundation for concurrent Rust applications.

The library balances ease of use with performance, making it suitable for both learning concurrent programming concepts and building production systems that require high-performance concurrency.

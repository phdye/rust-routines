# Implementation Plan - Rust Routines

**Goroutines in Rust**

## Project Overview

### Vision
Rust Routines brings Go's elegant concurrency model to Rust, providing lightweight, stackful coroutines with CSP-style channels while maintaining Rust's safety guarantees and zero-cost abstractions.

### Core Goals
- **Lightweight Execution**: M:N threading model with minimal overhead
- **Familiar API**: Go-inspired syntax adapted to Rust idioms
- **Safety First**: Leverage Rust's type system for compile-time concurrency safety
- **Performance**: Match or exceed native async/await performance
- **Compatibility**: Seamless integration with existing Rust ecosystem

### Key Features
- Stackful coroutines with automatic scheduling
- CSP channels for communication
- Select statements for non-blocking operations
- Work-stealing scheduler with NUMA awareness
- Structured concurrency patterns
- Zero-cost abstractions where possible

## Technical Architecture

### Core Components

```
rust-routines/
├── routine/          # Core routine implementation
│   ├── handle.rs    # RoutineHandle for join/abort
│   ├── spawn.rs     # Spawn implementation using M:N scheduler
│   └── routine.rs   # Routine lifecycle and state
├── scheduler/        # M:N scheduler implementation (COMPLETE)
│   ├── worker.rs    # Worker thread management
│   ├── queue.rs     # Work-stealing queues
│   ├── task.rs      # Task abstraction
│   └── scheduler.rs # Global scheduler coordination
├── channel/          # CSP channel implementation
│   ├── bounded.rs   # Bounded channels
│   ├── unbounded.rs # Unbounded channels
│   └── select.rs    # Select statement support
├── sync/            # Synchronization primitives
│   ├── mutex.rs     # Routine-aware mutex
│   ├── waitgroup.rs # WaitGroup implementation
│   └── once.rs      # Once initialization
├── io/              # I/O operations
│   ├── blocking.rs  # Blocking I/O on separate thread pool
│   ├── file.rs      # File operations
│   └── net.rs       # Network operations
└── runtime/         # Runtime management
    ├── runtime.rs   # Runtime initialization
    ├── executor.rs  # Task execution context
    ├── timer.rs     # Timer wheel for delays
    └── panic.rs     # Panic handling
```

### Design Principles

**1. Pure M:N Threading Model**
- Map M routines to N OS threads
- NO dependency on external async runtimes (Tokio, async-std)
- Blocking API with hidden concurrency (Go-style)
- Automatic load balancing via work-stealing

**2. Dual Thread Pool Design**
- CPU pool: For compute-bound operations (N threads)
- I/O pool: For blocking I/O operations (M threads, dynamic)
- Automatic migration between pools based on operation type
- Thread pool sizing based on workload characteristics

**3. Stack Management**
- Lightweight routine state (2-4KB per routine)
- No stack copying (routines are closures/tasks)
- Efficient memory pooling for routine allocations
- Automatic cleanup on routine termination

**4. Channel Design**
- Lock-free channels using crossbeam
- Type-safe channels with compile-time guarantees
- Both bounded and unbounded variants
- Select statement for multiplexing
- Zero-allocation fast path for common cases

**5. Safety Guarantees**
- No data races (enforced by Rust)
- Panic isolation per routine
- Graceful shutdown semantics
- Resource cleanup on routine termination

## Implementation Phases

### Phase 1: Foundation (v0.1.0) - COMPLETE
**Goal**: Basic routine spawning and execution

#### Section 1-1: Project Setup ✓
- Initialize Cargo project with workspace
- Configure build system and dependencies
- Set up CI/CD pipeline
- Create documentation structure

#### Section 1-2: Basic Routine Implementation ✓
- Routine struct and lifecycle
- Simple spawn API: `routine::spawn(|| { ... })`
- Basic executor with single-threaded runtime
- Thread-local routine storage

#### Section 1-3: M:N Scheduler ✓
- Work-stealing scheduler implementation
- Multiple worker threads
- Lock-free task queues
- Basic load balancing

**Deliverables**: Working M:N scheduler and basic routine spawning

### Phase 2: Scheduler Integration (v0.2.0) - IN PROGRESS
**Goal**: Connect M:N scheduler to public API and remove Tokio

#### Section 2-1: Remove Tokio Dependencies
- Remove tokio from Cargo.toml
- Replace tokio::spawn with our scheduler
- Convert async APIs to blocking APIs
- Update all tests to remove async/await

#### Section 2-2: I/O Thread Pool
- Implement separate thread pool for I/O
- Add routine migration mechanism
- Detect I/O operations automatically
- Dynamic thread pool scaling

#### Section 2-3: Timer Management
- Implement timer wheel for delays
- Park/unpark routine support
- Timeout operations
- Periodic timer support

#### Section 2-4: Integration Testing
- Verify routines use M:N scheduler
- Test I/O operations don't block CPU pool
- Benchmark vs Go implementation
- Stress test with millions of routines

**Deliverables**: Fully integrated M:N runtime without Tokio

### Phase 3: Channels (v0.3.0) - PARTIAL
**Goal**: CSP-style communication primitives

#### Section 3-1: Bounded Channels ✓
- Ring buffer implementation
- Send/receive operations
- Blocking and non-blocking modes
- Channel closing semantics

#### Section 3-2: Unbounded Channels ✓
- Linked list implementation
- Backpressure mechanisms
- Memory management
- Performance optimization

#### Section 3-3: Select Statement ✓
- Select macro implementation
- Non-deterministic choice
- Default case handling
- Timeout support

**Note**: Channels work but use Tokio internals - need migration to crossbeam

**Deliverables**: Complete channel system with Go-like semantics

### Phase 4: Synchronization (v0.4.0)
**Goal**: Routine-aware synchronization primitives

#### Section 4-1: Mutex and RwLock
- Routine-aware locking
- Fair scheduling under contention
- Deadlock detection (debug mode)
- Lock-free fast path

#### Section 4-2: WaitGroup and Barriers
- WaitGroup for routine coordination
- Cyclic barriers
- Countdown latches
- Semaphores

#### Section 4-3: Condition Variables
- Wait/notify mechanisms
- Spurious wakeup handling
- Timeout support
- Integration with mutexes

**Deliverables**: Full synchronization primitive suite

### Phase 5: Advanced Features (v0.5.0)
**Goal**: Production-ready features and optimizations

#### Section 5-1: Structured Concurrency
- Scoped routine spawning
- Automatic cancellation propagation
- Error handling patterns
- Resource cleanup guarantees

#### Section 5-2: Runtime Tunables
- Dynamic worker scaling
- Priority scheduling
- Routine-local storage
- Custom schedulers

#### Section 5-3: Debugging and Profiling
- Routine stack traces
- Deadlock detection
- Performance profiling hooks
- Visualization tools

**Deliverables**: Production-ready runtime with debugging support

### Phase 6: Platform Support (v0.6.0)
**Goal**: Cross-platform compatibility and native I/O

#### Section 6-1: Native Async I/O
- Linux io_uring integration
- Windows IOCP support
- macOS kqueue integration
- FreeBSD support

#### Section 6-2: Architecture Support
- ARM64 support
- RISC-V support
- WebAssembly adaptation
- 32-bit platforms

#### Section 6-3: Embedded Systems
- no_std support
- Static allocation mode
- Minimal runtime option
- Real-time scheduling

**Deliverables**: Comprehensive platform support

### Phase 7: Ecosystem Compatibility (v0.7.0)
**Goal**: Optional compatibility layers for async ecosystem

#### Section 7-1: Tokio Compatibility Layer
- Separate crate: rust-routines-tokio
- Future-to-routine adapter
- Routine-to-future wrapper
- Tokio runtime bridge

#### Section 7-2: Async-std Compatibility
- Separate crate: rust-routines-async-std
- async-std runtime bridge
- Compatibility shims
- Migration helpers

#### Section 7-3: Standard Library Extensions
- routine::io module
- routine::net for networking
- routine::fs for filesystem
- routine::time for timers

**Deliverables**: Optional compatibility with async ecosystem

### Phase 8: Performance & Polish (v1.0.0)
**Goal**: Production-ready 1.0 release

#### Section 8-1: Performance Optimization
- Assembly optimization
- Cache-line alignment
- SIMD where applicable
- Benchmark suite

#### Section 8-2: Documentation & Examples
- Comprehensive API docs
- Migration guide from Go
- Performance tuning guide
- Real-world examples

#### Section 8-3: Stability & Testing
- Extensive test coverage
- Fuzzing harness
- Stress testing
- API stabilization

**Deliverables**: Stable 1.0 release ready for production use

## Performance Targets

### Microbenchmarks
- Routine spawn: < 1μs
- Context switch: < 100ns
- Channel send/recv: < 50ns (uncontended)
- Work stealing: < 200ns per steal

### Comparison Targets
- Match Go's goroutine performance
- Exceed async/await for CPU-bound tasks
- Competitive with Tokio for I/O-bound tasks
- Lower memory overhead than OS threads

### Scalability Goals
- Linear scaling to 64 cores
- Support 1M+ concurrent routines
- Bounded memory growth
- Predictable latencies

## Testing Strategy

### Unit Testing
- Module-level testing with mocks
- Property-based testing with proptest
- Platform-specific test suites
- Benchmarking with criterion

### Integration Testing
- Multi-threaded stress tests
- Deadlock detection tests
- Memory leak detection
- Performance regression tests

### Stress Testing
- High concurrency scenarios
- Long-running stability tests
- Resource exhaustion handling
- Panic propagation verification

### Platform Testing
- CI matrix for all supported platforms
- Architecture-specific assembly validation
- Cross-compilation verification
- Embedded target testing

## API Design Examples

### Basic Usage
```rust
use rust_routines::routine;

fn main() {
    // Simple routine spawn - NO async/await
    routine::spawn(|| {
        println!("Hello from routine!");
    });
    
    // With move semantics
    let data = vec![1, 2, 3];
    routine::spawn(move || {
        println!("Data: {:?}", data);
    });
    
    routine::yield_now(); // Yield to scheduler
    routine::sleep(Duration::from_secs(1)); // Blocking sleep
}
```

### Channels
```rust
use rust_routines::channel;

fn main() {
    let (tx, rx) = channel::bounded(10);
    
    routine::spawn(move || {
        tx.send(42).unwrap(); // Blocking send
    });
    
    routine::spawn(move || {
        let value = rx.recv().unwrap(); // Blocking receive
        println!("Received: {}", value);
    });
}
```

### Select Statement
```rust
use rust_routines::{select, channel};

fn main() {
    let (tx1, rx1) = channel::bounded(1);
    let (tx2, rx2) = channel::bounded(1);
    
    routine::spawn(move || {
        // Macro-based select with Go-like syntax
        select! {
            val = rx1.recv() => println!("From rx1: {:?}", val),
            val = rx2.recv() => println!("From rx2: {:?}", val),
            default => println!("No data available"),
        }
    });
}
```

### Structured Concurrency
```rust
use rust_routines::routine::scope;

fn main() {
    let data = vec![1, 2, 3, 4, 5];
    
    scope(|s| {
        for chunk in data.chunks(2) {
            s.spawn(move || {
                println!("Processing: {:?}", chunk);
            });
        }
    }); // Waits for all routines to complete
}
```

## Risk Mitigation

### Technical Risks
- **Scheduler Integration**: M:N scheduler exists but not connected to API
  - **Mitigation**: Follow integration plan in doc/Scheduler-Integration.md
- **Tokio Removal Complexity**: Channels/select currently use Tokio
  - **Mitigation**: Migrate to crossbeam channels, implement blocking APIs
- **I/O Thread Pool Performance**: Blocking I/O might create bottlenecks
  - **Mitigation**: Dynamic thread pool sizing, eventual native async I/O
- **Platform Incompatibility**: Some platforms may not support our approach
  - **Mitigation**: Graceful fallback to simpler threading model

### Adoption Risks
- **API Changes**: Moving from async/await to blocking API
  - **Mitigation**: Clear migration guide, optional compatibility layer
- **Ecosystem Incompatibility**: Won't work directly with async crates
  - **Mitigation**: Separate compatibility crates for Tokio/async-std
- **Performance Expectations**: Users expecting async performance
  - **Mitigation**: Clear benchmarks showing when our model excels
- **Debugging Complexity**: M:N threading harder to debug than async
  - **Mitigation**: Rich debugging tools, routine-aware profilers

## Success Metrics

### Technical Metrics
- Performance parity with Go routines
- < 1% overhead vs native threads for CPU tasks
- Memory usage < 4KB per routine
- Zero unsafe code in public API

### Adoption Metrics
- 1,000+ GitHub stars within 6 months
- Integration with 5+ major frameworks
- Active community contributions
- Production deployment case studies

## Dependencies

### Core Dependencies
- `crossbeam`: Lock-free data structures and channels
- `parking_lot`: Efficient synchronization primitives  
- `num_cpus`: CPU detection
- `once_cell`: Lazy statics for global scheduler
- `rand`: For fair selection in select statements

### Development Dependencies
- `criterion`: Benchmarking
- `proptest`: Property testing
- `tracing`: Debugging and profiling

### Removed Dependencies
- ~~`tokio`~~: No longer needed - pure M:N implementation
- ~~`futures`~~: No async/await in core library
- ~~`async-trait`~~: Blocking APIs throughout

### Optional Dependencies (separate crates)
- `tokio`: For rust-routines-tokio compatibility crate
- `async-std`: For rust-routines-async-std compatibility crate

## Minimum Supported Rust Version (MSRV)

- Initial: Rust 1.70+
- Target: Rust 1.65+ by v1.0
- Stable channel only
- No nightly features in core

---

**Last Updated**: 2025-01-23
**Version**: 1.0.0
**Status**: Planning Phase
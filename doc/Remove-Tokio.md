# Remove Tokio Strategy Document

**Date**: January 23, 2025  
**Project**: rust-routines - Pure M:N Threading Without Tokio  
**Decision**: Remove Tokio entirely for true Go-like semantics

## Executive Summary

Tokio integration creates unnecessary complexity and prevents true Go-like behavior. This document outlines the strategy to remove Tokio entirely and implement pure M:N threading with blocking I/O that doesn't block OS threads.

## The Problem with Tokio

### Competing Runtimes
- Two thread pools (ours + Tokio's) competing for resources
- Context switching overhead between runtime models
- Complex debugging with two execution environments
- Forced async/await syntax instead of Go's simple blocking API

### Breaking Go Semantics
```rust
// Current (wrong) - Forces async complexity on users
let data = tokio::fs::read_to_string("file.txt").await?;

// Go way - Simple blocking API with hidden async
data, err := ioutil.ReadFile("file.txt")
```

## Proposed Architecture: Pure M:N

```
┌─────────────────────────────────────────────┐
│           User Code (Go-like API)           │
├─────────────────────────────────────────────┤
│              Routine Layer                  │
│  • spawn() / go!() - blocking API          │
│  • channels / select - CSP primitives      │
├─────────────────────────────────────────────┤
│            M:N Scheduler                    │
│  • Work-stealing queues                    │
│  • CPU worker pool (N threads)             │
│  • I/O worker pool (M threads, dynamic)    │
├─────────────────────────────────────────────┤
│          I/O Abstraction Layer             │
│  • Blocking I/O with thread migration      │
│  • Future: Native async (io_uring/IOCP)    │
├─────────────────────────────────────────────┤
│            OS / Kernel                      │
└─────────────────────────────────────────────┘
```

## Implementation Strategy

### Phase 1: Remove Tokio Dependencies
- Remove tokio from Cargo.toml
- Replace tokio::spawn with our scheduler
- Convert async functions to blocking APIs
- Use std::thread for I/O operations initially

### Phase 2: I/O Thread Pool
- Add separate thread pool for blocking I/O
- Implement routine migration between pools
- Smart detection of I/O operations
- Automatic thread scaling based on I/O load

### Phase 3: Native Async I/O (Future)
- Linux: io_uring integration
- Windows: IOCP integration  
- macOS: kqueue integration
- Fallback to blocking I/O on other platforms

## API Changes

### Before (with Tokio)
```rust
pub async fn read_file(path: &str) -> Result<Vec<u8>> {
    tokio::fs::read(path).await
}

pub async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await
}
```

### After (Pure M:N)
```rust
pub fn read_file(path: &str) -> Result<Vec<u8>> {
    // Blocking API, but scheduler handles thread migration
    scheduler::block_on_io(|| {
        std::fs::read(path)
    })
}

pub fn sleep(duration: Duration) {
    // Park routine, let scheduler handle wake-up
    scheduler::park_current(duration);
}
```

## Benefits

1. **Simpler Mental Model**: One runtime to understand
2. **True Go Semantics**: Blocking API with hidden concurrency
3. **Better Performance**: No runtime context switching
4. **Smaller Binary**: Tokio is ~500KB of dependencies
5. **Full Control**: Can optimize for specific use cases
6. **Easier Debugging**: Single execution model to trace

## Migration Plan

### Step 1: Core Runtime (Week 1)
- [ ] Remove tokio dependency
- [ ] Implement blocking spawn() using our scheduler
- [ ] Convert routine.rs to use pure M:N model
- [ ] Update channel implementation to remove tokio::sync

### Step 2: I/O Operations (Week 2)
- [ ] Implement I/O thread pool
- [ ] Add routine migration mechanism
- [ ] Create blocking I/O wrappers
- [ ] Test file and network operations

### Step 3: Timers & Delays (Week 3)
- [ ] Implement timer wheel for sleep operations
- [ ] Add deadline/timeout support
- [ ] Remove tokio::time completely
- [ ] Test timer precision

### Step 4: Testing & Benchmarking (Week 4)
- [ ] Update all tests to remove async/await
- [ ] Benchmark vs Go implementation
- [ ] Stress test I/O thread pool
- [ ] Validate thread migration

## Risk Mitigation

### Risk: Blocking I/O Performance
**Mitigation**: Use thread pool sizing heuristics from Go's runtime. Start with GOMAXPROCS equivalent for CPU threads and 128 for I/O threads.

### Risk: Ecosystem Compatibility
**Mitigation**: Provide compatibility layer for async ecosystem through separate crate (rust-routines-tokio-compat).

### Risk: Platform Differences
**Mitigation**: Abstract platform-specific code behind traits. Use std::thread as fallback.

## Success Metrics

- Zero tokio dependencies in core
- Blocking API for all operations
- Performance within 10% of Go for equivalent operations
- Memory usage comparable to Go routines (2-4KB per routine)
- Thread count matches configured limits

## Conclusion

Removing Tokio enables true Go-like concurrency in Rust. The blocking API with M:N scheduling hidden underneath provides the simplicity Go developers expect while maintaining Rust's safety guarantees.

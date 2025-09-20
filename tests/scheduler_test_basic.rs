//! Minimal scheduler integration test
//!
//! This is a simplified version that should compile without linker issues.

mod common;

use rust_routines::prelude::*;

/// Simple test that should always pass
#[test]
fn test_basic_scheduler() {
    common::run_test(|| async {
        // Just test that we can spawn a simple routine
        let handle = go!(async {
            42
        });
        
        let result = handle.join().await.unwrap();
        assert_eq!(result, 42);
    });
}

/// Common test harness for rust-routines tests
/// 
/// This module provides a proper test harness that runs tests within
/// the rust-routines runtime context, avoiding conflicts with Tokio.

use std::future::Future;
use std::sync::Once;
use futures::executor::block_on;

/// Initialize the test environment once
static INIT: Once = Once::new();

/// Initialize test environment (called automatically by test macros)
pub fn init_test_env() {
    INIT.call_once(|| {
        // Any one-time initialization for the test suite
        // The GLOBAL_SCHEDULER is already initialized lazily
    });
}

/// Run an async test in the rust-routines context
/// 
/// This function executes async tests using futures::executor::block_on
/// which provides a minimal async runtime that doesn't conflict with
/// the rust-routines scheduler.
pub fn run_test<F, Fut>(test_fn: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    init_test_env();
    
    // Run the test using futures executor
    // This provides async/await support without Tokio
    block_on(test_fn());
}

/// Macro to define a rust-routines test
/// 
/// Usage:
/// ```
/// rr_test!(async fn my_test() {
///     // test code here
/// });
/// ```
#[macro_export]
macro_rules! rr_test {
    (async fn $name:ident() $body:block) => {
        #[test]
        fn $name() {
            $crate::common::run_test(|| async move $body);
        }
    };
}

// Testing infrastructure module for nh
//
// This module provides a specialized, isolated testing interface with
// proptest-based property testing and a uniform interface for testing the
// codebase.

pub mod fixtures;
pub mod harness;
pub mod mock;
pub mod prop;
pub mod utils;

// Re-export key components for easier imports in tests
pub use fixtures::*;
pub use harness::*;
pub use prop::*;
pub use utils::*;

/// Test initialization macro that sets up the test environment
/// with proper tracing and error handling.
#[macro_export]
macro_rules! init_test {
    () => {
        let _guard = $crate::testing::harness::test_init();
    };
}

/// Property test initialization macro
#[macro_export]
macro_rules! init_prop_test {
    ($config:expr) => {
        $crate::testing::prop::with_proptest_config($config, || {
            let _guard = $crate::testing::harness::test_init();
        })
    };
    () => {
        $crate::init_prop_test!($crate::testing::prop::default_config())
    };
}

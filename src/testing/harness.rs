use std::sync::{
    LazyLock,
    Mutex,
    Once,
};

use tracing_subscriber::{
    fmt,
    prelude::*,
    registry,
    EnvFilter,
};

// We gotta make sure test initialization happens *only* once
static INIT: Once = Once::new();

// Global mutex to prevent parallel environment manipulation
static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

// Set the log level during tests
const TEST_LOG_ENV: &str = "NH_TEST_LOG";

/// Make sure proper cleanup occurs when test exits
pub struct TestGuard;

impl Drop for TestGuard {
    fn drop(&mut self) {
        // Cleanup resources when tests complete
        std::env::remove_var("NH_NO_CHECKS");
        std::env::remove_var("NH_FLAKE");
        std::env::remove_var("NH_OS_FLAKE");
        std::env::remove_var("NH_HOME_FLAKE");
        std::env::remove_var("NH_DARWIN_FLAKE");
        std::env::remove_var("FLAKE");
    }
}

/// Initialize the test environment once
///
/// Sets up:
/// - Color-eyre for error reporting
/// - Tracing subscribers for test logging
/// - Default environment configuration
///
/// Returns a guard that cleans up when the test completes
pub fn test_init() -> TestGuard {
    // Only run initialization once
    INIT.call_once(|| {
        color_eyre::config::HookBuilder::default()
            .display_location_section(true)
            .display_env_section(false)
            .panic_section("Test panic - please revise your changes")
            .install()
            .expect("Failed to install color-eyre hook");

        // Specialized tracing setup for tests. Since this is not a user-facing
        // component we can be a bit more verbose here if we want to.
        let filter = EnvFilter::try_from_env(TEST_LOG_ENV).unwrap_or_else(|_| EnvFilter::new("off"));

        let fmt_layer = fmt::layer().with_target(true).with_writer(std::io::stderr);

        registry().with(filter).with(fmt_layer).init();

        // Set default test environment
        // This helps with isolation, and speeds up tests. We can afford to avoid
        // testing the environment inside tests since the checks mechanism is tested
        // by its own.
        std::env::set_var("NH_NO_CHECKS", "1");
    });

    TestGuard
}

/// Provides a temporary environment context for tests
pub struct EnvContext {
    original:    std::collections::HashMap<String, Option<String>>,
    _lock_guard: Option<std::sync::MutexGuard<'static, ()>>,
}

impl EnvContext {
    /// Create a new environment context
    pub fn new() -> Self {
        Self {
            original:    std::collections::HashMap::new(),
            _lock_guard: None,
        }
    }

    /// Create a new environment context with locking to prevent parallel tests
    /// from interfering
    pub fn with_lock() -> Self {
        Self {
            original:    std::collections::HashMap::new(),
            _lock_guard: Some(ENV_LOCK.lock().unwrap()),
        }
    }

    /// Set an environment variable for the duration of this context
    pub fn set_var(&mut self, key: &str, value: &str) -> &mut Self {
        let key = key.to_string();
        if !self.original.contains_key(&key) {
            self.original.insert(key.clone(), std::env::var(&key).ok());
        }
        std::env::set_var(key, value);
        self
    }

    /// Remove an environment variable for the duration of this context
    pub fn remove_var(&mut self, key: &str) -> &mut Self {
        let key = key.to_string();
        if !self.original.contains_key(&key) {
            self.original.insert(key.clone(), std::env::var(&key).ok());
        }
        std::env::remove_var(key);
        self
    }

    /// Run the provided function in this environment context
    pub fn run<F, T>(&mut self, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let result = f();
        self.restore();
        result
    }

    /// Restore the original environment
    fn restore(&self) {
        for (key, value) in &self.original {
            match value {
                Some(val) => std::env::set_var(key, val),
                None => std::env::remove_var(key),
            }
        }
    }
}

impl Drop for EnvContext {
    fn drop(&mut self) {
        self.restore();
    }
}

impl Default for EnvContext {
    fn default() -> Self {
        Self::new()
    }
}

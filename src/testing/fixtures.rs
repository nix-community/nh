use std::{
    path::PathBuf,
    sync::LazyLock,
};

use crate::installable::Installable;

// Provide common test data for unit tests and property tests. This is better
// for consistency across tests and hopefully reduces duplication of test setup
// code.

/// Common test flake references
pub static TEST_FLAKE_REFERENCES: LazyLock<Vec<String>> = LazyLock::new(|| {
    vec![
        "github:viperML/nh".to_string(),
        "github:nix-community/home-manager".to_string(),
        "nixpkgs".to_string(),
        ".".to_string(),
        "/home/user/nixos-config".to_string(),
    ]
});

/// Common test attribute paths
pub static TEST_ATTRIBUTE_PATHS: LazyLock<Vec<Vec<String>>> = LazyLock::new(|| {
    vec![
        vec!["nixosConfigurations".to_string(), "myhost".to_string()],
        vec!["homeConfigurations".to_string(), "myuser".to_string()],
        vec!["darwinConfigurations".to_string(), "myhost".to_string()],
        vec![
            "legacyPackages".to_string(),
            "x86_64-linux".to_string(),
            "hello".to_string(),
        ],
        vec![],
    ]
});

/// Common test hostnames
pub static TEST_HOSTNAMES: LazyLock<Vec<String>> = LazyLock::new(|| {
    vec![
        "myhost".to_string(),
        "nixos".to_string(),
        "macbook".to_string(),
        "desktop".to_string(),
    ]
});

/// Common test profile paths
pub static TEST_PROFILE_PATHS: LazyLock<Vec<PathBuf>> = LazyLock::new(|| {
    vec![
        PathBuf::from("/nix/var/nix/profiles/system"),
        PathBuf::from("/nix/var/nix/profiles/per-user/myuser/home-manager"),
        PathBuf::from("/home/myuser/.local/state/nix/profiles/home-manager"),
        PathBuf::from("/Users/myuser/.local/state/nix/profiles/home-manager"),
    ]
});

/// Create a test Installable::Flake with specified reference and attribute
pub fn create_test_flake(reference: &str, attribute: Vec<String>) -> Installable {
    Installable::Flake {
        reference: reference.to_string(),
        attribute,
    }
}

/// Create a test Installable::File with specified path and attribute
pub fn create_test_file(path: &str, attribute: Vec<String>) -> Installable {
    Installable::File {
        path: PathBuf::from(path),
        attribute,
    }
}

/// Create a test Installable::Expression with specified expression and
/// attribute
pub fn create_test_expression(expression: &str, attribute: Vec<String>) -> Installable {
    Installable::Expression {
        expression: expression.to_string(),
        attribute,
    }
}

/// Create a test Installable::Store with specified path
pub fn create_test_store(path: &str) -> Installable {
    Installable::Store {
        path: PathBuf::from(path),
    }
}

/// Create a temporary directory structure for tests
pub fn create_temp_test_directory() -> tempfile::TempDir {
    tempfile::tempdir().expect("Failed to create temporary directory")
}

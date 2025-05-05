use std::{
    path::{
        Path,
        PathBuf,
    },
    time::{
        Duration,
        SystemTime,
    },
};

use color_eyre::Result;
use humantime::DurationError;

/// Testing utilities module that provides common helper functions
/// for testing nh functionality.

/// Parse a duration string using the humantime crate
pub fn parse_test_duration(duration_str: &str) -> Result<Duration, DurationError> {
    humantime::parse_duration(duration_str)
}

/// Create a SystemTime at a specific offset from now
pub fn system_time_at_offset(offset_secs: i64) -> SystemTime {
    if offset_secs >= 0 {
        SystemTime::now() + Duration::from_secs(offset_secs as u64)
    } else {
        SystemTime::now() - Duration::from_secs((-offset_secs) as u64)
    }
}

/// Get all test files in a directory with the specified extension
pub fn find_test_files(directory: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    if directory.is_dir() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some(extension) {
                result.push(path);
            } else if path.is_dir() {
                result.extend(find_test_files(&path, extension)?);
            }
        }
    }
    Ok(result)
}

/// Create a temporary file with the given content
pub fn create_temp_file(content: &str) -> Result<(tempfile::NamedTempFile, PathBuf)> {
    let file = tempfile::NamedTempFile::new()?;
    std::fs::write(&file, content)?;
    let path = file.path().to_path_buf();
    Ok((file, path))
}

/// Create a temporary directory with a specific structure
pub fn create_test_directory_structure(
    base_dir: &Path,
    structure: &[(&str, Option<&str>)],
) -> Result<Vec<PathBuf>> {
    let mut created_paths = Vec::new();

    for (path, content) in structure {
        let full_path = base_dir.join(path);

        // Create parent directories if they don't exist
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Create file or directory
        if let Some(content) = content {
            // It's a file
            std::fs::write(&full_path, content)?;
        } else {
            // It's a directory
            std::fs::create_dir_all(&full_path)?;
        }

        created_paths.push(full_path);
    }

    Ok(created_paths)
}

/// Asserts two vectors contain the same elements, regardless of order
pub fn assert_same_elements<T: PartialEq + std::fmt::Debug>(a: &[T], b: &[T]) {
    assert_eq!(a.len(), b.len(), "Vectors have different lengths");

    for item in a {
        assert!(
            b.iter().any(|x| x == item),
            "Item {:?} in first vector not found in second vector",
            item
        );
    }
}

/// Helper function to run a command and capture its output for testing
pub fn run_test_command(command: &str, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new(command).args(args).output()?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Create a test environment with specific variables set
pub fn with_test_env<F, T>(env_vars: &[(&str, &str)], test_fn: F) -> T
where
    F: FnOnce() -> T,
{
    // Save the current environment
    let mut old_values = Vec::new();

    // Set test environment
    for (key, value) in env_vars {
        let old_value = std::env::var(key).ok();
        old_values.push((key, old_value));
        std::env::set_var(key, value);
    }

    // Run the test
    let result = test_fn();

    // Restore the environment
    for (key, old_value) in old_values {
        match old_value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    result
}

use std::path::PathBuf;

use color_eyre::Result;
use mockall::{
    predicate::*,
    *,
};

/// Mock for command execution
///
/// This allows testing code that shells out without actually
/// running commands on the system.
#[automock]
pub trait CommandExecutor {
    fn run_command<'a>(&self, cmd: &str, args: &[&'a str]) -> Result<String>;
    fn run_command_with_input<'a>(&self, cmd: &str, args: &[&'a str], input: &str) -> Result<String>;
}

/// Default implementation that actually runs commands
pub struct RealCommandExecutor;

impl CommandExecutor for RealCommandExecutor {
    fn run_command<'a>(&self, cmd: &str, args: &[&'a str]) -> Result<String> {
        let output = std::process::Command::new(cmd).args(args).output()?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(color_eyre::eyre::eyre!(
                "Command failed: {} {}\nStderr: {}",
                cmd,
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }

    fn run_command_with_input<'a>(&self, cmd: &str, args: &[&'a str], input: &str) -> Result<String> {
        let mut child = std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin.write_all(input.as_bytes())?;
        }

        let output = child.wait_with_output()?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(color_eyre::eyre::eyre!(
                "Command failed: {} {}\nStderr: {}",
                cmd,
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    }
}

/// Mock for file system operations
///
/// This allows testing code that interacts with the file system
/// without actually modifying files on disk.
#[automock]
pub trait FileSystem {
    fn read_file(&self, path: &PathBuf) -> Result<String>;
    fn write_file(&self, path: &PathBuf, contents: &str) -> Result<()>;
    fn file_exists(&self, path: &PathBuf) -> bool;
    fn create_dir_all(&self, path: &PathBuf) -> Result<()>;
}

/// Default implementation that actually interacts with the file system
pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn read_file(&self, path: &PathBuf) -> Result<String> {
        Ok(std::fs::read_to_string(path)?)
    }

    fn write_file(&self, path: &PathBuf, contents: &str) -> Result<()> {
        Ok(std::fs::write(path, contents)?)
    }

    fn file_exists(&self, path: &PathBuf) -> bool {
        path.exists()
    }

    fn create_dir_all(&self, path: &PathBuf) -> Result<()> {
        Ok(std::fs::create_dir_all(path)?)
    }
}

use std::{
  env,
  ffi::OsString,
  io::Read,
  path::{Path, PathBuf},
  sync::{
    Arc,
    OnceLock,
    atomic::{AtomicBool, Ordering},
  },
  time::Duration,
};

use color_eyre::{
  Result,
  eyre::{Context, bail, eyre},
};
use secrecy::{ExposeSecret, SecretString};
use subprocess::{Exec, ExitStatus, Redirection};
use tracing::{debug, error, info, warn};

use crate::{
  commands::{ElevationStrategy, cache_password, get_cached_password},
  installable::Installable,
  util::NixVariant,
};

/// Global flag indicating whether a SIGINT (Ctrl+C) was received.
static INTERRUPTED: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// Get or initialize the interrupt flag.
///
/// Returns a reference to the shared interrupt flag, initializing it on first
/// call.
fn get_interrupt_flag() -> &'static Arc<AtomicBool> {
  INTERRUPTED.get_or_init(|| Arc::new(AtomicBool::new(false)))
}

/// Cache for signal handler registration status.
static HANDLER_REGISTERED: OnceLock<()> = OnceLock::new();

/// Builds a remote command string with proper elevation handling.
///
/// Constructs the command to execute on the remote host, wrapping it with
/// the appropriate elevation program (sudo/doas/etc) based on the strategy.
///
/// # Arguments
/// * `strategy` - Optional elevation strategy to use
/// * `base_cmd` - The base command to execute
///
/// # Returns
/// The complete command string to execute on the remote
///
/// # Errors
/// Returns error if:
/// - Elevation program cannot be resolved
/// - Elevation program name cannot be determined
fn build_remote_command(
  strategy: Option<&ElevationStrategy>,
  base_cmd: &str,
) -> Result<String> {
  if let Some(strategy) = strategy {
    if matches!(strategy, ElevationStrategy::None) {
      return Ok(base_cmd.to_string());
    }

    let program = strategy.resolve()?;
    let program_name = program
      .file_name()
      .and_then(|name| name.to_str())
      .ok_or_else(|| eyre!("Failed to determine elevation program name"))?;

    match (program_name, strategy) {
      // sudo passwordless: use --non-interactive to fail if password required
      ("sudo", ElevationStrategy::Passwordless) => {
        Ok(format!(
          "{} --non-interactive {}",
          program.display(),
          base_cmd
        ))
      },
      ("sudo", _) => {
        Ok(format!(
          "{} --prompt= --stdin {}",
          program.display(),
          base_cmd
        ))
      },
      // doas passwordless: use -n flag (non-interactive)
      ("doas", ElevationStrategy::Passwordless) => {
        Ok(format!("{} -n {}", program.display(), base_cmd))
      },
      ("doas", _) => {
        bail!(
          "doas does not support stdin password input for remote deployment. \
           Use --elevation-strategy=passwordless if remote has NOPASSWD \
           configured."
        )
      },
      // run0 passwordless: use --no-ask-password flag
      ("run0", ElevationStrategy::Passwordless) => {
        Ok(format!(
          "{} --no-ask-password {}",
          program.display(),
          base_cmd
        ))
      },
      ("run0", _) => {
        bail!(
          "run0 does not support stdin password input for remote deployment. \
           Use --elevation-strategy=passwordless if authentication is not \
           required."
        )
      },
      // pkexec: no passwordless support
      ("pkexec", _) => {
        bail!(
          "pkexec does not support non-interactive password input for remote \
           deployment. pkexec requires a polkit agent which is not available \
           over SSH."
        )
      },
      // Unknown program: bail instead of guessing
      (_, ElevationStrategy::Passwordless) => {
        bail!(
          "Unknown elevation program '{}' does not have known passwordless \
           support. Only sudo, doas, and run0 are supported with \
           --elevation-strategy=passwordless",
          program_name
        )
      },
      (..) => {
        bail!(
          "Unknown elevation program '{}' does not support stdin password \
           input for remote deployment. Only sudo supports password input \
           over SSH. Use --elevation-strategy=passwordless if remote has \
           passwordless elevation configured, or use a known elevation \
           program (sudo/doas/run0).",
          program_name
        )
      },
    }
  } else {
    Ok(base_cmd.to_string())
  }
}

/// Register a SIGINT handler that sets the global interrupt flag.
///
/// This function is idempotent - multiple calls are safe and will not
/// create multiple handlers. Uses `signal_hook::flag::register` which
/// is async-signal-safe.
///
/// # Errors
///
/// Returns an error if the signal handler cannot be registered.
fn register_interrupt_handler() -> Result<()> {
  use signal_hook::{consts::SIGINT, flag};

  if HANDLER_REGISTERED.get().is_some() {
    return Ok(());
  }

  // Not registered yet, register it
  flag::register(SIGINT, Arc::clone(get_interrupt_flag()))
    .context("Failed to register SIGINT handler")?;

  // Mark as registered
  // The race condition here is benign. Worst case, we register twice, but both
  // handlers will set the same flag which is fine
  let _ = HANDLER_REGISTERED.set(());

  Ok(())
}

/// Guard that cleans up SSH `ControlMaster` sockets on drop.
///
/// This ensures SSH control connections are properly closed when remote
/// operations complete, preventing lingering SSH processes.
#[must_use]
pub struct SshControlGuard {
  control_dir: PathBuf,
}

impl Drop for SshControlGuard {
  fn drop(&mut self) {
    cleanup_ssh_control_sockets(&self.control_dir);
  }
}

/// Clean up SSH `ControlMaster` sockets in the control directory.
///
/// Iterates through all ssh-* control sockets and sends the "exit" command
/// to close the master connection. Errors are logged but not propagated.
fn cleanup_ssh_control_sockets(control_dir: &std::path::Path) {
  debug!(
    "Cleaning up SSH control sockets in {}",
    control_dir.display()
  );

  // Read directory entries
  let entries = match std::fs::read_dir(control_dir) {
    Ok(entries) => entries,
    Err(e) => {
      // Directory might not exist if no SSH connections were made
      debug!(
        "Could not read SSH control directory {}: {}",
        control_dir.display(),
        e
      );
      return;
    },
  };

  for entry in entries.flatten() {
    let path = entry.path();

    // Only process files starting with "ssh-"
    if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
      if filename.starts_with("ssh-") {
        debug!("Closing SSH control socket: {}", path.display());

        // Run: ssh -o ControlPath=<socket> -O exit dummyhost
        let result = Exec::cmd("ssh")
          .args(&["-o", &format!("ControlPath={}", path.display())])
          .args(&["-O", "exit", "dummyhost"])
          .stdout(Redirection::Pipe)
          .stderr(Redirection::Pipe)
          .capture();

        match result {
          Ok(capture) => {
            if !capture.exit_status.success() {
              // This is normal if the connection was already closed
              debug!(
                "SSH control socket cleanup exited with status {:?} for {}",
                capture.exit_status,
                path.display()
              );
            }
          },
          Err(e) => {
            tracing::warn!(
              "Failed to close SSH control socket at {}: {}",
              path.display(),
              e
            );
          },
        }
      }
    }
  }
}

/// Initialize SSH control socket management.
///
/// Returns a guard that will clean up SSH `ControlMaster` connections when
/// dropped. The guard should be held for the duration of remote operations.
pub fn init_ssh_control() -> SshControlGuard {
  let control_dir = get_ssh_control_dir().clone();
  SshControlGuard { control_dir }
}

/// Cache for the SSH control socket directory.
static SSH_CONTROL_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Get or create the SSH control socket directory.
///
/// This creates a temporary directory that persists for the lifetime of the
/// program, similar to nixos-rebuild-ng's tmpdir module.
fn get_ssh_control_dir() -> &'static PathBuf {
  SSH_CONTROL_DIR.get_or_init(|| {
    // Try to use XDG_RUNTIME_DIR first (usually /run/user/<uid>), fall back to
    // /tmp
    // XXX: I do not want to use the dirs crate just for this.
    let base = env::var("XDG_RUNTIME_DIR")
      .map_or_else(|_| PathBuf::from("/tmp"), PathBuf::from);

    let control_dir = base.join(format!("nh-ssh-{}", std::process::id()));

    // Create the directory if it doesn't exist
    if let Err(e1) = std::fs::create_dir_all(&control_dir) {
      debug!(
        "Failed to create SSH control directory at {}: {e1}",
        control_dir.display()
      );

      // Fall back to /tmp/nh-ssh-<pid> - try creating there instead
      let fallback_dir =
        PathBuf::from("/tmp").join(format!("nh-ssh-{}", std::process::id()));

      // As a last resort, if *all else* fails, we construct a unique
      // subdirectory under /tmp with PID and full timestamp to preserve
      // process isolation and avoid collisions between concurrent invocations
      if let Err(e2) = std::fs::create_dir_all(&fallback_dir) {
        let timestamp = std::time::SystemTime::now()
          .duration_since(std::time::UNIX_EPOCH)
          .map_or(0, |d| {
            d.as_secs() * 1_000_000_000 + u64::from(d.subsec_nanos())
          });

        let unique_dir = PathBuf::from("/tmp").join(format!(
          "nh-ssh-{}-{}",
          std::process::id(),
          timestamp
        ));

        if let Err(e3) = std::fs::create_dir_all(&unique_dir) {
          error!(
            "Failed to create SSH control directory after exhausting all \
             fallbacks. Errors: (1) {}: {e1}, (2) {}: {e2}, (3) {}: {e3}. SSH \
             operations will likely fail.",
            control_dir.display(),
            fallback_dir.display(),
            unique_dir.display()
          );

          // Return the path anyway; SSH will fail with a clear error if
          // directory creation is truly impossible, and at this point we
          // are out of options.
          return unique_dir;
        }

        debug!(
          "Created unique SSH control directory: {}",
          unique_dir.display()
        );
        return unique_dir;
      }
      return fallback_dir;
    }

    control_dir
  })
}

/// A parsed remote host specification.
///
/// Handles various formats:
///
/// - `hostname`
/// - `user@hostname`
/// - `ssh://[user@]hostname` (scheme stripped)
/// - `ssh-ng://[user@]hostname` (scheme stripped)
#[derive(Debug, Clone)]
pub struct RemoteHost {
  /// The host string (may include user@)
  host: String,
}

impl RemoteHost {
  /// Get the hostname part without the `user@` prefix.
  ///
  /// Used for hostname comparisons when determining if two `RemoteHost`
  /// instances refer to the same physical host (e.g., detecting when
  /// `build_host` == `target_host` regardless of different user credentials).
  ///
  /// Returns the bracketed IPv6 address as-is if present (e.g.,
  /// `[2001:db8::1]`).
  ///
  /// # Panics
  ///
  /// This function will never panic in practice because `rsplit('@').next()`
  /// always returns at least one element (the original string if no '@'
  /// exists).
  #[must_use]
  pub fn hostname(&self) -> &str {
    #[allow(clippy::unwrap_used)]
    self.host.rsplit('@').next().unwrap()
  }

  /// Parse a host specification string.
  ///
  /// Accepts:
  /// - `hostname`
  /// - `user@hostname`
  /// - `ssh://[user@]hostname`
  /// - `ssh-ng://[user@]hostname`
  ///
  /// URI schemes are stripped since `--build-host` uses direct SSH.
  ///
  /// # Errors
  ///
  /// Returns an error if the host specification is invalid (empty hostname,
  /// empty username, contains invalid characters like `:` or `/`).
  pub fn parse(input: &str) -> Result<Self> {
    // Strip URI schemes - we use direct SSH regardless
    let host = input
      .strip_prefix("ssh-ng://")
      .or_else(|| input.strip_prefix("ssh://"))
      .unwrap_or(input);

    if host.is_empty() {
      bail!("Empty hostname in host specification");
    }

    // Validate: check for empty user in user@host format
    if host.starts_with('@') {
      bail!("Empty username in host specification: {input}");
    }
    if host.ends_with('@') {
      bail!("Empty hostname in host specification: {input}");
    }

    // Validate hostname doesn't contain invalid characters
    // (after stripping any user@ prefix for the check)
    let hostname_part = host.rsplit('@').next().unwrap_or(host);
    if hostname_part.contains('/') {
      bail!(
        "Invalid hostname '{hostname_part}': contains '/'. Did you mean to \
         use a bare hostname?"
      );
    }

    // Check for colons, but allow them in bracketed IPv6 addresses
    if hostname_part.contains(':') {
      // Check if this is a bracketed IPv6 address
      let is_bracketed_ipv6 =
        hostname_part.starts_with('[') && hostname_part.contains(']');

      if !is_bracketed_ipv6 {
        bail!(
          "Invalid hostname '{}': contains ':'. Ports should be specified via \
           NIX_SSHOPTS=\"-p 2222\" or ~/.ssh/config",
          hostname_part
        );
      }

      // Validate bracket matching for IPv6
      if !hostname_part.ends_with(']') {
        bail!(
          "Invalid IPv6 address '{}': contains characters after closing \
           bracket",
          hostname_part
        );
      }

      let open_count = hostname_part.matches('[').count();
      let close_count = hostname_part.matches(']').count();
      if open_count != 1 || close_count != 1 {
        bail!(
          "Invalid IPv6 address '{}': mismatched brackets",
          hostname_part
        );
      }
    }

    Ok(Self {
      host: host.to_string(),
    })
  }

  /// Get the SSH-compatible host string.
  ///
  /// Strips brackets from IPv6 addresses since SSH doesn't accept them.
  /// Preserves zone IDs (`%eth0`) and `user@` prefix if present.
  ///
  /// Examples:
  ///
  /// - `[2001:db8::1]` -> `2001:db8::1`
  /// - `user@[2001:db8::1]` -> `user@2001:db8::1`
  /// - `[fe80::1%eth0]` -> `fe80::1%eth0`
  /// - `host.example` -> `host.example`
  #[must_use]
  pub fn ssh_host(&self) -> String {
    let hostname = self.hostname();

    // Check for bracketed IPv6 address
    if hostname.starts_with('[') && hostname.ends_with(']') {
      let inner = &hostname[1..hostname.len() - 1];

      // Validate it's actually a valid IPv6 address
      // Split on '%' to validate only the address part (zone ID is
      // SSH-specific)
      let addr_part = inner.split('%').next().unwrap_or(inner);
      if addr_part.parse::<std::net::Ipv6Addr>().is_ok() {
        // Reconstruct with user@ prefix if present
        if let Some(at_pos) = self.host.find('@') {
          let user = &self.host[..at_pos];
          return format!("{user}@{inner}");
        }
        return inner.to_string();
      }
    }

    // Not IPv6 or not bracketed, return as-is
    self.host.clone()
  }
}

impl std::fmt::Display for RemoteHost {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.host)
  }
}

/// Get the default SSH options for connection multiplexing.
/// Includes a `ControlPath` pointing to our control socket directory.
fn get_default_ssh_opts() -> Vec<String> {
  let control_dir = get_ssh_control_dir();
  let control_path = control_dir.join("ssh-%n");

  vec![
    "-o".to_string(),
    "ControlMaster=auto".to_string(),
    "-o".to_string(),
    format!("ControlPath={}", control_path.display()),
    "-o".to_string(),
    "ControlPersist=60".to_string(),
  ]
}

/// Shell-quote a string for safe passing through SSH to remote shell.
fn shell_quote(s: &str) -> String {
  // Use shlex::try_quote for battle-tested quoting
  // Returns Cow::Borrowed if no quoting needed, Cow::Owned if quoted
  shlex::try_quote(s).map_or_else(
    |_| format!("'{}'", s.replace('\'', r"'\''")),
    std::borrow::Cow::into_owned,
  )
}

/// Get SSH options from `NIX_SSHOPTS` plus our defaults. This includes
/// connection multiplexing options (`ControlMaster`, `ControlPath`,
/// `ControlPersist`) which enable efficient reuse of SSH connections.
pub fn get_ssh_opts() -> Vec<String> {
  let mut opts: Vec<String> = Vec::new();

  // User options first (from NIX_SSHOPTS)
  if let Ok(sshopts) = env::var("NIX_SSHOPTS") {
    if let Some(parsed) = shlex::split(&sshopts) {
      opts.extend(parsed);
    } else {
      let truncated = sshopts.chars().take(60).collect::<String>();
      let sshopts_display = if sshopts.len() > 60 {
        format!("{truncated}...",)
      } else {
        truncated
      };
      warn!(
        "Failed to parse NIX_SSHOPTS, ignoring. Provide valid options or use \
         ~/.ssh/config. Value: {sshopts_display}",
      );
    }
  }

  // Then our defaults (including ControlPath)
  opts.extend(get_default_ssh_opts());

  opts
}

/// Get `NIX_SSHOPTS` environment value with our defaults appended.
/// Used for `nix-copy-closure` which reads `NIX_SSHOPTS`.
///
/// Note: `nix-copy-closure` splits `NIX_SSHOPTS` by whitespace without shell
/// parsing, so values containing spaces cannot be properly passed through
/// this mechanism. Users needing complex SSH options should use
/// `~/.ssh/config` instead.
fn get_nix_sshopts_env() -> String {
  let user_opts = env::var("NIX_SSHOPTS").unwrap_or_default();
  let default_opts = get_default_ssh_opts();

  if user_opts.is_empty() {
    default_opts.join(" ")
  } else {
    // Append our defaults to user options
    // NOTE: We preserve user options as-is since nix-copy-closure
    // does simple whitespace splitting
    format!("{} {}", user_opts, default_opts.join(" "))
  }
}

/// Check if remote cleanup is enabled via environment variable.
///
/// Returns `true` if `NH_REMOTE_CLEANUP` is set to a truthy value:
/// "1", "true", "yes" (case-insensitive).
///
/// Returns `false` if unset, empty, or set to any other value.
fn should_cleanup_remote() -> bool {
  env::var("NH_REMOTE_CLEANUP").is_ok_and(|val| {
    let val = val.trim().to_lowercase();
    val == "1" || val == "true" || val == "yes"
  })
}

/// Attempt to clean up a remote process using pkill.
///
/// This is a best-effort (and opt-in) operation called when the user interrupts
/// a remote build. It tries to terminate the remote nix process via SSH and
/// pkill, but is inherently fragile due to the nature of remote building
/// semantics.
///
/// # Arguments
///
/// * `host` - The remote host where the process is running
/// * `remote_cmd` - The original command that was run remotely, used for pkill
///   matching
fn attempt_remote_cleanup(host: &RemoteHost, remote_cmd: &str) {
  if !should_cleanup_remote() {
    return;
  }

  let ssh_opts = get_ssh_opts();
  let quoted_cmd = shell_quote(remote_cmd); // for safe passing through pkill's --full argument

  // Build the pkill command:
  // pkill -INT --full '<quoted_cmd>' will match the exact command line
  let pkill_cmd = format!("pkill -INT --full {quoted_cmd}");

  // Build SSH command with stderr capture for diagnostics
  let mut ssh_cmd = Exec::cmd("ssh").stderr(Redirection::Pipe);
  for opt in &ssh_opts {
    ssh_cmd = ssh_cmd.arg(opt);
  }
  ssh_cmd = ssh_cmd.arg(host.ssh_host()).arg(&pkill_cmd);

  debug!("Attempting remote cleanup on '{host}': pkill -INT --full <command>");

  // Use popen with timeout to avoid hanging on unresponsive hosts
  let mut process = match ssh_cmd.popen() {
    Ok(p) => p,
    Err(e) => {
      info!("Failed to execute remote cleanup on '{host}': {e}");
      return;
    },
  };

  // Wait up to 5 seconds for cleanup to complete
  let timeout = Duration::from_secs(5);
  match process.wait_timeout(timeout) {
    Ok(Some(_)) => {
      // Process exited, check status below
    },
    Ok(None) => {
      // Timeout - kill the process and continue
      let _ = process.kill();
      let _ = process.wait();
      info!("Remote cleanup on '{host}' timed out after 5 seconds");
      return;
    },
    Err(e) => {
      info!("Error waiting for remote cleanup on '{host}': {e}");
      return;
    },
  }

  // Check exit status
  if let Some(exit_status) = process.exit_status() {
    if exit_status.success() {
      info!("Cleaned up remote process on '{}'", host);
    } else {
      // Capture stderr for error diagnosis
      let stderr = process.stderr.take().map_or_else(String::new, |mut e| {
        let mut s = String::new();
        let _ = e.read_to_string(&mut s);
        s
      });
      let stderr_lower = stderr.to_lowercase();

      if stderr.contains("No matching processes")
        || stderr_lower.contains("0 processes")
      {
        debug!(
          "No matching process found on '{host}' during cleanup (may have \
           already exited)"
        );
      } else if stderr_lower.contains("not found")
        || stderr_lower.contains("command not found")
      {
        info!("pkill not available on '{}', skipping remote cleanup", host);
      } else if stderr_lower.contains("permission denied")
        || stderr_lower.contains("operation not permitted")
      {
        info!(
          "Permission denied for pkill on '{host}', skipping remote cleanup",
        );
      } else {
        info!("Remote cleanup on '{host}' returned non-zero exit status");
      }
    }
  }
}

/// Get the flake experimental feature flags required for `nix` commands.
///
/// Returns the flags needed for `--extra-experimental-features "nix-command
/// flakes"` based on the detected Nix variant:
///
/// - Determinate Nix: No flags needed (features are stable)
/// - Nix/Lix: Returns `["--extra-experimental-features", "nix-command flakes"]`
///
/// Technically this is inconsistent with our default behaviour, which is to
/// *warn* on missing features but since this is for *remote deployment* it is
/// safer to assist the user instead. Without those features, remote deployment
/// may never succeed.
fn get_flake_flags() -> Vec<&'static str> {
  let variant = crate::util::get_nix_variant();
  match variant {
    NixVariant::Determinate => vec![],
    NixVariant::Nix | NixVariant::Lix => {
      vec!["--extra-experimental-features", "nix-command flakes"]
    },
  }
}

/// Convert `OsString` arguments to UTF-8 Strings.
///
/// Returns an error if any argument is not valid UTF-8.
fn convert_extra_args(extra_args: &[OsString]) -> Result<Vec<String>> {
  extra_args
    .iter()
    .map(|s| {
      s.to_str()
        .map(String::from)
        .ok_or_else(|| eyre!("Extra argument is not valid UTF-8: {:?}", s))
    })
    .collect::<Result<Vec<_>>>()
}

/// Run a command on a remote host via SSH.
fn run_remote_command(
  host: &RemoteHost,
  args: &[&str],
  capture_stdout: bool,
) -> Result<Option<String>> {
  let ssh_opts = get_ssh_opts();

  debug!("Running remote command on {}: {}", host, args.join(" "));

  let quoted_args: Vec<String> = args.iter().map(|s| shell_quote(s)).collect();
  let remote_cmd = quoted_args.join(" ");
  let mut cmd = Exec::cmd("ssh");
  for opt in &ssh_opts {
    cmd = cmd.arg(opt);
  }
  cmd = cmd.arg(host.ssh_host()).arg(&remote_cmd);

  if capture_stdout {
    cmd = cmd.stdout(Redirection::Pipe).stderr(Redirection::Pipe);
  }

  let capture = cmd.capture().wrap_err_with(|| {
    format!("Failed to execute command on remote host '{host}'")
  })?;

  if !capture.exit_status.success() {
    let stderr = capture.stderr_str();
    bail!(
      "Remote command failed on '{}' (exit {:?}):\n{}",
      host,
      capture.exit_status,
      stderr
    );
  }

  if capture_stdout {
    Ok(Some(capture.stdout_str().trim().to_string()))
  } else {
    Ok(None)
  }
}

/// Copy a Nix closure to a remote host.
fn copy_closure_to(
  host: &RemoteHost,
  path: &str,
  use_substitutes: bool,
) -> Result<()> {
  info!("Copying closure to build host '{}'", host);

  let mut cmd = Exec::cmd("nix-copy-closure")
    .arg("--to")
    .arg(host.ssh_host());

  if use_substitutes {
    cmd = cmd.arg("--use-substitutes");
  }

  cmd = cmd.arg(path).env("NIX_SSHOPTS", get_nix_sshopts_env());

  debug!(?cmd, "nix-copy-closure --to");

  let capture = cmd
    .capture()
    .wrap_err("Failed to copy closure to remote host")?;

  if !capture.exit_status.success() {
    bail!(
      "nix-copy-closure --to '{}' failed:\n{}",
      host,
      capture.stderr_str()
    );
  }

  Ok(())
}

/// Validates that essential files exist in a closure on a remote host.
///
/// Performs batched SSH checks using connection multiplexing. This is useful
/// for validating that a system closure contains all necessary files before
/// attempting activation.
///
/// # Arguments
///
/// * `host` - The remote host to check files on
/// * `closure_path` - The base path to the closure (e.g.,
///   `/nix/store/xxx-nixos-system`)
/// * `essential_files` - Slice of (`relative_path`, `description`) tuples for
///   files to validate
/// * `context_info` - Optional context for error messages (e.g., "built on
///   'host1'")
///
/// # Returns
///
/// Returns `Ok(())` if all files exist, or an error describing which files are
/// missing.
///
/// # Errors
///
/// Returns an error if:
///
/// - SSH connection to the remote host fails
/// - Any of the essential files are missing
/// - Path strings contain invalid UTF-8
pub fn validate_closure_remote(
  host: &RemoteHost,
  closure_path: &Path,
  essential_files: &[(&str, &str)],
  context_info: Option<&str>,
) -> Result<()> {
  // Build all test commands for batched execution
  let test_commands: Vec<String> = essential_files
    .iter()
    .map(|(file, _)| {
      let remote_path = closure_path.join(file);
      let path_str = remote_path.to_str().ok_or_else(|| {
        eyre!("Path is not valid UTF-8: {}", remote_path.display())
      })?;
      let quoted_path = shlex::try_quote(path_str).map_err(|_| {
        eyre!("Failed to quote path for shell: {}", remote_path.display())
      })?;
      Ok(format!("test -e {quoted_path}"))
    })
    .collect::<Result<Vec<_>>>()?;

  // Join all tests with &&, so the command fails on first missing file
  // We check each file individually afterward to identify which ones are
  // missing
  let batch_command = test_commands.join(" && ");

  // Get SSH options for connection multiplexing
  let ssh_opts = get_ssh_opts();

  // Execute batch check using SSH with proper connection multiplexing
  let check_result = std::process::Command::new("ssh")
    .args(&ssh_opts)
    .arg(host.ssh_host())
    .arg(&batch_command)
    .output();

  // If batch check succeeds, all files exist
  if let Ok(output) = &check_result {
    if output.status.success() {
      return Ok(());
    }
  }

  // Batch check failed or errored. Identify which files are missing
  let mut missing = Vec::new();
  for ((file, description), test_cmd) in
    essential_files.iter().zip(&test_commands)
  {
    let check_result = std::process::Command::new("ssh")
      .args(&ssh_opts)
      .arg(host.ssh_host())
      .arg(test_cmd)
      .output();

    match check_result {
      Ok(output) if !output.status.success() => {
        missing.push(format!("  - {file} ({description})"));
      },
      Err(e) => {
        return Err(eyre!(
          "Failed to check file existence on remote host {}: {}",
          host,
          e
        ));
      },
      _ => {}, // File exists
    }
  }

  if !missing.is_empty() {
    let missing_list = missing.join("\n");

    // Build context-aware error message
    let host_context = context_info.map_or_else(
      || format!("on remote host '{host}'"),
      |ctx| format!("on remote host '{host}' ({ctx})"),
    );

    return Err(eyre!(
      "Closure validation failed {}.\n\nMissing essential files in store path \
       '{}':\n{}\n\nThis typically happens when:\n1. Required system \
       components are disabled in your configuration\n2. The build was \
       incomplete or corrupted\n3. The Nix store path was not fully copied to \
       the target host\n\nTo fix this:\n1. Verify your configuration enables \
       all required components\n2. Ensure the complete closure was copied: \
       nix copy --to ssh://{} {}\n3. Rebuild your configuration if the \
       problem persists\n4. Use --no-validate to bypass this check if you're \
       certain the system is correctly configured",
      host_context,
      closure_path.display(),
      missing_list,
      host,
      closure_path.display()
    ));
  }

  Ok(())
}

/// Copy a Nix closure from a remote host to localhost.
fn copy_closure_from(
  host: &RemoteHost,
  path: &str,
  use_substitutes: bool,
) -> Result<()> {
  info!("Copying result from build host '{host}'");

  let mut cmd = Exec::cmd("nix-copy-closure")
    .arg("--from")
    .arg(host.ssh_host());

  if use_substitutes {
    cmd = cmd.arg("--use-substitutes");
  }

  cmd = cmd.arg(path).env("NIX_SSHOPTS", get_nix_sshopts_env());

  debug!(?cmd, "nix-copy-closure --from");

  let capture = cmd
    .capture()
    .wrap_err("Failed to copy closure from remote host")?;

  if !capture.exit_status.success() {
    bail!(
      "nix-copy-closure --from '{}' failed:\n{}",
      host,
      capture.stderr_str()
    );
  }

  Ok(())
}

/// Copy a Nix closure from localhost to a remote host.
///
/// Uses `nix copy --to ssh://host` to transfer a store path and its
/// dependencies from the local Nix store to a remote machine via SSH.
///
/// When `use_substitutes` is enabled, the remote host will attempt to fetch
/// missing paths from configured binary caches instead of transferring them
/// over SSH, which can significantly improve performance and reduce bandwidth
/// usage.
///
/// # Arguments
///
/// * `host` - The remote host to copy the closure to. SSH connection
///   multiplexing and options from `NIX_SSHOPTS` are automatically applied.
/// * `path` - The store path to copy (e.g., `/nix/store/xxx-nixos-system`). All
///   dependencies (the complete closure) are copied automatically.
/// * `use_substitutes` - When `true`, adds `--substitute-on-destination` to
///   allow the remote host to fetch missing paths from binary caches instead of
///   transferring them over SSH.
///
/// # Returns
///
/// Returns `Ok(())` on success, or an error if the copy operation fails.
///
/// # Errors
///
/// Returns an error if:
///
/// - The SSH connection to the remote host fails
/// - The `nix copy` command fails (e.g., insufficient disk space on remote,
///   network issues, authentication failures)
/// - The path does not exist in the local store
pub fn copy_to_remote(
  host: &RemoteHost,
  path: &Path,
  use_substitutes: bool,
) -> Result<()> {
  info!("Copying closure to remote host '{}'", host);

  let flake_flags = get_flake_flags();
  let mut cmd = Exec::cmd("nix")
    .args(&flake_flags)
    .args(&["copy", "--to"])
    .arg(format!("ssh://{}", host.ssh_host()));

  if use_substitutes {
    cmd = cmd.arg("--substitute-on-destination");
  }

  cmd = cmd.arg(path).env("NIX_SSHOPTS", get_nix_sshopts_env());

  debug!(?cmd, "nix copy --to");

  let capture = cmd
    .capture()
    .wrap_err("Failed to copy closure to remote host")?;

  if !capture.exit_status.success() {
    bail!("nix copy --to '{}' failed:\n{}", host, capture.stderr_str());
  }

  Ok(())
}

/// Copy a Nix closure from one remote host to another.
/// Uses `nix copy --from ssh://source --to ssh://dest`.
fn copy_closure_between_remotes(
  from_host: &RemoteHost,
  to_host: &RemoteHost,
  path: &str,
  use_substitutes: bool,
) -> Result<()> {
  info!("Copying closure from '{}' to '{}'", from_host, to_host);

  let flake_flags = get_flake_flags();
  let mut cmd = Exec::cmd("nix")
    .args(&flake_flags)
    .args(&["copy", "--from"])
    .arg(format!("ssh://{}", from_host.ssh_host()))
    .arg("--to")
    .arg(format!("ssh://{}", to_host.ssh_host()));

  if use_substitutes {
    cmd = cmd.arg("--substitute-on-destination");
  }

  cmd = cmd.arg(path).env("NIX_SSHOPTS", get_nix_sshopts_env());

  debug!(?cmd, "nix copy between remotes");

  let capture = cmd
    .capture()
    .wrap_err("Failed to copy closure between remote hosts")?;

  if !capture.exit_status.success() {
    bail!(
      "nix copy from '{}' to '{}' failed:\n{}",
      from_host,
      to_host,
      capture.stderr_str()
    );
  }

  Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Represents the type of activation to perform on a remote system.
///
/// This determines which action the system's activation script will execute.
pub enum ActivationType {
  /// Run the configuration in a test mode without activating
  Test,

  /// Atomically switch to the new configuration
  Switch,

  /// Make the new configuration the default boot option
  Boot,
}

impl ActivationType {
  /// Get the string representation used by activation scripts.
  #[must_use]
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Test => "test",
      Self::Switch => "switch",
      Self::Boot => "boot",
    }
  }
}

/// Represents the target platform for remote operations.
///
/// This enum allows the remote module to support multiple platforms while
/// keeping the implementation generic. Currently only NixOS is implemented.
/// Other platforms can be added in the future.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
  /// NixOS system configuration
  NixOS,
  // TODO: Add Darwin and HomeManager support
  //
  // To add support for other platforms:
  //
  // 1. Add the platform variant to this enum
  // 2. Implement platform-specific activation logic in a (private) function
  // 3. Update `activate_remote()` to dispatch to the new platform handler
  // Darwin,
  // HomeManager,
}

/// Configuration for remote activation operations.
#[derive(Debug)]
pub struct ActivateRemoteConfig {
  /// The target platform for activation
  pub platform: Platform,

  /// The type of activation to perform
  pub activation_type: ActivationType,

  /// Whether to install the bootloader during activation
  pub install_bootloader: bool,

  /// Whether to show output logs during activation
  pub show_logs: bool,

  /// Elevation strategy for remote activation commands.
  ///
  /// - `None`: No elevation, run commands as the remote user
  /// - `Some(strategy)`: Use the specified elevation strategy (sudo, doas,
  ///   etc.)
  pub elevation: Option<crate::commands::ElevationStrategy>,
}

/// Activate a system configuration on a remote host.
///
/// Currently only supports NixOS.
///
/// # Arguments
///
/// * `host` - The remote host to activate on
/// * `system_profile` - The path to the NixOS system profile (e.g.,
///   /nix/var/nix/profiles/system)
/// * `config` - Activation configuration options
///
/// # Errors
///
/// Returns an error if SSH connection fails or activation commands fail.
pub fn activate_remote(
  host: &RemoteHost,
  system_profile: &Path,
  config: &ActivateRemoteConfig,
) -> Result<()> {
  match config.platform {
    Platform::NixOS => activate_nixos_remote(host, system_profile, config),
    // TODO:
    // Platform::Darwin => activate_darwin_remote(host, system_profile, config),
    // Platform::HomeManager => activate_home_remote(host, system_profile,
    // config),
  }
}

/// Activate a NixOS system configuration on a remote host.
///
/// Handles the SSH commands required to activate a NixOS system. Supports
/// test, switch, and boot activation types.
///
/// # Arguments
///
/// * `host` - The remote host to activate on
/// * `system_profile` - The path to the NixOS system profile
/// * `config` - Activation configuration options
///
/// # Errors
///
/// Returns an error if SSH connection fails or activation commands fail.
fn activate_nixos_remote(
  host: &RemoteHost,
  system_profile: &Path,
  config: &ActivateRemoteConfig,
) -> Result<()> {
  let ssh_opts = get_ssh_opts();

  // Prompt for password if elevation is needed
  // Skip for None (no elevation) and Passwordless (remote has NOPASSWD
  // configured)
  let sudo_password = if let Some(ref strategy) = config.elevation {
    if matches!(
      strategy,
      ElevationStrategy::None | ElevationStrategy::Passwordless
    ) {
      // None: no elevation program used
      // Passwordless: elevation program used but no password needed
      None
    } else {
      let host_str = host.ssh_host();
      if let Some(cached_password) = get_cached_password(&host_str)? {
        Some(cached_password)
      } else {
        let password =
          inquire::Password::new(&format!("[sudo] password for {host_str}:"))
            .without_confirmation()
            .prompt()
            .context("Failed to read sudo password")?;
        if password.is_empty() {
          bail!("Password cannot be empty");
        }
        let secret_password = SecretString::new(password.into());
        cache_password(&host_str, secret_password.clone())?;
        Some(secret_password)
      }
    }
  } else {
    None
  };

  let switch_to_config = system_profile.join("bin/switch-to-configuration");

  let switch_path_str = switch_to_config.to_str().ok_or_else(|| {
    eyre!("switch-to-configuration path contains invalid UTF-8")
  })?;

  match config.activation_type {
    ActivationType::Test | ActivationType::Switch => {
      let action = config.activation_type.as_str();

      let mut ssh_cmd = Exec::cmd("ssh");
      for opt in &ssh_opts {
        ssh_cmd = ssh_cmd.arg(opt);
      }
      // Add -T flag to disable pseudo-terminal allocation (needed for stdin)
      ssh_cmd = ssh_cmd.arg("-T");
      ssh_cmd = ssh_cmd.arg(host.ssh_host());

      // Build the remote command using helper function
      let base_cmd = format!("{} {}", shell_quote(switch_path_str), action);
      let remote_cmd =
        build_remote_command(config.elevation.as_ref(), &base_cmd)?;

      ssh_cmd = ssh_cmd.arg(remote_cmd);

      // Pass password via stdin if elevation is needed
      if let Some(ref password) = sudo_password {
        ssh_cmd =
          ssh_cmd.stdin(format!("{}\n", password.expose_secret()).as_str());
      }

      if config.show_logs {
        ssh_cmd = ssh_cmd
          .stdout(Redirection::Merge)
          .stderr(Redirection::Merge);
      }

      debug!(?ssh_cmd, "Activating NixOS configuration");

      let capture = ssh_cmd
        .capture()
        .wrap_err("Failed to activate NixOS configuration")?;

      if !capture.exit_status.success() {
        bail!(
          "Activation ({}) failed on '{}':\n{}",
          action,
          host,
          capture.stderr_str()
        );
      }
    },

    ActivationType::Boot => {
      let mut profile_ssh_cmd = Exec::cmd("ssh");
      for opt in &ssh_opts {
        profile_ssh_cmd = profile_ssh_cmd.arg(opt);
      }
      // Add -T flag to disable pseudo-terminal allocation (needed for stdin)
      profile_ssh_cmd = profile_ssh_cmd.arg("-T");
      profile_ssh_cmd = profile_ssh_cmd.arg(host.ssh_host());

      // Build the remote command using helper function
      let base_cmd = format!(
        "nix build --no-link --profile {} {}",
        NIXOS_SYSTEM_PROFILE,
        shell_quote(&system_profile.to_string_lossy())
      );
      let profile_remote_cmd =
        build_remote_command(config.elevation.as_ref(), &base_cmd)?;

      profile_ssh_cmd = profile_ssh_cmd.arg(profile_remote_cmd);

      // Pass password via stdin if elevation is needed
      if let Some(ref password) = sudo_password {
        profile_ssh_cmd = profile_ssh_cmd
          .stdin(format!("{}\n", password.expose_secret()).as_str());
      }

      debug!(?profile_ssh_cmd, "Setting NixOS profile");

      let profile_capture = profile_ssh_cmd
        .capture()
        .wrap_err("Failed to set NixOS profile")?;

      if !profile_capture.exit_status.success() {
        bail!(
          "Failed to set system profile on '{}':\n{}",
          host,
          profile_capture.stderr_str()
        );
      }

      let mut boot_ssh_cmd = Exec::cmd("ssh");
      for opt in &ssh_opts {
        boot_ssh_cmd = boot_ssh_cmd.arg(opt);
      }
      // Add -T flag to disable pseudo-terminal allocation (needed for stdin)
      boot_ssh_cmd = boot_ssh_cmd.arg("-T");
      boot_ssh_cmd = boot_ssh_cmd.arg(host.ssh_host());

      // Build the remote command using helper function
      let boot_remote_cmd = if config.install_bootloader {
        let base_cmd = format!(
          "NIXOS_INSTALL_BOOTLOADER=1 {} boot",
          shell_quote(switch_path_str)
        );
        build_remote_command(config.elevation.as_ref(), &base_cmd)?
      } else {
        let base_cmd = format!("{} boot", shell_quote(switch_path_str));
        build_remote_command(config.elevation.as_ref(), &base_cmd)?
      };

      boot_ssh_cmd = boot_ssh_cmd.arg(boot_remote_cmd);

      // Pass password via stdin if elevation is needed
      if let Some(ref password) = sudo_password {
        boot_ssh_cmd = boot_ssh_cmd
          .stdin(format!("{}\n", password.expose_secret()).as_str());
      }

      debug!(?boot_ssh_cmd, "Bootloader activation");

      let boot_capture = boot_ssh_cmd
        .capture()
        .wrap_err("Bootloader activation failed")?;

      if !boot_capture.exit_status.success() {
        bail!(
          "Bootloader activation failed on '{}':\n{}",
          host,
          boot_capture.stderr_str()
        );
      }
    },
  }

  Ok(())
}

/// System profile path for NixOS.
/// Used by remote activation functions.
const NIXOS_SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";

/// Evaluate a flake installable to get its derivation path.
/// Matches nixos-rebuild-ng: `nix eval --raw <flake>.drvPath`
fn eval_drv_path(installable: &Installable) -> Result<String> {
  // Build the installable with .drvPath appended
  let drv_installable = match installable {
    Installable::Flake {
      reference,
      attribute,
    } => {
      let mut drv_attr = attribute.clone();
      drv_attr.push("drvPath".to_string());
      Installable::Flake {
        reference: reference.clone(),
        attribute: drv_attr,
      }
    },
    Installable::File { path, attribute } => {
      let mut drv_attr = attribute.clone();
      drv_attr.push("drvPath".to_string());
      Installable::File {
        path:      path.clone(),
        attribute: drv_attr,
      }
    },
    Installable::Expression {
      expression,
      attribute,
    } => {
      let mut drv_attr = attribute.clone();
      drv_attr.push("drvPath".to_string());
      Installable::Expression {
        expression: expression.clone(),
        attribute:  drv_attr,
      }
    },
    Installable::Store { path } => {
      bail!(
        "Cannot perform remote build with store path '{}'. Store paths are \
         already built.",
        path.display()
      );
    },
    Installable::Unspecified => {
      bail!("Cannot evaluate unspecified installable");
    },
  };

  let args = drv_installable.to_args();
  debug!("Evaluating drvPath: nix eval --raw {:?}", args);

  let flake_flags = get_flake_flags();
  let cmd = Exec::cmd("nix")
    .args(&flake_flags)
    .arg("eval")
    .arg("--raw")
    .args(&args)
    .stdout(Redirection::Pipe)
    .stderr(Redirection::Pipe);

  let capture = cmd.capture().wrap_err("Failed to run nix eval")?;

  if !capture.exit_status.success() {
    bail!(
      "Failed to evaluate derivation path:\n{}",
      capture.stderr_str()
    );
  }

  let drv_path = capture.stdout_str().trim().to_string();
  if drv_path.is_empty() {
    bail!("nix eval returned empty derivation path");
  }

  debug!("Derivation path: {}", drv_path);
  Ok(drv_path)
}

/// Configuration for a remote build operation.
///
/// # Host Interaction Semantics
///
/// The behavior depends on which hosts are specified:
///
/// | `build_host` | `target_host` | Behavior |
/// |--------------|---------------|----------|
/// | Some(H2)     | None          | Build on H2, copy result to localhost |
/// | Some(H2)     | Some(H2)      | Build on H2, no copy (build host = target) |
/// | Some(H2)     | Some(H3)      | Build on H2, try direct copy to H3; if that fails, relay through localhost |
///
/// When `build_host` and `target_host` differ, the code attempts a direct
/// copy between remotes first. If this fails (common when the hosts can't
/// see each other), it falls back to relaying through localhost:
///
/// - Direct: Host2 -> Host3
/// - Fallback: Host2 -> Host1 (localhost) → Host3
///
/// If `out_link` is requested in `build_remote()`, the result is always
/// copied to localhost regardless of whether a direct copy succeeded,
/// because the symlink must point to a local store path.
#[derive(Debug, Clone)]
pub struct RemoteBuildConfig {
  /// The host to build on
  pub build_host: RemoteHost,

  /// Optional target host to copy the result to (instead of localhost).
  /// When set, copies directly from `build_host` to `target_host`.
  pub target_host: Option<RemoteHost>,

  /// Whether to use nix-output-monitor for build output
  pub use_nom: bool,

  /// Whether to use substitutes when copying closures
  pub use_substitutes: bool,

  /// Extra arguments to pass to the build command
  pub extra_args: Vec<OsString>,
}

/// Perform a remote build of a flake installable.
///
/// This implements the `build_remote_flake` workflow from nixos-rebuild-ng:
/// 1. Evaluate drvPath locally via `nix eval --raw`
/// 2. Copy the derivation to the build host via `nix-copy-closure`
/// 3. Build on remote host via `nix build <drv>^* --print-out-paths`
/// 4. Copy the result back (to localhost or `target_host`)
///
/// Returns the output path in the Nix store.
///
/// # Errors
///
/// Returns an error if any step fails (evaluation, copy, build).
pub fn build_remote(
  installable: &Installable,
  config: &RemoteBuildConfig,
  out_link: Option<&std::path::Path>,
) -> Result<PathBuf> {
  let build_host = &config.build_host;
  let use_substitutes = config.use_substitutes;

  // Step 1: Evaluate drvPath locally
  info!("Evaluating derivation path");
  let drv_path = eval_drv_path(installable)?;

  // Step 2: Copy derivation to build host
  copy_closure_to(build_host, &drv_path, use_substitutes)?;

  // Step 3: Build on remote
  info!("Building on remote host '{}'", build_host);
  let out_path = build_on_remote(build_host, &drv_path, config)?;

  // Step 4: Copy result to destination
  //
  // Optimizes copy paths based on hostname comparison:
  // - When build_host != target_host: copy build -> target, then build -> local
  //   if needed
  // - When build_host == target_host: skip redundant copies, only copy to local
  //   if out-link is needed
  // - When target_host is None: always copy build -> local
  let target_is_build_host = config
    .target_host
    .as_ref()
    .is_some_and(|th| th.hostname() == build_host.hostname());

  let need_local_copy = match &config.target_host {
    None => true,
    Some(_target_host) if target_is_build_host => {
      debug!(
        "Skipping copy from build host to target host (same host: {})",
        build_host.hostname()
      );

      // When build_host == target_host and both are remote, the result is
      // already where it needs to be. No need to copy to localhost even if
      // out_link is requested, since the closure will be activated remotely.
      // This is a little confusing, but frankly, respecting --out-link to
      // create a local path while everything happens remotely is a bit
      // more confusing.
      false
    },
    Some(target_host) => {
      match copy_closure_between_remotes(
        build_host,
        target_host,
        &out_path,
        use_substitutes,
      ) {
        Ok(()) => {
          debug!(
            "Successfully copied closure directly from {} to {}",
            build_host.hostname(),
            target_host.hostname()
          );
          out_link.is_some()
        },
        Err(e) => {
          warn!(
            "Direct copy from {} to {} failed: {}. Will relay through \
             localhost.",
            build_host.hostname(),
            target_host.hostname(),
            e
          );
          true
        },
      }
    },
  };

  if need_local_copy {
    copy_closure_from(build_host, &out_path, use_substitutes)?;
  }

  // Create local out-link if requested and the result is in local store
  // When build_host == target_host (both remote), skip out-link creation
  // since the closure is remote and won't be copied to localhost
  if let Some(link) = out_link {
    if need_local_copy {
      debug!("Creating out-link: {} -> {}", link.display(), out_path);
      // Remove existing symlink/file if present
      let _ = std::fs::remove_file(link);
      std::os::unix::fs::symlink(&out_path, link)
        .wrap_err("Failed to create out-link")?;
    } else {
      debug!(
        "Skipping out-link creation: result is on remote host and not copied \
         to localhost"
      );
    }
  }

  Ok(PathBuf::from(out_path))
}

/// Build a derivation on a remote host.
/// Returns the output path.
fn build_on_remote(
  host: &RemoteHost,
  drv_path: &str,
  config: &RemoteBuildConfig,
) -> Result<String> {
  // Build command: nix build <drv>^* --print-out-paths [extra_args...]
  let drv_with_outputs = format!("{drv_path}^*");

  if config.use_nom {
    // Check that nom is available before attempting to use it
    which::which("nom")
      .wrap_err("nom (nix-output-monitor) is required but not found in PATH")?;

    // With nom: pipe through nix-output-monitor
    build_on_remote_with_nom(host, &drv_with_outputs, config)
  } else {
    // Without nom: simple remote execution
    build_on_remote_simple(host, &drv_with_outputs, config)
  }
}

/// Build the argument list for remote nix build commands.
/// Returns owned strings to avoid lifetime issues with `extra_args`.
fn build_nix_command(
  drv_with_outputs: &str,
  extra_flags: &[&str],
  extra_args: &[OsString],
) -> Result<Vec<String>> {
  let flake_flags = get_flake_flags();
  let extra_args_strings = convert_extra_args(extra_args)?;

  let mut args = vec!["nix".to_string()];
  args.extend(flake_flags.iter().map(|s| (*s).to_string()));
  args.push("build".to_string());
  args.push(drv_with_outputs.to_string());
  args.extend(extra_flags.iter().map(|s| (*s).to_string()));
  args.extend(extra_args_strings);

  Ok(args)
}

/// Build on remote without nom - just capture output.
fn build_on_remote_simple(
  host: &RemoteHost,
  drv_with_outputs: &str,
  config: &RemoteBuildConfig,
) -> Result<String> {
  // Register interrupt handler at start
  register_interrupt_handler()?;

  let ssh_opts = get_ssh_opts();

  let args = build_nix_command(
    drv_with_outputs,
    &["--print-out-paths"],
    &config.extra_args,
  )?;
  let arg_refs: Vec<&str> =
    args.iter().map(std::string::String::as_str).collect();

  // Build SSH command with stdout capture
  // Quote all arguments for safe shell passing
  let quoted_args: Vec<String> =
    arg_refs.iter().map(|s| shell_quote(s)).collect();
  let remote_cmd = quoted_args.join(" ");

  let mut ssh_cmd = Exec::cmd("ssh");
  for opt in &ssh_opts {
    ssh_cmd = ssh_cmd.arg(opt);
  }
  ssh_cmd = ssh_cmd
    .arg(host.ssh_host())
    .arg(&remote_cmd)
    .stdout(Redirection::Pipe)
    .stderr(Redirection::Pipe);

  // Execute with popen to get process handle
  let mut process = ssh_cmd.popen()?;

  // Wait for completion with interrupt checking
  let exit_status = loop {
    match process.wait_timeout(std::time::Duration::from_millis(100))? {
      Some(status) => break status,
      None => {
        // Check interrupt flag while waiting
        if get_interrupt_flag().load(Ordering::Relaxed) {
          debug!("Interrupt detected, killing SSH process");

          let _ = process.kill();
          let _ = process.wait(); // reap zombie

          // Attempt remote cleanup if enabled
          attempt_remote_cleanup(host, &remote_cmd);

          bail!("Operation interrupted by user");
        }
      },
    }
  };

  // Check exit status
  if !exit_status.success() {
    let stderr = process
      .stderr
      .take()
      .and_then(|mut e| {
        let mut s = String::new();
        e.read_to_string(&mut s).ok().map(|_| s)
      })
      .unwrap_or_else(|| String::from("(no stderr)"));
    bail!("Remote command failed: {}", stderr);
  }

  // Read stdout
  let stdout = process
    .stdout
    .take()
    .ok_or_else(|| eyre!("Failed to capture stdout"))?;
  let mut reader = std::io::BufReader::new(stdout);
  let mut output = String::new();
  reader.read_to_string(&mut output)?;

  // --print-out-paths may return multiple lines; take first
  let out_path = output
    .lines()
    .next()
    .ok_or_else(|| eyre!("Remote build returned empty output"))?
    .trim()
    .to_string();

  debug!("Remote build output: {}", out_path);
  Ok(out_path)
}

/// Build on remote with nom - pipe through nix-output-monitor.
fn build_on_remote_with_nom(
  host: &RemoteHost,
  drv_with_outputs: &str,
  config: &RemoteBuildConfig,
) -> Result<String> {
  // Register interrupt handler at start
  register_interrupt_handler()?;

  let ssh_opts = get_ssh_opts();

  // Build the remote command with JSON output for nom
  let remote_args = build_nix_command(
    drv_with_outputs,
    &["--log-format", "internal-json", "--verbose"],
    &config.extra_args,
  )?;
  let arg_refs: Vec<&str> = remote_args
    .iter()
    .map(std::string::String::as_str)
    .collect();

  // Build SSH command
  // Quote all arguments for safe shell passing
  let quoted_remote: Vec<String> =
    arg_refs.iter().map(|s| shell_quote(s)).collect();
  let remote_cmd = quoted_remote.join(" ");

  let mut ssh_cmd = Exec::cmd("ssh");
  for opt in &ssh_opts {
    ssh_cmd = ssh_cmd.arg(opt);
  }
  ssh_cmd = ssh_cmd
    .arg(host.ssh_host())
    .arg(&remote_cmd)
    .stdout(Redirection::Pipe)
    .stderr(Redirection::Merge);

  // Pipe through nom
  let nom_cmd = Exec::cmd("nom").arg("--json");
  let pipeline = (ssh_cmd | nom_cmd).stdout(Redirection::None);

  debug!(?pipeline, "Running remote build with nom");

  // Use popen() to get access to individual processes so we can check
  // ssh's exit status, not nom's. The pipeline's join() only returns
  // the exit status of the last command (nom), which always succeeds
  // even when the remote nix command fails.
  let mut processes =
    pipeline.popen().wrap_err("Remote build with nom failed")?;

  // Use wait_timeout in a polling loop to check interrupt flag every 100ms
  let poll_interval = Duration::from_millis(100);

  for proc in &mut processes {
    #[allow(
      clippy::needless_continue,
      reason = "Better for explicitness and consistency"
    )]
    loop {
      // Check interrupt flag before waiting
      if get_interrupt_flag().load(Ordering::Relaxed) {
        debug!("Interrupt detected during build with nom");
        // Kill remaining local processes. This will cause SSH to terminate
        // the remote command automatically
        for p in &mut processes {
          let _ = p.kill();
          let _ = p.wait(); // reap zombie
        }

        // Attempt remote cleanup if enabled
        attempt_remote_cleanup(host, &remote_cmd);

        bail!("Operation interrupted by user");
      }

      // Poll process with timeout
      match proc.wait_timeout(poll_interval)? {
        Some(_) => {
          // Process has exited, exit status is automatically cached in the
          // Popen struct Move to next process
          break;
        },

        None => {
          // Timeout elapsed, process still running - loop continues
          // and will check interrupt flag again
          continue;
        },
      }
    }
  }

  // Check the exit status of the FIRST process (ssh -> nix build)
  // This is the one that matters. If the remote build fails, we should fail
  // too
  if let Some(ssh_proc) = processes.first() {
    if let Some(exit_status) = ssh_proc.exit_status() {
      match exit_status {
        ExitStatus::Exited(0) => {},
        other => bail!("Remote build failed with exit status: {other:?}"),
      }
    }
  }

  // nom consumed the output, so we need to query the output path separately
  // Run nix build again with --print-out-paths (it will be a no-op since
  // already built)
  let query_args =
    build_nix_command(drv_with_outputs, &["--print-out-paths"], &[])?;
  let query_refs: Vec<&str> =
    query_args.iter().map(std::string::String::as_str).collect();

  let result = run_remote_command(host, &query_refs, true);

  // Check if interrupted during query
  if get_interrupt_flag().load(Ordering::Relaxed) {
    debug!("Interrupt detected during output path query");
    bail!("Operation interrupted by user");
  }

  let result =
    result?.ok_or_else(|| eyre!("Failed to get output path after build"))?;

  let out_path = result
    .lines()
    .next()
    .ok_or_else(|| eyre!("Output path query returned empty"))?
    .trim()
    .to_string();

  debug!("Remote build output: {}", out_path);
  Ok(out_path)
}

#[cfg(test)]
mod tests {
  #![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "Fine in tests"
  )]
  use proptest::prelude::*;
  use serial_test::serial;

  use super::*;

  proptest! {
    #[test]
    fn hostname_always_returns_suffix_after_last_at(s in "\\PC*") {
        let host = RemoteHost { host: s.clone() };
        let expected = s.rsplit('@').next().unwrap();
        prop_assert_eq!(host.hostname(), expected);
    }

    #[test]
    fn hostname_is_substring_of_host(s in "\\PC*") {
        let host = RemoteHost { host: s.clone() };
        prop_assert!(s.contains(host.hostname()));
    }

    #[test]
    fn hostname_no_at_means_whole_string(s in "[^@]*") {
        let host = RemoteHost { host: s.clone() };
        prop_assert_eq!(host.hostname(), s);
    }

    #[test]
    fn hostname_with_user(user in "[a-zA-Z0-9_]+", hostname in "[a-zA-Z0-9_.-]+") {
        let full = format!("{user}@{hostname}");
        let host = RemoteHost { host: full };
        prop_assert_eq!(host.hostname(), hostname);
    }

    #[test]
    fn parse_valid_bare_hostname(hostname in "[a-zA-Z0-9_.-]+") {
        let result = RemoteHost::parse(&hostname);
        prop_assert!(result.is_ok());
        let host = result.unwrap();
        prop_assert_eq!(host.hostname(), hostname);
    }

    #[test]
    fn parse_valid_user_at_hostname(user in "[a-zA-Z0-9_]+", hostname in "[a-zA-Z0-9_.-]+") {
        let full = format!("{user}@{hostname}");
        let result = RemoteHost::parse(&full);
        prop_assert!(result.is_ok());
        let host = result.unwrap();
        prop_assert_eq!(host.hostname(), hostname);
    }
  }

  #[test]
  fn test_parse_bare_hostname() {
    let host = RemoteHost::parse("buildserver").expect("should parse");
    assert_eq!(host.to_string(), "buildserver");
  }

  #[test]
  fn test_parse_user_at_hostname() {
    let host = RemoteHost::parse("root@buildserver").expect("should parse");
    assert_eq!(host.to_string(), "root@buildserver");
  }

  #[test]
  fn test_parse_ssh_uri_stripped() {
    let host = RemoteHost::parse("ssh://buildserver").expect("should parse");
    assert_eq!(host.to_string(), "buildserver");
  }

  #[test]
  fn test_parse_ssh_ng_uri_stripped() {
    let host = RemoteHost::parse("ssh-ng://buildserver").expect("should parse");
    assert_eq!(host.to_string(), "buildserver");
  }

  #[test]
  fn test_parse_ssh_uri_with_user() {
    let host =
      RemoteHost::parse("ssh://root@buildserver").expect("should parse");
    assert_eq!(host.to_string(), "root@buildserver");
  }

  #[test]
  fn test_parse_ssh_ng_uri_with_user() {
    let host =
      RemoteHost::parse("ssh-ng://admin@buildserver").expect("should parse");
    assert_eq!(host.to_string(), "admin@buildserver");
  }

  #[test]
  fn test_parse_empty_fails() {
    assert!(RemoteHost::parse("").is_err());
  }

  #[test]
  fn test_parse_empty_user_fails() {
    assert!(RemoteHost::parse("@hostname").is_err());
  }

  #[test]
  fn test_parse_empty_hostname_fails() {
    assert!(RemoteHost::parse("user@").is_err());
  }

  #[test]
  fn test_parse_port_rejected() {
    let Err(err) = RemoteHost::parse("hostname:22") else {
      panic!("expected error for port in hostname");
    };
    assert!(err.to_string().contains("NIX_SSHOPTS"));
  }

  #[test]
  fn test_parse_ipv6_bracketed() {
    let host = RemoteHost::parse("[2001:db8::1]").expect("should parse IPv6");
    assert_eq!(host.to_string(), "[2001:db8::1]");
    assert_eq!(host.hostname(), "[2001:db8::1]");
  }

  #[test]
  fn test_parse_ipv6_with_user() {
    let host = RemoteHost::parse("root@[2001:db8::1]")
      .expect("should parse IPv6 with user");
    assert_eq!(host.to_string(), "root@[2001:db8::1]");
    assert_eq!(host.hostname(), "[2001:db8::1]");
  }

  #[test]
  fn test_parse_ipv6_with_zone_id() {
    let host =
      RemoteHost::parse("[fe80::1%eth0]").expect("should parse IPv6 with zone");
    assert_eq!(host.to_string(), "[fe80::1%eth0]");
  }

  #[test]
  fn test_parse_ipv6_ssh_uri() {
    let host = RemoteHost::parse("ssh://[2001:db8::1]")
      .expect("should parse IPv6 SSH URI");
    assert_eq!(host.to_string(), "[2001:db8::1]");
  }

  #[test]
  fn test_parse_ipv6_ssh_uri_with_user() {
    let host = RemoteHost::parse("ssh://root@[2001:db8::1]")
      .expect("should parse IPv6 SSH URI with user");
    assert_eq!(host.to_string(), "root@[2001:db8::1]");
  }

  #[test]
  fn test_parse_ipv6_localhost() {
    let host = RemoteHost::parse("[::1]").expect("should parse IPv6 localhost");
    assert_eq!(host.to_string(), "[::1]");
  }

  #[test]
  fn test_parse_ipv6_compressed() {
    let host =
      RemoteHost::parse("[2001:db8::]").expect("should parse compressed IPv6");
    assert_eq!(host.to_string(), "[2001:db8::]");
  }

  #[test]
  fn test_parse_ipv6_unbracketed_rejected() {
    // Bare IPv6 without brackets should be rejected
    let result = RemoteHost::parse("2001:db8::1");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("NIX_SSHOPTS"));
  }

  #[test]
  fn test_parse_ipv6_mismatched_brackets_rejected() {
    assert!(RemoteHost::parse("[2001:db8::1").is_err());
    assert!(RemoteHost::parse("2001:db8::1]").is_err());
  }

  #[test]
  fn test_parse_ipv6_extra_brackets_rejected() {
    assert!(RemoteHost::parse("[[2001:db8::1]]").is_err());
    assert!(RemoteHost::parse("[2001:db8::[1]]").is_err());
  }

  #[test]
  fn test_parse_ipv6_with_port_rejected() {
    // IPv6 with port syntax should be rejected (use NIX_SSHOPTS)
    let result = RemoteHost::parse("[2001:db8::1]:22");
    assert!(result.is_err());
  }

  #[test]
  fn test_parse_ipv6_chars_after_bracket_rejected() {
    // Characters after closing bracket should be rejected
    let result = RemoteHost::parse("[2001:db8::1]extra");
    assert!(result.is_err());
  }

  #[test]
  fn test_parse_ipv6_at_inside_brackets_rejected() {
    // @ character inside brackets should be rejected (not valid IPv6)
    // This ensures [@2001:db8::1] and [2001@db8::1] are both rejected
    let result = RemoteHost::parse("[@2001:db8::1]");
    assert!(result.is_err(), "[@2001:db8::1] should be rejected");

    let result2 = RemoteHost::parse("[2001@db8::1]");
    assert!(result2.is_err(), "[2001@db8::1] should be rejected");
  }

  #[test]
  fn test_ssh_host_ipv6_strips_brackets() {
    let host = RemoteHost::parse("[2001:db8::1]").expect("should parse IPv6");
    assert_eq!(host.ssh_host(), "2001:db8::1");
  }

  #[test]
  fn test_ssh_host_ipv6_with_user() {
    let host = RemoteHost::parse("user@[2001:db8::1]").expect("should parse");
    assert_eq!(host.ssh_host(), "user@2001:db8::1");
  }

  #[test]
  fn test_ssh_host_ipv6_with_zone_id() {
    let host = RemoteHost::parse("[fe80::1%eth0]").expect("should parse");
    assert_eq!(host.ssh_host(), "fe80::1%eth0");
  }

  #[test]
  fn test_ssh_host_ipv6_with_zone_id_and_user() {
    let host = RemoteHost::parse("user@[fe80::1%eth0]").expect("should parse");
    assert_eq!(host.ssh_host(), "user@fe80::1%eth0");
  }

  #[test]
  fn test_ssh_host_ipv6_localhost() {
    let host = RemoteHost::parse("[::1]").expect("should parse");
    assert_eq!(host.ssh_host(), "::1");
  }

  #[test]
  fn test_ssh_host_non_ipv6_unchanged() {
    let host = RemoteHost::parse("host.example").expect("should parse");
    assert_eq!(host.ssh_host(), "host.example");
  }

  #[test]
  fn test_ssh_host_non_ipv6_with_user() {
    let host = RemoteHost::parse("user@host.example").expect("should parse");
    assert_eq!(host.ssh_host(), "user@host.example");
  }

  #[test]
  fn test_ssh_host_ssh_uri_ipv6() {
    let host = RemoteHost::parse("ssh://[2001:db8::1]").expect("should parse");
    assert_eq!(host.ssh_host(), "2001:db8::1");
  }

  #[test]
  fn test_ssh_host_ssh_uri_ipv6_with_user() {
    let host =
      RemoteHost::parse("ssh://root@[2001:db8::1]").expect("should parse");
    assert_eq!(host.ssh_host(), "root@2001:db8::1");
  }

  #[test]
  fn test_shell_quote_simple() {
    assert_eq!(shell_quote("simple"), "simple");
    assert_eq!(
      shell_quote("/nix/store/abc123-foo"),
      "/nix/store/abc123-foo"
    );
  }

  #[test]
  #[serial]
  fn test_get_ssh_opts_default() {
    // Clear env var for test
    unsafe {
      std::env::remove_var("NIX_SSHOPTS");
    }
    let opts = get_ssh_opts();
    assert!(opts.contains(&"-o".to_string()));
    assert!(opts.contains(&"ControlMaster=auto".to_string()));
    assert!(opts.contains(&"ControlPersist=60".to_string()));
    // Check that ControlPath is present (the exact path varies)
    assert!(opts.iter().any(|o| o.starts_with("ControlPath=")));
  }

  #[test]
  #[serial]
  fn test_get_ssh_opts_with_simple_nix_sshopts() {
    unsafe {
      std::env::set_var("NIX_SSHOPTS", "-p 2222 -i /path/to/key");
    }
    let opts = get_ssh_opts();
    // User options should be included
    assert!(opts.contains(&"-p".to_string()));
    assert!(opts.contains(&"2222".to_string()));
    assert!(opts.contains(&"-i".to_string()));
    assert!(opts.contains(&"/path/to/key".to_string()));
    // Default options should still be present
    assert!(opts.contains(&"ControlMaster=auto".to_string()));
    unsafe {
      std::env::remove_var("NIX_SSHOPTS");
    }
  }

  #[test]
  #[serial]
  fn test_get_ssh_opts_with_quoted_nix_sshopts() {
    // Test that quoted paths with spaces are handled correctly
    unsafe {
      std::env::set_var("NIX_SSHOPTS", r#"-i "/path/with spaces/key""#);
    }
    let opts = get_ssh_opts();
    // The path should be parsed as a single argument without quotes
    assert!(opts.contains(&"-i".to_string()));
    assert!(opts.contains(&"/path/with spaces/key".to_string()));
    // Default options should still be present
    assert!(opts.contains(&"ControlMaster=auto".to_string()));
    unsafe {
      std::env::remove_var("NIX_SSHOPTS");
    }
  }

  #[test]
  #[serial]
  fn test_get_ssh_opts_with_option_value_nix_sshopts() {
    // Test -o with quoted value containing spaces
    unsafe {
      std::env::set_var(
        "NIX_SSHOPTS",
        r#"-o "ProxyCommand=ssh -W %h:%p jump""#,
      );
    }
    let opts = get_ssh_opts();
    assert!(opts.contains(&"-o".to_string()));
    assert!(opts.contains(&"ProxyCommand=ssh -W %h:%p jump".to_string()));
    unsafe {
      std::env::remove_var("NIX_SSHOPTS");
    }
  }

  #[test]
  fn test_shell_quote_behavior() {
    // Verify shell_quote adds quotes when needed
    assert_eq!(shell_quote("simple"), "simple");
    assert_eq!(shell_quote("has space"), "'has space'");
    // shlex::try_quote uses double quotes when string contains single quote
    assert_eq!(shell_quote("has'quote"), "\"has'quote\"");
  }

  #[test]
  fn test_shell_quote_roundtrip() {
    // Test that quoting and then parsing gives back the original
    let test_cases = vec![
      "simple",
      "/nix/store/abc123-foo",
      "has space",
      "has'quote",
      "has\"doublequote",
      "$(dangerous)",
      "path/with spaces/and'quotes",
    ];

    for original in test_cases {
      let quoted = shell_quote(original);
      // Parse the quoted string back - should give single element
      let parsed = shlex::split(&quoted);
      assert!(
        parsed.is_some(),
        "Failed to parse quoted string for: {original}"
      );
      let parsed = parsed.expect("checked above");
      assert_eq!(
        parsed.len(),
        1,
        "Expected single element for: {original}, got: {parsed:?}"
      );
      assert_eq!(
        parsed[0], original,
        "Roundtrip failed for: {original}, quoted as: {quoted}"
      );
    }
  }

  #[test]
  fn test_shell_quote_nix_drv_output() {
    // Test the drv^* syntax used by nix
    let drv_path = "/nix/store/abc123.drv^*";
    let quoted = shell_quote(drv_path);
    let parsed = shlex::split(&quoted).expect("should parse");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0], drv_path);
  }

  #[test]
  fn test_shell_quote_preserves_equals() {
    // Environment variable assignments should work
    let env_var = "PATH=/usr/bin:/bin";
    let quoted = shell_quote(env_var);
    let parsed = shlex::split(&quoted).expect("should parse");
    assert_eq!(parsed[0], env_var);
  }

  #[test]
  fn test_shell_quote_unicode() {
    // Unicode should be preserved
    let unicode = "path/with/émojis/🚀";
    let quoted = shell_quote(unicode);
    let parsed = shlex::split(&quoted).expect("should parse");
    assert_eq!(parsed[0], unicode);
  }

  #[test]
  #[serial]
  fn test_get_nix_sshopts_env_empty() {
    unsafe {
      std::env::remove_var("NIX_SSHOPTS");
    }
    let result = get_nix_sshopts_env();
    // Should contain our defaults as space-separated values
    assert!(result.contains("-o"));
    assert!(result.contains("ControlMaster=auto"));
    assert!(result.contains("ControlPersist=60"));
    // Should contain ControlPath (exact path varies)
    assert!(result.contains("ControlPath="));
  }

  #[test]
  #[serial]
  fn test_get_nix_sshopts_env_simple() {
    unsafe {
      std::env::set_var("NIX_SSHOPTS", "-p 2222");
    }
    let result = get_nix_sshopts_env();
    // User options should come first
    assert!(result.starts_with("-p 2222"));
    // Defaults should be appended
    assert!(result.contains("ControlMaster=auto"));
    unsafe {
      std::env::remove_var("NIX_SSHOPTS");
    }
  }

  #[test]
  #[serial]
  fn test_get_nix_sshopts_env_preserves_user_opts() {
    // User options are preserved as-is (nix-copy-closure does whitespace split)
    unsafe {
      std::env::set_var("NIX_SSHOPTS", "-i /path/to/key -p 22");
    }
    let result = get_nix_sshopts_env();
    // User options preserved at start
    assert!(result.starts_with("-i /path/to/key -p 22"));
    // Our defaults appended
    assert!(result.contains("ControlMaster=auto"));
    unsafe {
      std::env::remove_var("NIX_SSHOPTS");
    }
  }

  #[test]
  #[serial]
  fn test_get_nix_sshopts_env_no_extra_quoting() {
    // Verify we don't add shell quotes (nix-copy-closure doesn't parse them)
    unsafe {
      std::env::remove_var("NIX_SSHOPTS");
    }
    let result = get_nix_sshopts_env();
    // Should NOT contain shell quote characters around our options
    assert!(!result.contains("'ControlMaster"));
    assert!(!result.contains("\"ControlMaster"));
    // Values should be bare
    assert!(result.contains("-o ControlMaster=auto"));
  }

  #[test]
  fn test_hostname_comparison_for_same_host() {
    let host1 = RemoteHost::parse("user1@host.example").unwrap();
    let host2 = RemoteHost::parse("user2@host.example").unwrap();
    let host3 = RemoteHost::parse("host.example").unwrap();
    let host4 = RemoteHost::parse("other.host").unwrap();

    assert_eq!(host1.hostname(), "host.example");
    assert_eq!(host2.hostname(), "host.example");
    assert_eq!(host3.hostname(), "host.example");
    assert_eq!(host4.hostname(), "other.host");

    assert_eq!(host1.hostname(), host2.hostname());
    assert_eq!(host1.hostname(), host3.hostname());
    assert_ne!(host1.hostname(), host4.hostname());
  }

  #[test]
  fn test_get_ssh_control_dir_creates_directory() {
    let dir = get_ssh_control_dir();
    // The directory should exist in normal operation. In extreme edge cases
    // (read-only /tmp), the function returns a path that may not exist, and
    // SSH will fail with a clear error when attempting to use it.
    assert!(
      dir.exists(),
      "Control dir should exist in normal operation: {}",
      dir.display()
    );

    // Should contain our process-specific suffix
    let dir_str = dir.to_string_lossy();
    assert!(
      dir_str.contains("nh-ssh-"),
      "Control dir should contain 'nh-ssh-': {dir_str}"
    );
  }

  #[test]
  fn test_init_ssh_control_returns_guard() {
    // Verify that init_ssh_control() returns a guard
    // and that the guard holds the correct control directory
    let guard = init_ssh_control();
    let expected_dir = get_ssh_control_dir();

    // Verify the guard holds the same directory
    assert_eq!(guard.control_dir, *expected_dir);
  }

  #[test]
  fn test_ssh_control_guard_drop() {
    // Verify that dropping the guard doesn't panic
    // We can't easily test the actual cleanup without creating real SSH
    // connections, but we can at least verify the Drop implementation runs
    let guard = init_ssh_control();
    drop(guard);
    // If this completes without panic, the Drop impl is at least safe
  }

  proptest! {
    #[test]
    #[serial]
    fn test_should_cleanup_remote_enabled_by_valid_values(
        value in prop_oneof![
            Just("1"),
            Just("true"),
            Just("yes"),
            Just("TRUE"),
            Just("YES"),
            Just("True"),
        ]
    ) {
      unsafe {
        std::env::set_var("NH_REMOTE_CLEANUP", value);
      }
      prop_assert!(should_cleanup_remote());
      unsafe {
        std::env::remove_var("NH_REMOTE_CLEANUP");
      }
    }
  }

  #[test]
  #[serial]
  fn test_should_cleanup_remote_empty_disabled() {
    // Empty value should NOT enable cleanup
    unsafe {
      std::env::set_var("NH_REMOTE_CLEANUP", "");
    }
    assert!(!should_cleanup_remote());
    unsafe {
      std::env::remove_var("NH_REMOTE_CLEANUP");
    }
  }

  #[test]
  #[serial]
  fn test_should_cleanup_remote_arbitrary_value_disabled() {
    // Arbitrary values should NOT enable cleanup
    unsafe {
      std::env::set_var("NH_REMOTE_CLEANUP", "maybe");
    }
    assert!(!should_cleanup_remote());
    unsafe {
      std::env::remove_var("NH_REMOTE_CLEANUP");
    }
  }

  #[test]
  fn test_attempt_remote_cleanup_does_nothing_when_disabled() {
    // When should_cleanup_remote returns false, no SSH command should be
    // executed. We can't easily verify no SSH was spawned, but we can verify
    // the function doesn't panic or error when cleanup is disabled
    let host = RemoteHost::parse("user@host.example").unwrap();
    let remote_cmd = "nix build /nix/store/abc.drv^* --print-out-paths";

    // This should complete without error even when cleanup is disabled
    attempt_remote_cleanup(&host, remote_cmd);
    // If we reach here, the function handled the disabled case gracefully
  }
}

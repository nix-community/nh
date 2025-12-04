use std::{env, ffi::OsString, path::PathBuf, sync::OnceLock};

use color_eyre::{
  Result,
  eyre::{Context, bail, eyre},
};
use subprocess::{Exec, ExitStatus, Redirection};
use tracing::{debug, info};

use crate::{installable::Installable, util::NixVariant};

/// Cache for the SSH control socket directory.
static SSH_CONTROL_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Get or create the SSH control socket directory.
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
    if let Err(e) = std::fs::create_dir_all(&control_dir) {
      debug!("Failed to create SSH control directory: {e}");
      // Fall back to /tmp if we can't create the directory
      return PathBuf::from("/tmp");
    }

    control_dir
  })
}

/// A parsed remote host specification.
///
/// Handles various formats:
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
        "Invalid hostname '{}': contains '/'. Did you mean to use a bare \
         hostname?",
        hostname_part
      );
    }
    if hostname_part.contains(':') {
      bail!(
        "Invalid hostname '{}': contains ':'. Ports should be specified via \
         NIX_SSHOPTS=\"-p 2222\" or ~/.ssh/config",
        hostname_part
      );
    }

    Ok(Self {
      host: host.to_string(),
    })
  }

  /// Get the host string for use with nix-copy-closure and SSH.
  #[must_use]
  pub fn host(&self) -> &str {
    &self.host
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

/// Shell-quote a string for safe use in SSH commands.
// FIXME: this is handrolled so that I can confirm whether we match
// nixos-rebuild-ng's use of shlex.quote, but we'll want to introduce shlex as a
// dependency and drop this. This is a hard blocker.
///
fn shell_quote(s: &str) -> String {
  // shlex.quote in Python returns the string unchanged if it contains only
  // safe characters, otherwise wraps in single quotes with escaping
  if s.is_empty() {
    return "''".to_string();
  }
  if s
    .chars()
    .all(|c| c.is_ascii_alphanumeric() || "-_./+:=@^".contains(c))
  {
    return s.to_string();
  }
  // Escape single quotes and wrap in single quotes
  let escaped = s.replace('\'', "'\"'\"'");
  format!("'{escaped}'")
}

/// Get SSH options from `NIX_SSHOPTS` plus our defaults.
fn get_ssh_opts() -> Vec<String> {
  let mut opts: Vec<String> = Vec::new();

  // User options first (from NIX_SSHOPTS)
  if let Ok(sshopts) = env::var("NIX_SSHOPTS") {
    for opt in sshopts.split_whitespace() {
      opts.push(opt.to_string());
    }
  }

  // Then our defaults (including ControlPath)
  opts.extend(get_default_ssh_opts());

  opts
}

/// Get `NIX_SSHOPTS` environment value with our defaults appended.
/// Used for `nix-copy-closure` which reads `NIX_SSHOPTS`.
fn get_nix_sshopts_env() -> String {
  let sshopts = env::var("NIX_SSHOPTS").unwrap_or_default();
  let defaults = get_default_ssh_opts().join(" ");
  if sshopts.is_empty() {
    defaults
  } else {
    format!("{sshopts} {defaults}")
  }
}

/// Get the flake experimental feature flags required for `nix` commands.
///
/// Returns the flags needed for `--extra-experimental-features "nix-command
/// flakes"` based on the detected Nix variant:
/// - Determinate Nix: No flags needed (features are stable)
/// - Nix/Lix: Returns `["--extra-experimental-features", "nix-command flakes"]`
fn get_flake_flags() -> Vec<&'static str> {
  let variant = crate::util::get_nix_variant();
  match variant {
    NixVariant::Determinate => vec![],
    NixVariant::Nix | NixVariant::Lix => {
      vec!["--extra-experimental-features", "nix-command flakes"]
    },
  }
}

/// Run a command on a remote host via SSH.
fn run_remote_command(
  host: &RemoteHost,
  args: &[&str],
  capture_stdout: bool,
) -> Result<Option<String>> {
  let ssh_opts = get_ssh_opts();

  // Quote args for shell (matching nixos-rebuild-ng's shlex.quote)
  let quoted_args: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();

  debug!(
    "Running remote command on {}: {}",
    host,
    quoted_args.join(" ")
  );

  let mut cmd = Exec::cmd("ssh");
  for opt in &ssh_opts {
    cmd = cmd.arg(opt);
  }
  cmd = cmd.arg(host.host()).arg("--");
  for arg in &quoted_args {
    cmd = cmd.arg(arg);
  }

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

  let mut cmd = Exec::cmd("nix-copy-closure").arg("--to").arg(host.host());

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

/// Copy a Nix closure from a remote host to localhost.
fn copy_closure_from(
  host: &RemoteHost,
  path: &str,
  use_substitutes: bool,
) -> Result<()> {
  info!("Copying result from build host '{}'", host);

  let mut cmd = Exec::cmd("nix-copy-closure").arg("--from").arg(host.host());

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
    .arg(format!("ssh://{}", from_host.host()))
    .arg("--to")
    .arg(format!("ssh://{}", to_host.host()));

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
/// 3. Build on remote via `nix build <drv>^* --print-out-paths`
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
  // If target_host is set, copy directly from build_host to target_host.
  // Otherwise, copy back to localhost.
  if let Some(ref target_host) = config.target_host {
    copy_closure_between_remotes(
      build_host,
      target_host,
      &out_path,
      use_substitutes,
    )?;
  } else {
    copy_closure_from(build_host, &out_path, use_substitutes)?;

    // Create local out-link if requested (only when copying to localhost)
    if let Some(link) = out_link {
      debug!("Creating out-link: {} -> {}", link.display(), out_path);
      // Remove existing symlink/file if present
      let _ = std::fs::remove_file(link);
      std::os::unix::fs::symlink(&out_path, link)
        .wrap_err("Failed to create out-link")?;
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
    // With nom: pipe through nix-output-monitor
    build_on_remote_with_nom(host, &drv_with_outputs, config)
  } else {
    // Without nom: simple remote execution
    build_on_remote_simple(host, &drv_with_outputs, config)
  }
}

/// Build on remote without nom - just capture output.
fn build_on_remote_simple(
  host: &RemoteHost,
  drv_with_outputs: &str,
  config: &RemoteBuildConfig,
) -> Result<String> {
  // Get flake flags for the remote nix command
  let flake_flags = get_flake_flags();

  let mut args: Vec<&str> = vec!["nix"];
  args.extend(flake_flags);
  args.extend(["build", drv_with_outputs, "--print-out-paths"]);

  // Collect extra args that are valid strings
  let extra_args_strings: Vec<String> = config
    .extra_args
    .iter()
    .filter_map(|s| s.to_str().map(String::from))
    .collect();
  for arg in &extra_args_strings {
    args.push(arg);
  }

  let result = run_remote_command(host, &args, true)?
    .ok_or_else(|| eyre!("Remote build returned no output"))?;

  // --print-out-paths may return multiple lines; take first
  let out_path = result
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
  let ssh_opts = get_ssh_opts();
  let flake_flags = get_flake_flags();

  // Build the remote command with JSON output for nom
  let mut remote_args: Vec<&str> = vec!["nix"];
  remote_args.extend(flake_flags.iter().copied());
  remote_args.extend([
    "build",
    drv_with_outputs,
    "--log-format",
    "internal-json",
    "--verbose",
  ]);

  let extra_args_strings: Vec<String> = config
    .extra_args
    .iter()
    .filter_map(|s| s.to_str().map(String::from))
    .collect();
  for arg in &extra_args_strings {
    remote_args.push(arg);
  }

  // Quote for shell
  let quoted_remote: Vec<String> =
    remote_args.iter().map(|a| shell_quote(a)).collect();

  // Build SSH command
  let mut ssh_cmd = Exec::cmd("ssh");
  for opt in &ssh_opts {
    ssh_cmd = ssh_cmd.arg(opt);
  }
  ssh_cmd = ssh_cmd
    .arg(host.host())
    .arg("--")
    .args(&quoted_remote)
    .stdout(Redirection::Pipe)
    .stderr(Redirection::Merge);

  // Pipe through nom
  let nom_cmd = Exec::cmd("nom").arg("--json");
  let pipeline = (ssh_cmd | nom_cmd).stdout(Redirection::None);

  debug!(?pipeline, "Running remote build with nom");

  let exit = pipeline.join().wrap_err("Remote build with nom failed")?;

  match exit {
    ExitStatus::Exited(0) => {},
    other => bail!("Remote build failed with exit status: {other:?}"),
  }

  // nom consumed the output, so we need to query the output path separately
  // Run nix build again with --print-out-paths (it will be a no-op since
  // already built)
  let mut query_args: Vec<&str> = vec!["nix"];
  query_args.extend(flake_flags.iter().copied());
  query_args.extend(["build", drv_with_outputs, "--print-out-paths"]);

  let result = run_remote_command(host, &query_args, true)?
    .ok_or_else(|| eyre!("Failed to get output path after build"))?;

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
  use super::*;

  #[test]
  fn test_parse_bare_hostname() {
    let host = RemoteHost::parse("buildserver").expect("should parse");
    assert_eq!(host.host(), "buildserver");
  }

  #[test]
  fn test_parse_user_at_hostname() {
    let host = RemoteHost::parse("root@buildserver").expect("should parse");
    assert_eq!(host.host(), "root@buildserver");
  }

  #[test]
  fn test_parse_ssh_uri_stripped() {
    let host = RemoteHost::parse("ssh://buildserver").expect("should parse");
    assert_eq!(host.host(), "buildserver");
  }

  #[test]
  fn test_parse_ssh_ng_uri_stripped() {
    let host = RemoteHost::parse("ssh-ng://buildserver").expect("should parse");
    assert_eq!(host.host(), "buildserver");
  }

  #[test]
  fn test_parse_ssh_uri_with_user() {
    let host =
      RemoteHost::parse("ssh://root@buildserver").expect("should parse");
    assert_eq!(host.host(), "root@buildserver");
  }

  #[test]
  fn test_parse_ssh_ng_uri_with_user() {
    let host =
      RemoteHost::parse("ssh-ng://admin@buildserver").expect("should parse");
    assert_eq!(host.host(), "admin@buildserver");
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
  fn test_shell_quote_simple() {
    assert_eq!(shell_quote("simple"), "simple");
    assert_eq!(
      shell_quote("/nix/store/abc123-foo"),
      "/nix/store/abc123-foo"
    );
  }

  #[test]
  fn test_shell_quote_with_caret() {
    // drv^* syntax must work
    assert_eq!(
      shell_quote("/nix/store/xyz.drv^*"),
      "'/nix/store/xyz.drv^*'"
    );
  }

  #[test]
  fn test_shell_quote_special_chars() {
    assert_eq!(shell_quote("has space"), "'has space'");
    assert_eq!(shell_quote("has'quote"), "'has'\"'\"'quote'");
    assert_eq!(shell_quote("$(dangerous)"), "'$(dangerous)'");
  }

  #[test]
  fn test_shell_quote_empty() {
    assert_eq!(shell_quote(""), "''");
  }

  #[test]
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
}

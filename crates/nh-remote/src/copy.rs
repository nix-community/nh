use std::{
  ffi::OsString,
  io::{BufRead, Read},
  path::Path,
};

use color_eyre::{
  Result,
  eyre::{Context, eyre},
};
use nh_core::{
  command::{CommandKind, NixCommand, exec_with_streaming},
  progress::{self, Spinner},
};
use subprocess::{Exec, Redirection};
use tracing::{debug, error, info};

use super::{RemoteHost, get_flake_flags, get_nix_sshopts_env};

#[derive(Debug, Clone, Copy)]
enum CopyDirection<'a> {
  FromRemote(&'a RemoteHost),
  ToRemote {
    host:            &'a RemoteHost,
    use_substitutes: bool,
  },
  BetweenRemotes {
    from_host:       &'a RemoteHost,
    to_host:         &'a RemoteHost,
    use_substitutes: bool,
  },
}

impl CopyDirection<'_> {
  fn args(self) -> Vec<String> {
    match self {
      Self::FromRemote(host) => {
        vec![
          "--no-check-sigs".to_string(),
          "--from".to_string(),
          store_uri(host),
        ]
      },
      Self::ToRemote {
        host,
        use_substitutes,
      } => {
        let mut args = vec![
          "--no-check-sigs".to_string(),
          "--to".to_string(),
          store_uri(host),
        ];
        push_substitute_on_destination(&mut args, use_substitutes);
        args
      },
      Self::BetweenRemotes {
        from_host,
        to_host,
        use_substitutes,
      } => {
        let mut args = vec![
          "--no-check-sigs".to_string(),
          "--from".to_string(),
          store_uri(from_host),
          "--to".to_string(),
          store_uri(to_host),
        ];
        push_substitute_on_destination(&mut args, use_substitutes);
        args
      },
    }
  }
}

fn push_substitute_on_destination(
  args: &mut Vec<String>,
  use_substitutes: bool,
) {
  if use_substitutes {
    args.push("--substitute-on-destination".to_string());
  }
}

fn store_uri(host: &RemoteHost) -> String {
  host.nix_store_uri()
}

fn build_nix_copy_command<P: Into<OsString>>(
  direction: CopyDirection<'_>,
  path: P,
) -> Exec {
  NixCommand::new(CommandKind::Copy)
    .global_args(get_flake_flags())
    .args(direction.args())
    .arg(path.into())
    .env("NIX_SSHOPTS", get_nix_sshopts_env())
    .to_exec()
}

/// Copy a Nix closure from a remote host to localhost.
pub fn copy_closure_from(host: &RemoteHost, path: &str) -> Result<()> {
  info!("Copying result from build host '{host}'");

  let cmd = build_nix_copy_command(CopyDirection::FromRemote(host), path);
  debug!(?cmd, "nix copy --from");

  let (exit_status, _stdout, stderr) = exec_with_streaming(cmd, true)
    .wrap_err("Failed to copy closure from remote host")?;

  if !exit_status.success() {
    color_eyre::eyre::bail!(format_copy_failure(
      &format!("nix copy --from '{host}' failed"),
      exit_status,
      &stderr,
    ));
  }

  Ok(())
}

fn spawn_spinner_stream_thread<R>(
  pipe: R,
  spinner: Spinner,
  stream_name: &'static str,
) -> std::thread::JoinHandle<Result<String>>
where
  R: Read + Send + 'static,
{
  std::thread::spawn(move || {
    let mut reader = std::io::BufReader::new(pipe);
    let mut line = Vec::new();
    let mut output = String::new();

    loop {
      line.clear();
      let bytes_read = reader
        .read_until(b'\n', &mut line)
        .wrap_err_with(|| format!("Failed to read {stream_name}"))?;

      if bytes_read == 0 {
        break;
      }

      let message = String::from_utf8_lossy(&line)
        .trim_end_matches(['\r', '\n'])
        .to_string();
      spinner.println(message);
      output.push_str(&String::from_utf8_lossy(&line));
    }

    Ok(output)
  })
}

fn format_copy_failure(
  message: &str,
  exit_status: subprocess::ExitStatus,
  stderr: &str,
) -> String {
  let stderr = stderr.trim();

  if stderr.is_empty() {
    format!("{message} (exit status: {exit_status:?})")
  } else {
    format!("{message} (exit status: {exit_status:?})\nstderr:\n{stderr}")
  }
}

fn exec_with_spinner_streaming(
  cmd: Exec,
  spinner: &Spinner,
) -> Result<(subprocess::ExitStatus, String, String)> {
  let mut job = cmd
    .stdout(Redirection::Pipe)
    .stderr(Redirection::Pipe)
    .start()
    .wrap_err("Failed to start command")?;

  let stdout_pipe = job
    .stdout
    .take()
    .ok_or_else(|| eyre!("Failed to capture stdout"))?;
  let stderr_pipe = job
    .stderr
    .take()
    .ok_or_else(|| eyre!("Failed to capture stderr"))?;

  let stdout_thread =
    spawn_spinner_stream_thread(stdout_pipe, spinner.clone(), "stdout");
  let stderr_thread =
    spawn_spinner_stream_thread(stderr_pipe, spinner.clone(), "stderr");

  let exit_status = job
    .wait()
    .wrap_err("Failed to wait for command completion")?;

  let stdout = stdout_thread
    .join()
    .map_err(|_| eyre!("Stdout thread panicked"))??;
  let stderr = stderr_thread
    .join()
    .map_err(|_| eyre!("Stderr thread panicked"))??;

  Ok((exit_status, stdout, stderr))
}

/// Copy a Nix closure from localhost to a remote host.
///
/// Uses `nix copy --to <host-store-uri>` to transfer a store path and its
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
  let cmd = build_nix_copy_command(
    CopyDirection::ToRemote {
      host,
      use_substitutes,
    },
    path,
  );
  debug!(?cmd, "nix copy --to");

  let spinner =
    progress::spinner(format!("Copying closure to remote host '{host}'..."));

  let copy_result = exec_with_spinner_streaming(cmd, &spinner);

  // We finish and *clear*, because the log line needs to come next. If we try
  // to make the spinner change the text, we cannot reliably match the `info!`
  // or `error!` style.
  spinner.finish_and_clear();
  let (exit_status, _stdout, stderr) =
    copy_result.wrap_err("Failed to copy closure to remote host")?;

  if !exit_status.success() {
    error!("Failed to copy closure to remote host '{host}'");
    color_eyre::eyre::bail!(format_copy_failure(
      &format!("nix copy --to '{host}' failed"),
      exit_status,
      &stderr,
    ));
  }
  info!("Copied closure to remote host '{host}'");

  Ok(())
}

/// Copy a Nix closure from one remote host to another.
/// Uses `nix copy --from <source-store-uri> --to <dest-store-uri>`.
pub fn copy_closure_between_remotes(
  from_host: &RemoteHost,
  to_host: &RemoteHost,
  path: &str,
  use_substitutes: bool,
) -> Result<()> {
  info!("Copying closure from '{}' to '{}'", from_host, to_host);

  let cmd = build_nix_copy_command(
    CopyDirection::BetweenRemotes {
      from_host,
      to_host,
      use_substitutes,
    },
    path,
  );
  debug!(?cmd, "nix copy between remotes");

  let (exit_status, _stdout, stderr) = exec_with_streaming(cmd, true)
    .wrap_err("Failed to copy closure between remote hosts")?;

  if !exit_status.success() {
    color_eyre::eyre::bail!(format_copy_failure(
      &format!("nix copy from '{from_host}' to '{to_host}' failed"),
      exit_status,
      &stderr,
    ));
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  #![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "Fine in tests"
  )]
  use std::io::Read;

  use super::*;

  #[test]
  fn test_copy_direction_to_remote_args() {
    let host = RemoteHost::parse("build.example").unwrap();

    assert_eq!(
      CopyDirection::ToRemote {
        host:            &host,
        use_substitutes: true,
      }
      .args(),
      vec![
        "--no-check-sigs",
        "--to",
        "ssh-ng://build.example",
        "--substitute-on-destination",
      ]
    );
  }

  #[test]
  fn test_copy_direction_preserves_ssh_store_scheme() {
    let host = RemoteHost::parse("ssh://build.example").unwrap();

    assert_eq!(
      CopyDirection::ToRemote {
        host:            &host,
        use_substitutes: true,
      }
      .args(),
      vec![
        "--no-check-sigs",
        "--to",
        "ssh://build.example",
        "--substitute-on-destination",
      ]
    );
  }

  #[test]
  fn test_copy_direction_from_remote_cannot_take_substitute_policy() {
    let host = RemoteHost::parse("build.example").unwrap();

    assert_eq!(CopyDirection::FromRemote(&host).args(), vec![
      "--no-check-sigs",
      "--from",
      "ssh-ng://build.example"
    ]);
  }

  #[test]
  fn test_copy_direction_between_remotes_args() {
    let from_host = RemoteHost::parse("build.example").unwrap();
    let to_host = RemoteHost::parse("target.example").unwrap();

    assert_eq!(
      CopyDirection::BetweenRemotes {
        from_host:       &from_host,
        to_host:         &to_host,
        use_substitutes: true,
      }
      .args(),
      vec![
        "--no-check-sigs",
        "--from",
        "ssh-ng://build.example",
        "--to",
        "ssh-ng://target.example",
        "--substitute-on-destination",
      ]
    );
  }

  #[test]
  fn test_copy_direction_preserves_ipv6_store_uri_brackets() {
    let host = RemoteHost::parse("user@[2001:db8::1]").unwrap();

    assert_eq!(
      CopyDirection::ToRemote {
        host:            &host,
        use_substitutes: false,
      }
      .args(),
      vec!["--no-check-sigs", "--to", "ssh-ng://user@[2001:db8::1]"]
    );
  }

  /// A reader that always returns an I/O error, used to test error
  /// propagation through `spawn_spinner_stream_thread`.
  struct FaultyReader;

  impl Read for FaultyReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
      Err(std::io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        "simulated pipe failure",
      ))
    }
  }

  #[test]
  fn test_exec_with_spinner_streaming_mixed_output_no_deadlock() {
    let spinner = Spinner::hidden();
    // Interleaved stdout and stderr: alternating lines with explicit flush.
    let cmd = Exec::cmd("bash").arg("-c").arg(
      r#"
for i in $(seq 1 10); do
  echo "stdout $i"
  echo "stderr $i" >&2
done
"#,
    );
    let result = exec_with_spinner_streaming(cmd, &spinner);
    assert!(
      result.is_ok(),
      "exec_with_spinner_streaming must not deadlock on mixed stdout/stderr"
    );
    let (_status, stdout, stderr) = result.unwrap();
    assert!(stdout.contains("stdout 10"));
    assert!(stderr.contains("stderr 10"));
  }

  #[test]
  fn test_spawn_spinner_stream_thread_error_propagation() {
    let spinner = Spinner::hidden();
    let handle =
      spawn_spinner_stream_thread(FaultyReader, spinner, "faulty-stream");
    let result = handle
      .join()
      .expect("spawn_spinner_stream_thread should not panic");
    assert!(
      result.is_err(),
      "spawn_spinner_stream_thread must propagate read errors"
    );
  }

  #[test]
  fn test_exec_with_spinner_streaming_command_start_error_propagation() {
    let spinner = Spinner::hidden();
    // A nonexistent command triggers `cmd.start()` failure.
    // This should verify that errors propagate out of
    // `exec_with_spinner_streaming` rather than panicking.
    let cmd = Exec::cmd("nonexistent_command_xyz_123");
    let result = exec_with_spinner_streaming(cmd, &spinner);
    assert!(
      result.is_err(),
      "exec_with_spinner_streaming must propagate command start errors"
    );
  }
}

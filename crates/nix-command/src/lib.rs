use std::{
  ffi::{OsStr, OsString},
  io::{self, Read, Write},
  process::{Command, ExitStatus, Output, Stdio},
  sync::mpsc,
  thread,
  time::{Duration, Instant},
};

use subprocess::Exec;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
  #[error("io: {0}")]
  Io(#[from] io::Error),
  #[error("command '{command}' failed")]
  CommandFailed { command: String },
  #[error("nix {command} timed out after {} seconds", duration.as_secs())]
  Timeout {
    command:  String,
    duration: Duration,
  },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandKind {
  Build,
  Config,
  Copy,
  Develop,
  Eval,
  Flake,
  PathInfo,
  Repl,
  Run,
  Shell,
  Store,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandSpec {
  pub name:             &'static str,
  pub print_build_logs: bool,
  pub interactive:      bool,
}

pub const COMMAND_SPECS: &[CommandSpec] = &[
  CommandSpec {
    name:             "build",
    print_build_logs: true,
    interactive:      false,
  },
  CommandSpec {
    name:             "config",
    print_build_logs: false,
    interactive:      false,
  },
  CommandSpec {
    name:             "copy",
    print_build_logs: false,
    interactive:      false,
  },
  CommandSpec {
    name:             "develop",
    print_build_logs: true,
    interactive:      true,
  },
  CommandSpec {
    name:             "eval",
    print_build_logs: false,
    interactive:      false,
  },
  CommandSpec {
    name:             "flake",
    print_build_logs: false,
    interactive:      false,
  },
  CommandSpec {
    name:             "path-info",
    print_build_logs: false,
    interactive:      false,
  },
  CommandSpec {
    name:             "repl",
    print_build_logs: false,
    interactive:      true,
  },
  CommandSpec {
    name:             "run",
    print_build_logs: true,
    interactive:      true,
  },
  CommandSpec {
    name:             "shell",
    print_build_logs: true,
    interactive:      true,
  },
  CommandSpec {
    name:             "store",
    print_build_logs: false,
    interactive:      false,
  },
];

impl CommandKind {
  #[must_use]
  pub const fn as_str(self) -> &'static str {
    self.spec().name
  }

  #[must_use]
  pub const fn spec(self) -> CommandSpec {
    match self {
      Self::Build => COMMAND_SPECS[0],
      Self::Config => COMMAND_SPECS[1],
      Self::Copy => COMMAND_SPECS[2],
      Self::Develop => COMMAND_SPECS[3],
      Self::Eval => COMMAND_SPECS[4],
      Self::Flake => COMMAND_SPECS[5],
      Self::PathInfo => COMMAND_SPECS[6],
      Self::Repl => COMMAND_SPECS[7],
      Self::Run => COMMAND_SPECS[8],
      Self::Shell => COMMAND_SPECS[9],
      Self::Store => COMMAND_SPECS[10],
    }
  }
}

impl TryFrom<&str> for CommandKind {
  type Error = UnknownCommand;

  fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
    match value {
      "build" => Ok(Self::Build),
      "config" => Ok(Self::Config),
      "copy" => Ok(Self::Copy),
      "develop" => Ok(Self::Develop),
      "eval" => Ok(Self::Eval),
      "flake" => Ok(Self::Flake),
      "path-info" => Ok(Self::PathInfo),
      "repl" => Ok(Self::Repl),
      "run" => Ok(Self::Run),
      "shell" => Ok(Self::Shell),
      "store" => Ok(Self::Store),
      command => {
        Err(UnknownCommand {
          command: command.to_string(),
        })
      },
    }
  }
}

#[derive(Debug, Error, Eq, PartialEq)]
#[error("unknown nix command '{command}'")]
pub struct UnknownCommand {
  command: String,
}

#[derive(Debug)]
enum PipeEvent {
  Stdout(Vec<u8>),
  Stderr(Vec<u8>),
  Error(io::Error),
}

fn read_pipe<R: Read>(
  mut reader: R,
  tx: &mpsc::Sender<PipeEvent>,
  is_stderr: bool,
) {
  let mut buf = [0u8; 4096];
  loop {
    match reader.read(&mut buf) {
      Ok(0) => break,
      Ok(n) => {
        let event = if is_stderr {
          PipeEvent::Stderr(buf[..n].to_vec())
        } else {
          PipeEvent::Stdout(buf[..n].to_vec())
        };
        if tx.send(event).is_err() {
          break;
        }
      },
      Err(e) => {
        let _ = tx.send(PipeEvent::Error(e));
        break;
      },
    }
  }
}

pub struct NixCommand {
  kind:                    Option<CommandKind>,
  binary:                  OsString,
  global_args:             Vec<OsString>,
  args:                    Vec<OsString>,
  env:                     Vec<(OsString, OsString)>,
  impure:                  bool,
  print_build_logs:        bool,
  interactive:             bool,
  timeout:                 Option<Duration>,
  eval_profiler_mode:      Option<String>,
  eval_profiler_frequency: Option<u32>,
  eval_profile_file:       Option<String>,
}

impl NixCommand {
  #[must_use]
  pub fn new(kind: CommandKind) -> Self {
    let spec = kind.spec();
    Self {
      kind:                    Some(kind),
      binary:                  OsString::from("nix"),
      global_args:             Vec::new(),
      args:                    Vec::new(),
      env:                     Vec::new(),
      impure:                  false,
      print_build_logs:        spec.print_build_logs,
      interactive:             spec.interactive,
      timeout:                 None,
      eval_profiler_mode:      None,
      eval_profiler_frequency: None,
      eval_profile_file:       None,
    }
  }

  #[must_use]
  pub fn raw() -> Self {
    Self {
      kind:                    None,
      binary:                  OsString::from("nix"),
      global_args:             Vec::new(),
      args:                    Vec::new(),
      env:                     Vec::new(),
      impure:                  false,
      print_build_logs:        false,
      interactive:             false,
      timeout:                 None,
      eval_profiler_mode:      None,
      eval_profiler_frequency: None,
      eval_profile_file:       None,
    }
  }

  #[must_use]
  pub fn arg<S: AsRef<OsStr>>(mut self, arg: S) -> Self {
    self.args.push(arg.as_ref().to_os_string());
    self
  }

  #[must_use]
  pub fn global_arg<S: AsRef<OsStr>>(mut self, arg: S) -> Self {
    self.global_args.push(arg.as_ref().to_os_string());
    self
  }

  #[must_use]
  pub fn global_args<I>(mut self, args: I) -> Self
  where
    I: IntoIterator,
    I::Item: AsRef<OsStr>,
  {
    self
      .global_args
      .extend(args.into_iter().map(|arg| arg.as_ref().to_os_string()));
    self
  }

  #[must_use]
  pub fn args_ref(mut self, args: &[String]) -> Self {
    self.args.extend(args.iter().map(OsString::from));
    self
  }

  #[must_use]
  pub fn args<I>(mut self, args: I) -> Self
  where
    I: IntoIterator,
    I::Item: AsRef<OsStr>,
  {
    self
      .args
      .extend(args.into_iter().map(|arg| arg.as_ref().to_os_string()));
    self
  }

  #[must_use]
  pub fn env<K: AsRef<OsStr>, V: AsRef<OsStr>>(
    mut self,
    key: K,
    value: V,
  ) -> Self {
    self
      .env
      .push((key.as_ref().to_os_string(), value.as_ref().to_os_string()));
    self
  }

  #[must_use]
  pub fn envs<I, K, V>(mut self, envs: I) -> Self
  where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
  {
    for (key, value) in envs {
      self = self.env(key, value);
    }
    self
  }

  #[must_use]
  pub const fn impure(mut self, yes: bool) -> Self {
    self.impure = yes;
    self
  }

  #[must_use]
  pub fn binary<S: AsRef<OsStr>>(mut self, path: S) -> Self {
    self.binary = path.as_ref().to_os_string();
    self
  }

  #[must_use]
  pub const fn interactive(mut self, yes: bool) -> Self {
    self.interactive = yes;
    self
  }

  #[must_use]
  pub const fn print_build_logs(mut self, yes: bool) -> Self {
    self.print_build_logs = yes;
    self
  }

  #[must_use]
  pub const fn with_timeout(mut self, timeout: Duration) -> Self {
    self.timeout = Some(timeout);
    self
  }

  #[must_use]
  pub fn eval_profiler<S: Into<String>>(mut self, mode: S) -> Self {
    self.eval_profiler_mode = Some(mode.into());
    self
  }

  #[must_use]
  pub const fn eval_profiler_frequency(mut self, hz: u32) -> Self {
    self.eval_profiler_frequency = Some(hz);
    self
  }

  #[must_use]
  pub fn eval_profile_file<S: Into<String>>(mut self, path: S) -> Self {
    self.eval_profile_file = Some(path.into());
    self
  }

  #[must_use]
  pub fn with_required_env(mut self) -> Self {
    const PRESERVE_ENV: &[&str] = &[
      "LOCALE_ARCHIVE",
      "PATH",
      "NIX_SSHOPTS",
      "HOME_MANAGER_BACKUP_EXT",
      "NIX_CONFIG",
      "NIX_PATH",
      "NIX_REMOTE",
      "NIX_SSL_CERT_FILE",
      "NIX_USER_CONF_FILES",
    ];

    if let Ok(user) = std::env::var("USER") {
      self = self.env("USER", user);
    }
    if let Ok(home) = std::env::var("HOME") {
      self = self.env("HOME", home);
    }

    for key in PRESERVE_ENV {
      if let Ok(value) = std::env::var(key) {
        self = self.env(key, value);
      }
    }

    for (key, value) in std::env::vars() {
      if key.starts_with("NH_") {
        self = self.env(key, value);
      }
    }

    self
  }

  #[must_use]
  pub fn argv(&self) -> Vec<OsString> {
    let mut argv = vec![self.binary.clone()];
    argv.extend(self.global_args.iter().cloned());
    if let Some(kind) = self.kind {
      argv.push(OsString::from(kind.as_str()));
    }
    if self.print_build_logs
      && !self
        .args
        .iter()
        .any(|a| a == OsStr::new("--no-build-output"))
    {
      argv.push(OsString::from("--print-build-logs"));
    }
    if self.impure {
      argv.push(OsString::from("--impure"));
    }
    if let Some(ref mode) = self.eval_profiler_mode {
      argv.push(OsString::from("--eval-profiler"));
      argv.push(OsString::from(mode));
    }
    if let Some(hz) = self.eval_profiler_frequency {
      argv.push(OsString::from("--eval-profiler-frequency"));
      argv.push(OsString::from(hz.to_string()));
    }
    if let Some(ref path) = self.eval_profile_file {
      argv.push(OsString::from("--eval-profile-file"));
      argv.push(OsString::from(path));
    }
    argv.extend(self.args.iter().cloned());
    argv
  }

  #[must_use]
  pub fn to_std_command(&self) -> Command {
    let argv = self.argv();
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    for (k, v) in &self.env {
      cmd.env(k, v);
    }
    cmd
  }

  pub fn to_exec(&self) -> Exec {
    let argv = self.argv();
    let mut cmd = Exec::cmd(&argv[0]).args(&argv[1..]);
    for (key, value) in &self.env {
      cmd = cmd.env(key, value);
    }
    cmd
  }

  #[must_use]
  pub fn into_parts(
    self,
  ) -> (OsString, Vec<OsString>, Vec<(OsString, OsString)>) {
    let mut argv = self.argv();
    let binary = argv.remove(0);
    (binary, argv, self.env)
  }

  #[must_use]
  pub fn nix_store() -> Self {
    Self::raw().binary("nix-store")
  }

  #[must_use]
  pub fn nix_instantiate() -> Self {
    Self::raw().binary("nix-instantiate")
  }

  /// Run the command, streaming stdout and stderr.
  ///
  /// Interactive commands inherit stdio directly, while non-interactive
  /// commands stream stdout and stderr while the process runs.
  ///
  /// # Errors
  ///
  /// Returns an error if the command cannot be started, stdout or stderr
  /// cannot be captured, a pipe read fails, waiting for the process fails, or
  /// the configured timeout expires.
  pub fn run_with_logs(&self) -> Result<ExitStatus> {
    let mut cmd = self.to_std_command();

    if self.interactive {
      return Ok(
        cmd
          .stdout(Stdio::inherit())
          .stderr(Stdio::inherit())
          .stdin(Stdio::inherit())
          .status()?,
      );
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| self.command_failed())?;
    let stderr = child.stderr.take().ok_or_else(|| self.command_failed())?;
    let (tx, rx) = mpsc::channel();
    let stdout_thread = thread::spawn({
      let tx = tx.clone();
      move || read_pipe(stdout, &tx, false)
    });
    let stderr_thread = thread::spawn(move || read_pipe(stderr, &tx, true));
    let start = Instant::now();

    loop {
      if let Some(timeout) = self.timeout
        && start.elapsed() > timeout
      {
        kill_wait_join(&mut child, stdout_thread, stderr_thread)?;
        return Err(self.timeout_error(timeout));
      }

      match rx.recv_timeout(Duration::from_millis(100)) {
        Ok(PipeEvent::Stdout(data)) => {
          let _ = io::stdout().write_all(&data);
        },
        Ok(PipeEvent::Stderr(data)) => {
          let _ = io::stderr().write_all(&data);
        },
        Ok(PipeEvent::Error(e)) => {
          kill_wait_join(&mut child, stdout_thread, stderr_thread)?;
          return Err(Error::Io(e));
        },
        Err(mpsc::RecvTimeoutError::Timeout) => {},
        Err(mpsc::RecvTimeoutError::Disconnected) => break,
      }
    }

    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    Ok(child.wait()?)
  }

  /// Run the command and collect its output.
  ///
  /// Interactive commands inherit stdio directly.
  ///
  /// # Errors
  ///
  /// Returns an error if the command cannot be started or its output cannot be
  /// collected.
  pub fn output(&self) -> Result<Output> {
    let mut cmd = self.to_std_command();
    if self.interactive {
      return Ok(
        cmd
          .stdout(Stdio::inherit())
          .stderr(Stdio::inherit())
          .stdin(Stdio::inherit())
          .output()?,
      );
    }
    Ok(cmd.output()?)
  }

  fn command_failed(&self) -> Error {
    Error::CommandFailed {
      command: self.command_name(),
    }
  }

  fn timeout_error(&self, duration: Duration) -> Error {
    Error::Timeout {
      command: self.command_name(),
      duration,
    }
  }

  fn command_name(&self) -> String {
    self.kind.map_or_else(
      || self.binary.to_string_lossy().into_owned(),
      |kind| kind.as_str().to_string(),
    )
  }
}

fn kill_wait_join(
  child: &mut std::process::Child,
  stdout_thread: thread::JoinHandle<()>,
  stderr_thread: thread::JoinHandle<()>,
) -> Result<()> {
  let _ = child.kill();
  let _ = stdout_thread.join();
  let _ = stderr_thread.join();
  child.wait()?;
  Ok(())
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "Fine in tests")]
mod tests {
  use super::*;

  #[test]
  fn schema_parses_supported_commands() {
    for spec in COMMAND_SPECS {
      let kind = CommandKind::try_from(spec.name).unwrap();
      assert_eq!(kind.as_str(), spec.name);
    }
  }

  #[test]
  fn schema_rejects_unknown_commands() {
    assert_eq!(
      CommandKind::try_from("doctor"),
      Err(UnknownCommand {
        command: "doctor".to_string(),
      })
    );
  }

  #[test]
  fn argv_is_deterministic_and_schema_driven() {
    let argv = NixCommand::new(CommandKind::Build)
      .arg("nixpkgs#hello")
      .impure(true)
      .argv();
    assert_eq!(argv, [
      "nix",
      "build",
      "--print-build-logs",
      "--impure",
      "nixpkgs#hello"
    ]);
  }

  #[test]
  fn no_build_output_suppresses_print_build_logs() {
    let argv = NixCommand::new(CommandKind::Build)
      .arg("--no-build-output")
      .argv();
    assert_eq!(argv, ["nix", "build", "--no-build-output"]);
  }

  #[test]
  fn eval_defaults_to_quiet_schema() {
    assert_eq!(NixCommand::new(CommandKind::Eval).argv(), ["nix", "eval"]);
  }

  #[test]
  fn interactive_defaults_come_from_schema() {
    assert!(NixCommand::new(CommandKind::Run).interactive);
    assert!(NixCommand::new(CommandKind::Shell).interactive);
    assert!(NixCommand::new(CommandKind::Develop).interactive);
    assert!(!NixCommand::new(CommandKind::Build).interactive);
  }

  #[test]
  fn commands_default_to_no_timeout() {
    assert_eq!(NixCommand::new(CommandKind::Build).timeout, None);
    assert_eq!(NixCommand::raw().timeout, None);
  }

  #[test]
  fn with_timeout_sets_command_timeout() {
    assert_eq!(
      NixCommand::new(CommandKind::Build)
        .with_timeout(Duration::from_secs(30))
        .timeout,
      Some(Duration::from_secs(30))
    );
  }

  #[test]
  fn eval_profiler_flags_are_added_to_argv() {
    let argv = NixCommand::new(CommandKind::Eval)
      .arg("nixpkgs#hello")
      .impure(true)
      .eval_profiler("flamegraph")
      .eval_profiler_frequency(9999)
      .eval_profile_file("/tmp/nix.profile")
      .argv();
    assert_eq!(argv, [
      "nix",
      "eval",
      "--impure",
      "--eval-profiler",
      "flamegraph",
      "--eval-profiler-frequency",
      "9999",
      "--eval-profile-file",
      "/tmp/nix.profile",
      "nixpkgs#hello"
    ]);
  }

  #[test]
  fn global_args_are_inserted_before_subcommand() {
    let argv = NixCommand::new(CommandKind::Eval)
      .global_args(["--extra-experimental-features", "nix-command flakes"])
      .arg("--raw")
      .arg("nixpkgs#hello")
      .argv();
    assert_eq!(argv, [
      "nix",
      "--extra-experimental-features",
      "nix-command flakes",
      "eval",
      "--raw",
      "nixpkgs#hello"
    ]);
  }

  #[test]
  fn raw_command_omits_subcommand() {
    let argv = NixCommand::raw().arg("--version").argv();
    assert_eq!(argv, ["nix", "--version"]);
  }

  #[test]
  fn alternate_binary_omits_nix_subcommand() {
    let argv = NixCommand::nix_store().arg("--optimise").argv();
    assert_eq!(argv, ["nix-store", "--optimise"]);
  }
}

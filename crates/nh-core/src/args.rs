use std::path::PathBuf;

use nh_installable::InstallableArgs;
use pound::{Choice, FromArg, Parse, ValueEnum, ValueError};
use tracing::warn;

#[derive(Debug, Parse)]
pub struct CommonRebuildArgs {
  /// Only print actions, without performing them
  #[pound(long, short = 'n')]
  pub dry: bool,

  /// Ask for confirmation
  #[pound(long, short, env = "NH_ASK")]
  pub ask: bool,

  #[pound(flatten)]
  pub installable: InstallableArgs,

  /// Don't use rom for the build process
  #[pound(long)]
  pub no_rom: bool,

  /// Deprecated alias for `--no-rom`
  #[pound(long, hidden)]
  pub no_nom: bool,

  /// Path to save the result link, defaults to using a temporary directory
  #[pound(long, short)]
  pub out_link: Option<PathBuf>,

  /// Whether to display a package diff
  #[pound(long, short, default = "auto")]
  pub diff: Diff,

  #[pound(flatten)]
  pub passthrough: NixBuildPassthroughArgs,
}

/// When to display a package diff, and what to show in it.
#[derive(Debug, Clone, Copy)]
pub struct Diff {
  pub mode: DiffType,
  pub full: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffType {
  Auto,
  Always,
  Never,
}

#[derive(ValueEnum)]
enum DiffWord {
  /// Display package diff, and only warn if it fails
  Auto,
  /// Display package diff, and abort if it fails
  Always,
  /// Never display package diff
  Never,
  /// Also show packages whose version did not change, e.g. always,full
  Full,
}

impl FromArg for Diff {
  const POSSIBLE: Option<&'static [Choice]> = DiffWord::POSSIBLE;

  fn from_arg(raw: &str) -> Result<Self, ValueError> {
    let mut mode = None;
    let mut full = false;

    for word in raw.split(',') {
      let parsed = match DiffWord::from_arg(word)? {
        DiffWord::Auto => DiffType::Auto,
        DiffWord::Always => DiffType::Always,
        DiffWord::Never => DiffType::Never,
        DiffWord::Full => {
          full = true;
          continue;
        },
      };

      if mode.replace(parsed).is_some() {
        return Err(ValueError::new(
          raw,
          "expected at most one of auto, always or never",
        ));
      }
    }

    let mode = mode.unwrap_or(DiffType::Auto);
    if full && mode == DiffType::Never {
      return Err(ValueError::new(raw, "full has no effect with never"));
    }

    Ok(Self { mode, full })
  }
}

#[derive(Debug, Default, Parse)]
pub struct NixBuildPassthroughArgs {
  /// Number of concurrent jobs Nix should run
  #[pound(long, short = 'j')]
  pub max_jobs: Option<usize>,

  /// Number of cores Nix should utilize
  #[pound(long)]
  pub cores: Option<usize>,

  /// Logging format used by Nix
  #[pound(long)]
  pub log_format: Option<String>,

  /// Continue building despite encountering errors
  #[pound(long, short = 'k')]
  pub keep_going: bool,

  /// Keep build outputs from failed builds
  #[pound(long, short = 'K')]
  pub keep_failed: bool,

  /// Attempt to build locally if substituters fail
  #[pound(long)]
  pub fallback: bool,

  /// Repair corrupted store paths
  #[pound(long)]
  pub repair: bool,

  /// Explicitly define remote builders
  #[pound(long)]
  pub builders: Option<String>,

  /// Paths to include
  #[pound(long, short = 'I')]
  pub include: Vec<String>,

  /// Print build logs directly to stdout
  #[pound(long, short = 'L')]
  pub print_build_logs: bool,

  /// Display tracebacks on errors
  #[pound(long, short = 't')]
  pub show_trace: bool,

  /// Accept configuration from flakes
  #[pound(long)]
  pub accept_flake_config: bool,

  /// Refresh flakes to the latest revision
  #[pound(long)]
  pub refresh: bool,

  /// Allow impure builds
  #[pound(long)]
  pub impure: bool,

  /// Build without internet access
  #[pound(long)]
  pub offline: bool,

  /// Prohibit network usage
  #[pound(long)]
  pub no_net: bool,

  /// Recreate the flake.lock file entirely
  #[pound(long)]
  pub recreate_lock_file: bool,

  /// Do not update the flake.lock file
  #[pound(long)]
  pub no_update_lock_file: bool,

  /// Do not write a lock file
  #[pound(long)]
  pub no_write_lock_file: bool,

  /// Do not use registries
  #[pound(long = "no-use-registries")]
  pub no_use_registries: bool,

  /// Do not use registries (deprecated, use --no-use-registries)
  #[pound(long, alias = "no-registries")]
  pub no_registries: bool,

  /// Commit the lock file after updates
  #[pound(long)]
  pub commit_lock_file: bool,

  /// Suppress build output
  #[pound(long, short = 'Q')]
  pub no_build_output: bool,

  /// Use substitutes when copying
  #[pound(long)]
  pub use_substitutes: bool,

  /// Output results in JSON format
  #[pound(long)]
  pub json: bool,

  /// Set a Nix configuration option (may be given multiple times)
  #[pound(long, num_args = 2, value_name = "NAME VALUE")]
  pub option: Vec<String>,

  /// Override a specific flake input (may be given multiple times)
  #[pound(long, num_args = 2, value_name = "INPUT FLAKE_URL")]
  pub override_input: Vec<String>,
}

impl NixBuildPassthroughArgs {
  #[must_use]
  pub const fn network_restricted(&self) -> bool {
    self.offline || self.no_net
  }

  /// Generate arguments that affect evaluation without changing its output.
  #[must_use]
  pub fn generate_evaluation_args(&self) -> Vec<String> {
    let mut args = Vec::new();

    for inc in &self.include {
      args.push("--include".into());
      args.push(inc.clone());
    }
    if self.show_trace {
      args.push("--show-trace".into());
    }
    if self.accept_flake_config {
      args.push("--accept-flake-config".into());
    }
    if self.refresh {
      args.push("--refresh".into());
    }
    if self.impure {
      args.push("--impure".into());
    }
    if self.offline {
      args.push("--offline".into());
    }
    if self.no_net {
      args.push("--no-net".into());
    }
    if self.recreate_lock_file {
      args.push("--recreate-lock-file".into());
    }
    if self.no_update_lock_file {
      args.push("--no-update-lock-file".into());
    }
    if self.no_write_lock_file {
      args.push("--no-write-lock-file".into());
    }
    if self.no_use_registries || self.no_registries {
      args.push("--no-use-registries".into());
    }
    for pair in self.option.as_chunks::<2>().0 {
      args.push("--option".into());
      args.extend(pair.iter().cloned());
    }
    for pair in self.override_input.as_chunks::<2>().0 {
      args.push("--override-input".into());
      args.extend(pair.iter().cloned());
    }

    args
  }

  /// Generate arguments supported by `nix flake update`.
  #[must_use]
  pub fn generate_update_args(&self) -> Vec<String> {
    let mut args = self.generate_evaluation_args().into_iter();
    let mut update_args = Vec::new();

    while let Some(arg) = args.next() {
      match arg.as_str() {
        "--no-update-lock-file" | "--no-write-lock-file" => {},
        "--override-input" => {
          args.next();
          args.next();
        },
        _ => update_args.push(arg),
      }
    }

    update_args
  }

  /// Generate arguments supported by legacy Nix evaluation commands.
  #[must_use]
  pub fn generate_legacy_evaluation_args(&self) -> Vec<String> {
    let mut args = Vec::new();

    for inc in &self.include {
      args.push("--include".into());
      args.push(inc.clone());
    }
    if self.show_trace {
      args.push("--show-trace".into());
    }
    if self.impure {
      args.push("--impure".into());
    }
    for pair in self.option.as_chunks::<2>().0 {
      args.push("--option".into());
      args.extend(pair.iter().cloned());
    }

    args
  }

  fn append_execution_args(&self, args: &mut Vec<String>) {
    if let Some(jobs) = self.max_jobs {
      args.push("--max-jobs".into());
      args.push(jobs.to_string());
    }
    if let Some(cores) = self.cores {
      args.push("--cores".into());
      args.push(cores.to_string());
    }
    if let Some(ref format) = self.log_format {
      args.push("--log-format".into());
      args.push(format.clone());
    }
    if self.keep_going {
      args.push("--keep-going".into());
    }
    if self.keep_failed {
      args.push("--keep-failed".into());
    }
    if self.fallback {
      args.push("--fallback".into());
    }
    if self.repair {
      args.push("--repair".into());
    }
    if let Some(ref builders) = self.builders {
      args.push("--builders".into());
      args.push(builders.clone());
    }
    for inc in &self.include {
      args.push("--include".into());
      args.push(inc.clone());
    }
    if self.print_build_logs {
      args.push("--print-build-logs".into());
    }
    if self.show_trace {
      args.push("--show-trace".into());
    }
    if self.accept_flake_config {
      args.push("--accept-flake-config".into());
    }
    if self.refresh {
      args.push("--refresh".into());
    }
    if self.impure {
      args.push("--impure".into());
    }
    if self.offline {
      args.push("--offline".into());
    }
    if self.no_net {
      args.push("--no-net".into());
    }
    if self.recreate_lock_file {
      args.push("--recreate-lock-file".into());
    }
    if self.no_update_lock_file {
      args.push("--no-update-lock-file".into());
    }
    if self.no_write_lock_file {
      args.push("--no-write-lock-file".into());
    }
    if self.no_use_registries {
      args.push("--no-use-registries".into());
    }
    if self.no_registries {
      warn!("--no-registries is deprecated, use --no-use-registries instead");
      args.push("--no-use-registries".into());
    }
    if self.no_build_output {
      args.push("--quiet".into());
    }
    for pair in self.option.chunks(2) {
      args.push("--option".into());
      args.push(pair[0].clone());
      args.push(pair[1].clone());
    }
    for pair in self.override_input.chunks(2) {
      args.push("--override-input".into());
      args.push(pair[0].clone());
      args.push(pair[1].clone());
    }
  }

  #[must_use]
  pub fn generate_passthrough_args(&self) -> Vec<String> {
    let mut args = Vec::new();
    self.append_execution_args(&mut args);
    if self.json {
      args.push("--json".into());
    }
    args
  }

  #[must_use]
  pub fn generate_build_args(&self) -> Vec<String> {
    let mut args = Vec::new();
    self.append_execution_args(&mut args);
    args
  }

  /// Generate the settings a Nix daemon applies to the builds it runs, as
  /// `nix-daemon` arguments.
  ///
  /// Nix splits `remote-program` on whitespace, so a setting containing any is
  /// skipped with a warning.
  #[must_use]
  pub fn generate_daemon_args(&self) -> Vec<String> {
    let mut args = Vec::new();
    let mut forward = |setting: &[&str]| {
      if setting
        .iter()
        .any(|word| word.contains(char::is_whitespace))
      {
        warn!(
          "Not forwarding `{}` to the build host, as it contains whitespace",
          setting.join(" ")
        );
      } else {
        args.extend(setting.iter().map(|&word| word.to_owned()));
      }
    };

    if let Some(jobs) = self.max_jobs {
      forward(&["--max-jobs", &jobs.to_string()]);
    }
    if let Some(cores) = self.cores {
      forward(&["--cores", &cores.to_string()]);
    }
    if self.keep_going {
      forward(&["--keep-going"]);
    }
    if self.keep_failed {
      forward(&["--keep-failed"]);
    }
    if self.fallback {
      forward(&["--fallback"]);
    }
    if let Some(ref builders) = self.builders {
      forward(&["--builders", builders]);
    }
    for [name, value] in self.option.as_chunks().0 {
      forward(&["--option", name, value]);
    }

    args
  }
}

#[cfg(test)]
mod tests {
  use super::NixBuildPassthroughArgs;

  #[test]
  fn no_build_output_maps_to_nix_quiet_flag() {
    let args = NixBuildPassthroughArgs {
      no_build_output: true,
      ..Default::default()
    };

    assert_eq!(args.generate_passthrough_args(), ["--quiet"]);
  }

  #[test]
  fn option_pairs_are_emitted() {
    let args = NixBuildPassthroughArgs {
      option: vec![
        "sandbox".into(),
        "false".into(),
        "cores".into(),
        "4".into(),
      ],
      ..Default::default()
    };

    assert_eq!(args.generate_passthrough_args(), [
      "--option", "sandbox", "false", "--option", "cores", "4"
    ]);
  }

  #[test]
  fn evaluation_args_include_flake_resolution_flags_without_output_flags() {
    let args = NixBuildPassthroughArgs {
      offline: true,
      no_net: true,
      recreate_lock_file: true,
      json: true,
      no_build_output: true,
      ..Default::default()
    };

    assert_eq!(args.generate_evaluation_args(), [
      "--offline",
      "--no-net",
      "--recreate-lock-file"
    ]);
  }

  #[test]
  fn legacy_evaluation_args_exclude_new_cli_flags() {
    let args = NixBuildPassthroughArgs {
      include: vec!["nixpkgs=/src".into()],
      impure: true,
      offline: true,
      no_net: true,
      recreate_lock_file: true,
      ..Default::default()
    };

    assert_eq!(args.generate_legacy_evaluation_args(), [
      "--include",
      "nixpkgs=/src",
      "--impure",
    ]);
  }

  #[test]
  fn update_args_exclude_flags_that_prevent_updates() {
    let args = NixBuildPassthroughArgs {
      no_net: true,
      recreate_lock_file: true,
      no_update_lock_file: true,
      no_write_lock_file: true,
      override_input: vec!["nixpkgs".into(), "path:/src".into()],
      ..Default::default()
    };

    assert_eq!(args.generate_update_args(), [
      "--no-net",
      "--recreate-lock-file"
    ]);
  }

  #[test]
  fn override_input_pairs_are_emitted() {
    let args = NixBuildPassthroughArgs {
      override_input: vec![
        "nixpkgs".into(),
        "github:NixOS/nixpkgs/nixos-unstable".into(),
      ],
      ..Default::default()
    };

    assert_eq!(args.generate_passthrough_args(), [
      "--override-input",
      "nixpkgs",
      "github:NixOS/nixpkgs/nixos-unstable"
    ]);
  }

  #[test]
  fn build_args_exclude_json() {
    let args = NixBuildPassthroughArgs {
      json: true,
      no_net: true,
      ..Default::default()
    };

    assert_eq!(args.generate_build_args(), ["--no-net"]);
  }
}

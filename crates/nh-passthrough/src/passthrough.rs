use clap::Args;
use tracing::warn;

#[derive(Debug, Args)]
pub struct NixBuildPassthroughArgs {
  /// Number of concurrent jobs Nix should run
  #[arg(long, short = 'j')]
  pub max_jobs: Option<usize>,

  /// Number of cores Nix should utilize
  #[arg(long)]
  pub cores: Option<usize>,

  /// Logging format used by Nix
  #[arg(long)]
  pub log_format: Option<String>,

  /// Continue building despite encountering errors
  #[arg(long, short = 'k')]
  pub keep_going: bool,

  /// Keep build outputs from failed builds
  #[arg(long, short = 'K')]
  pub keep_failed: bool,

  /// Attempt to build locally if substituters fail
  #[arg(long)]
  pub fallback: bool,

  /// Repair corrupted store paths
  #[arg(long)]
  pub repair: bool,

  /// Explicitly define remote builders
  #[arg(long)]
  pub builders: Option<String>,

  /// Paths to include
  #[arg(long, short = 'I')]
  pub include: Vec<String>,

  /// Print build logs directly to stdout
  #[arg(long, short = 'L')]
  pub print_build_logs: bool,

  /// Display tracebacks on errors
  #[arg(long, short = 't')]
  pub show_trace: bool,

  /// Accept configuration from flakes
  #[arg(long)]
  pub accept_flake_config: bool,

  /// Refresh flakes to the latest revision
  #[arg(long)]
  pub refresh: bool,

  /// Allow impure builds
  #[arg(long)]
  pub impure: bool,

  /// Build without internet access
  #[arg(long)]
  pub offline: bool,

  /// Prohibit network usage
  #[arg(long)]
  pub no_net: bool,

  /// Recreate the flake.lock file entirely
  #[arg(long)]
  pub recreate_lock_file: bool,

  /// Do not update the flake.lock file
  #[arg(long)]
  pub no_update_lock_file: bool,

  /// Do not write a lock file
  #[arg(long)]
  pub no_write_lock_file: bool,

  /// Do not use registries
  #[arg(long = "no-use-registries")]
  pub no_use_registries: bool,

  /// Do not use registries (deprecated, use --no-use-registries)
  #[arg(long, alias = "no-registries")]
  pub no_registries: bool,

  /// Commit the lock file after updates
  #[arg(long)]
  pub commit_lock_file: bool,

  /// Suppress build output
  #[arg(long, short = 'Q')]
  pub no_build_output: bool,

  /// Use substitutes when copying
  #[arg(long)]
  pub use_substitutes: bool,

  /// Output results in JSON format
  #[arg(long)]
  pub json: bool,
}

impl NixBuildPassthroughArgs {
  #[must_use]
  pub fn generate_passthrough_args(&self) -> Vec<String> {
    let mut args = Vec::new();

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
      args.push("--no-build-output".into());
    }
    if self.json {
      args.push("--json".into());
    }

    args
  }
}

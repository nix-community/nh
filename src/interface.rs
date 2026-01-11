use std::{env, path::PathBuf};

use anstyle::Style;
use clap::{Args, Parser, Subcommand, ValueEnum, builder::Styles};
use clap_verbosity_flag::InfoLevel;
use tracing::warn;

use crate::{
  Result,
  checks::{
    DarwinReplFeatures,
    FeatureRequirements,
    FlakeFeatures,
    HomeReplFeatures,
    LegacyFeatures,
    NoFeatures,
    OsReplFeatures,
  },
  commands::ElevationStrategy,
  generations::Field,
  installable::Installable,
};

const fn make_style() -> Styles {
  Styles::plain().header(Style::new().bold()).literal(
    Style::new()
      .bold()
      .fg_color(Some(anstyle::Color::Ansi(anstyle::AnsiColor::Yellow))),
  )
}

#[derive(Parser, Debug)]
#[command(
    version,
    about,
    long_about = None,
    styles=make_style(),
    propagate_version = false,
    help_template = "
{name} {version}
{about-with-newline}
{usage-heading} {usage}

{all-args}{after-help}
"
)]
/// Yet another nix helper
pub struct Main {
  #[command(flatten)]
  /// Increase logging verbosity, can be passed multiple times for
  /// more detailed logs.
  pub verbosity: clap_verbosity_flag::Verbosity<InfoLevel>,

  #[arg(
    short,
    long,
    global = true,
    env = "NH_ELEVATION_STRATEGY",
    value_hint = clap::ValueHint::CommandName,
    alias = "elevation-program"
  )]
  /// Choose the privilege elevation strategy.
  ///
  /// Can be a path to an elevation program (e.g., /usr/bin/sudo),
  /// or one of: 'none' (no elevation),
  /// 'passwordless' (use elevation without password prompt for remote hosts
  /// with NOPASSWD configured), or 'auto' (automatically detect available
  /// elevation programs in order: doas, sudo, run0, pkexec)
  pub elevation_strategy: Option<crate::commands::ElevationStrategyArg>,

  #[command(subcommand)]
  pub command: NHCommand,
}

#[derive(Subcommand, Debug)]
#[command(disable_help_subcommand = true)]
pub enum NHCommand {
  Os(OsArgs),
  Home(HomeArgs),
  Darwin(DarwinArgs),
  Search(SearchArgs),
  Clean(CleanProxy),
}

impl NHCommand {
  #[must_use]
  pub fn get_feature_requirements(&self) -> Box<dyn FeatureRequirements> {
    match self {
      Self::Os(args) => args.get_feature_requirements(),
      Self::Home(args) => args.get_feature_requirements(),
      Self::Darwin(args) => args.get_feature_requirements(),
      Self::Search(..) | Self::Clean(..) => Box::new(NoFeatures),
    }
  }

  pub fn run(self, elevation: ElevationStrategy) -> Result<()> {
    // Check features specific to this command
    let requirements = self.get_feature_requirements();
    requirements.check_features()?;

    match self {
      Self::Os(args) => {
        unsafe {
          std::env::set_var("NH_CURRENT_COMMAND", "os");
        }
        args.run(elevation)
      },
      Self::Search(args) => args.run(),
      Self::Clean(proxy) => proxy.command.run(elevation),
      Self::Home(args) => {
        unsafe {
          std::env::set_var("NH_CURRENT_COMMAND", "home");
        }
        args.run()
      },
      Self::Darwin(args) => {
        unsafe {
          std::env::set_var("NH_CURRENT_COMMAND", "darwin");
        }
        args.run(elevation)
      },
    }
  }
}

#[derive(Args, Debug)]
#[clap(verbatim_doc_comment)]
/// `NixOS` functionality
///
/// Implements functionality mostly around but not exclusive to nixos-rebuild
pub struct OsArgs {
  #[command(subcommand)]
  pub subcommand: OsSubcommand,
}

impl OsArgs {
  #[must_use]
  pub fn get_feature_requirements(&self) -> Box<dyn FeatureRequirements> {
    match &self.subcommand {
      OsSubcommand::Repl(args) => {
        let is_flake = args.uses_flakes();
        Box::new(OsReplFeatures { is_flake })
      },
      OsSubcommand::Switch(args)
      | OsSubcommand::Boot(args)
      | OsSubcommand::Test(args) => {
        if args.rebuild.uses_flakes() {
          Box::new(FlakeFeatures)
        } else {
          Box::new(LegacyFeatures)
        }
      },
      OsSubcommand::Build(args) => {
        if args.uses_flakes() {
          Box::new(FlakeFeatures)
        } else {
          Box::new(LegacyFeatures)
        }
      },
      OsSubcommand::BuildVm(args) => {
        if args.common.uses_flakes() {
          Box::new(FlakeFeatures)
        } else {
          Box::new(LegacyFeatures)
        }
      },
      OsSubcommand::Info(_) | OsSubcommand::Rollback(_) => {
        Box::new(LegacyFeatures)
      },
    }
  }
}

#[derive(Debug, Subcommand)]
pub enum OsSubcommand {
  /// Build and activate the new configuration, and make it the boot default
  Switch(OsRebuildActivateArgs),

  /// Build the new configuration and make it the boot default
  Boot(OsRebuildActivateArgs),

  /// Build and activate the new configuration
  Test(OsRebuildActivateArgs),

  /// Build the new configuration
  Build(OsRebuildArgs),

  /// Load system in a repl
  Repl(OsReplArgs),

  /// List available generations from profile path
  Info(OsGenerationsArgs),

  /// Rollback to a previous generation
  Rollback(OsRollbackArgs),

  /// Build a `NixOS` VM image
  BuildVm(OsBuildVmArgs),
}

#[derive(Debug, Args)]
pub struct OsBuildVmArgs {
  #[command(flatten)]
  pub common: OsRebuildArgs,

  /// Build with bootloader. Bootloader is bypassed by default.
  #[arg(long, short = 'B')]
  pub with_bootloader: bool,

  /// Run the VM immediately after building
  #[arg(long, short = 'r')]
  pub run: bool,
}

#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct OsRebuildArgs {
  #[command(flatten)]
  pub common: CommonRebuildArgs,

  #[command(flatten)]
  pub update_args: UpdateArgs,

  /// When using a flake installable, select this hostname from
  /// nixosConfigurations
  #[arg(long, short = 'H', global = true)]
  pub hostname: Option<String>,

  /// Explicitly select some specialisation
  #[arg(long, short)]
  pub specialisation: Option<String>,

  /// Ignore specialisations
  #[arg(long, short = 'S')]
  pub no_specialisation: bool,

  /// Install bootloader for switch and boot commands
  #[arg(long)]
  pub install_bootloader: bool,

  /// Extra arguments passed to nix build
  #[arg(last = true)]
  pub extra_args: Vec<String>,

  /// Don't panic if calling nh as root
  #[arg(short = 'R', long, env = "NH_BYPASS_ROOT_CHECK")]
  pub bypass_root_check: bool,

  /// Deploy the built configuration to a different host over SSH
  #[arg(long)]
  pub target_host: Option<String>,

  /// Build the configuration on a different host over SSH
  #[arg(long)]
  pub build_host: Option<String>,

  /// Skip pre-activation system validation checks
  #[arg(long, env = "NH_NO_VALIDATE")]
  pub no_validate: bool,
}

#[derive(Debug, Args)]
pub struct OsRebuildActivateArgs {
  #[command(flatten)]
  pub rebuild: OsRebuildArgs,

  /// Show activation logs
  #[arg(long, env = "NH_SHOW_ACTIVATION_LOGS")]
  pub show_activation_logs: bool,
}

impl OsRebuildArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    // Check environment variables first
    if env::var("NH_OS_FLAKE").is_ok_and(|v| !v.is_empty()) {
      return true;
    }

    // Check installable type
    matches!(self.common.installable, Installable::Flake { .. })
  }
}

#[derive(ValueEnum, Clone, Default, Debug)]
pub enum DiffType {
  /// Display package diff only if the of the
  /// current and the deployed configuration matches
  #[default]
  Auto,
  /// Always display package diff
  Always,
  /// Never display package diff
  Never,
}

#[derive(Debug, Args)]
pub struct OsRollbackArgs {
  /// Only print actions, without performing them
  #[arg(long, short = 'n')]
  pub dry: bool,

  /// Ask for confirmation
  #[arg(long, short)]
  pub ask: bool,

  /// Explicitly select some specialisation
  #[arg(long, short)]
  pub specialisation: Option<String>,

  /// Ignore specialisations
  #[arg(long, short = 'S')]
  pub no_specialisation: bool,

  /// Rollback to a specific generation number (defaults to previous
  /// generation)
  #[arg(long, short)]
  pub to: Option<u64>,

  /// Don't panic if calling nh as root
  #[arg(short = 'R', long, env = "NH_BYPASS_ROOT_CHECK")]
  pub bypass_root_check: bool,

  /// Whether to display a package diff
  #[arg(long, short, value_enum, default_value_t = DiffType::Auto)]
  pub diff: DiffType,
}

#[derive(Debug, Args)]
pub struct CommonRebuildArgs {
  /// Only print actions, without performing them
  #[arg(long, short = 'n')]
  pub dry: bool,

  /// Ask for confirmation
  #[arg(long, short)]
  pub ask: bool,

  #[command(flatten)]
  pub installable: Installable,

  /// Don't use nix-output-monitor for the build process
  #[arg(long)]
  pub no_nom: bool,

  /// Path to save the result link, defaults to using a temporary directory
  #[arg(long, short)]
  pub out_link: Option<PathBuf>,

  /// Whether to display a package diff
  #[arg(long, short, value_enum, default_value_t = DiffType::Auto)]
  pub diff: DiffType,

  #[command(flatten)]
  pub passthrough: NixBuildPassthroughArgs,
}

#[derive(Debug, Args)]
pub struct OsReplArgs {
  #[command(flatten)]
  pub installable: Installable,

  /// When using a flake installable, select this hostname from
  /// nixosConfigurations
  #[arg(long, short = 'H', global = true)]
  pub hostname: Option<String>,
}

impl OsReplArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    // Check environment variables first
    if env::var("NH_OS_FLAKE").is_ok() {
      return true;
    }

    // Check installable type
    matches!(self.installable, Installable::Flake { .. })
  }
}

#[derive(Debug, Args)]
pub struct OsGenerationsArgs {
  /// Path to Nix' profiles directory
  #[arg(long, short = 'P', default_value = "/nix/var/nix/profiles/system")]
  pub profile: Option<String>,

  /// Comma-delimited list of field(s) to display
  #[arg(long, value_delimiter = ',')]
  pub fields: Option<Vec<Field>>,
}

#[derive(Args, Debug)]
/// Searches packages by querying search.nixos.org
pub struct SearchArgs {
  #[arg(long, short, default_value = "30")]
  /// Number of search results to display
  pub limit: u64,

  #[arg(
    long,
    short,
    env = "NH_SEARCH_CHANNEL",
    default_value = "nixos-unstable"
  )]
  /// Name of the channel to query (e.g nixos-23.11, nixos-unstable, etc)
  pub channel: String,

  #[arg(long, short = 'P', env = "NH_SEARCH_PLATFORM", value_parser = clap::builder::BoolishValueParser::new())]
  /// Show supported platforms for each package
  pub platforms: bool,

  #[arg(long, short = 'j', env = "NH_SEARCH_JSON", value_parser = clap::builder::BoolishValueParser::new())]
  /// Output results as JSON
  pub json: bool,

  /// Name of the package to search
  pub query: Vec<String>,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum SearchNixpkgsFrom {
  Flake,
  Path,
}

// Needed a struct to have multiple sub-subcommands
#[derive(Debug, Clone, Args)]
pub struct CleanProxy {
  #[clap(subcommand)]
  command: CleanMode,
}

#[derive(Debug, Clone, Subcommand)]
/// Enhanced nix cleanup
pub enum CleanMode {
  /// Clean all profiles
  All(CleanArgs),
  /// Clean the current user's profiles
  User(CleanArgs),
  /// Clean a specific profile
  Profile(CleanProfileArgs),
}

#[derive(Args, Clone, Debug)]
#[clap(verbatim_doc_comment)]
/// Enhanced nix cleanup
///
/// For --keep-since, see the documentation of humantime for possible formats: <https://docs.rs/humantime/latest/humantime/fn.parse_duration.html>
pub struct CleanArgs {
  #[arg(long, short, default_value = "1")]
  /// At least keep this number of generations
  pub keep: u32,

  #[arg(long, short = 'K', default_value = "0h")]
  /// At least keep gcroots and generations in this time range since now.
  pub keep_since: humantime::Duration,

  /// Only print actions, without performing them
  #[arg(long, short = 'n')]
  pub dry: bool,

  /// Ask for confirmation
  #[arg(long, short)]
  pub ask: bool,

  /// Don't run nix store --gc
  #[arg(long = "no-gc", alias = "nogc")]
  pub no_gc: bool,

  /// Don't clean gcroots
  #[arg(long = "no-gcroots", alias = "nogcroots")]
  pub no_gcroots: bool,

  /// Run nix-store --optimise after gc
  #[arg(long)]
  pub optimise: bool,

  /// Pass --max to nix store gc
  #[arg(long)]
  pub max: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct CleanProfileArgs {
  #[command(flatten)]
  pub common: CleanArgs,

  /// Which profile to clean
  pub profile: PathBuf,
}

#[derive(Debug, Args)]
/// Home-manager functionality
pub struct HomeArgs {
  #[command(subcommand)]
  pub subcommand: HomeSubcommand,
}

impl HomeArgs {
  #[must_use]
  pub fn get_feature_requirements(&self) -> Box<dyn FeatureRequirements> {
    match &self.subcommand {
      HomeSubcommand::Repl(args) => {
        let is_flake = args.uses_flakes();
        Box::new(HomeReplFeatures { is_flake })
      },
      HomeSubcommand::Switch(args) | HomeSubcommand::Build(args) => {
        if args.uses_flakes() {
          Box::new(FlakeFeatures)
        } else {
          Box::new(LegacyFeatures)
        }
      },
    }
  }
}

#[derive(Debug, Subcommand)]
pub enum HomeSubcommand {
  /// Build and activate a home-manager configuration
  Switch(HomeRebuildArgs),

  /// Build a home-manager configuration
  Build(HomeRebuildArgs),

  /// Load a home-manager configuration in a Nix REPL
  Repl(HomeReplArgs),
}

#[derive(Debug, Args)]
pub struct HomeRebuildArgs {
  #[command(flatten)]
  pub common: CommonRebuildArgs,

  #[command(flatten)]
  pub update_args: UpdateArgs,

  /// Name of the flake homeConfigurations attribute, like username@hostname
  ///
  /// If unspecified, will try <username>@<hostname> and <username>
  #[arg(long, short)]
  pub configuration: Option<String>,

  /// Explicitly select some specialisation
  #[arg(long, short)]
  pub specialisation: Option<String>,

  /// Ignore specialisations
  #[arg(long, short = 'S')]
  pub no_specialisation: bool,

  /// Extra arguments passed to nix build
  #[arg(last = true)]
  pub extra_args: Vec<String>,

  /// Move existing files by backing up with this file extension
  #[arg(long, short = 'b')]
  pub backup_extension: Option<String>,

  /// Show activation logs
  #[arg(long, env = "NH_SHOW_ACTIVATION_LOGS")]
  pub show_activation_logs: bool,

  /// Build the configuration on a different host over SSH
  #[arg(long)]
  pub build_host: Option<String>,
}

impl HomeRebuildArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    // Check environment variables first
    if env::var("NH_HOME_FLAKE").is_ok_and(|v| !v.is_empty()) {
      return true;
    }

    // Check installable type
    matches!(self.common.installable, Installable::Flake { .. })
  }
}

#[derive(Debug, Args)]
pub struct HomeReplArgs {
  #[command(flatten)]
  pub installable: Installable,

  /// Name of the flake homeConfigurations attribute, like username@hostname
  ///
  /// If unspecified, will try <username>@<hostname> and <username>
  #[arg(long, short)]
  pub configuration: Option<String>,

  /// Extra arguments passed to nix repl
  #[arg(last = true)]
  pub extra_args: Vec<String>,
}

impl HomeReplArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    // Check environment variables first
    if env::var("NH_HOME_FLAKE").is_ok_and(|v| !v.is_empty()) {
      return true;
    }

    // Check installable type
    matches!(self.installable, Installable::Flake { .. })
  }
}

/// Nix-darwin functionality
///
/// Implements functionality mostly around but not exclusive to darwin-rebuild
#[derive(Debug, Args)]
pub struct DarwinArgs {
  #[command(subcommand)]
  pub subcommand: DarwinSubcommand,
}

impl DarwinArgs {
  #[must_use]
  pub fn get_feature_requirements(&self) -> Box<dyn FeatureRequirements> {
    match &self.subcommand {
      DarwinSubcommand::Repl(args) => {
        let is_flake = args.uses_flakes();
        Box::new(DarwinReplFeatures { is_flake })
      },
      DarwinSubcommand::Switch(args) | DarwinSubcommand::Build(args) => {
        if args.uses_flakes() {
          Box::new(FlakeFeatures)
        } else {
          Box::new(LegacyFeatures)
        }
      },
    }
  }
}

#[derive(Debug, Subcommand)]
pub enum DarwinSubcommand {
  /// Build and activate a nix-darwin configuration
  Switch(DarwinRebuildArgs),
  /// Build a nix-darwin configuration
  Build(DarwinRebuildArgs),
  /// Load a nix-darwin configuration in a Nix REPL
  Repl(DarwinReplArgs),
}

#[derive(Debug, Args)]
pub struct DarwinRebuildArgs {
  #[command(flatten)]
  pub common: CommonRebuildArgs,

  #[command(flatten)]
  pub update_args: UpdateArgs,

  /// When using a flake installable, select this hostname from
  /// darwinConfigurations
  #[arg(long, short = 'H', global = true)]
  pub hostname: Option<String>,

  /// Extra arguments passed to nix build
  #[arg(last = true)]
  pub extra_args: Vec<String>,

  /// Don't panic if calling nh as root
  #[arg(short = 'R', long, env = "NH_BYPASS_ROOT_CHECK")]
  pub bypass_root_check: bool,

  /// Show activation logs
  #[arg(long, env = "NH_SHOW_ACTIVATION_LOGS")]
  pub show_activation_logs: bool,

  /// Build the configuration on a different host over SSH
  #[arg(long)]
  pub build_host: Option<String>,
}

impl DarwinRebuildArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    // Check environment variables first
    if env::var("NH_DARWIN_FLAKE").is_ok_and(|v| !v.is_empty()) {
      return true;
    }

    // Check installable type
    matches!(self.common.installable, Installable::Flake { .. })
  }
}

#[derive(Debug, Args)]
pub struct DarwinReplArgs {
  #[command(flatten)]
  pub installable: Installable,

  /// When using a flake installable, select this hostname from
  /// darwinConfigurations
  #[arg(long, short = 'H', global = true)]
  pub hostname: Option<String>,
}

impl DarwinReplArgs {
  #[must_use]
  pub fn uses_flakes(&self) -> bool {
    // Check environment variables first
    if env::var("NH_DARWIN_FLAKE").is_ok_and(|v| !v.is_empty()) {
      return true;
    }

    // Check installable type
    matches!(self.installable, Installable::Flake { .. })
  }
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
  #[arg(short = 'u', long = "update", conflicts_with = "update_input")]
  /// Update all flake inputs
  pub update_all: bool,

  #[arg(short = 'U', long = "update-input", conflicts_with = "update_all")]
  /// Update the specified flake input(s)
  pub update_input: Option<Vec<String>>,
}

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
    if self.commit_lock_file {
      args.push("--commit-lock-file".into());
    }
    if self.no_build_output {
      args.push("--no-build-output".into());
    }
    if self.use_substitutes {
      args.push("--use-substitutes".into());
    }
    if self.json {
      args.push("--json".into());
    }

    args
  }
}

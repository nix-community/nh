use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

#[derive(Args, Debug)]
/// Searches packages or NixOS/home-manager options via search.nixos.org,
/// or a local SPAM database
pub struct SearchArgs {
  /// Number of search results to display
  #[arg(long, short, default_value = "30", global = true)]
  pub limit: u64,

  /// Name of the channel to query (e.g nixos-23.11, nixos-unstable, etc)
  #[arg(
    long,
    short,
    env = "NH_SEARCH_CHANNEL",
    default_value = "nixos-unstable"
  )]
  pub channel: String,

  /// Show supported platforms for each package
  #[arg(
    long,
    short = 'P',
    env = "NH_SEARCH_PLATFORM",
    value_parser = clap::builder::BoolishValueParser::new()
  )]
  pub platforms: bool,

  /// Output results as JSON
  #[arg(
    long,
    short = 'j',
    env = "NH_SEARCH_JSON",
    value_parser = clap::builder::BoolishValueParser::new(),
    global = true
  )]
  pub json: bool,

  /// Default search mode used when no subcommand is given.
  /// Accepts `packages` or `options` (scope defaults to `all`).
  #[arg(
    long,
    env = "NH_DEFAULT_SEARCH",
    default_value = "packages",
    value_name = "MODE"
  )]
  pub default_search: SearchDefault,

  #[command(subcommand)]
  pub mode: Option<SearchMode>,

  /// Query shorthand: equivalent to `nh search packages <query>` or
  /// `nh search options <query>` depending on `--default-search`
  pub query: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum SearchMode {
  /// Search packages via search.nixos.org
  Packages(PackagesArgs),
  /// Search NixOS/home-manager options via search.nixos.org
  Options(OptionsArgs),
  /// Search local SPAM database(s) without network access
  Offline(OfflineArgs),
}

#[derive(Args, Debug)]
pub struct PackagesArgs {
  /// Name of the package to search
  #[arg(required = true)]
  pub query: Vec<String>,
}

#[derive(Args, Debug)]
pub struct OptionsArgs {
  /// Options scope: nixpkgs, home-manager, or all (default)
  #[arg(
    long,
    num_args = 0..=1,
    default_missing_value = "all",
    require_equals = true,
    value_name = "SCOPE"
  )]
  pub scope: Option<OptionScope>,

  /// Name of the option to search
  #[arg(required = true)]
  pub query: Vec<String>,
}

#[derive(Args, Debug)]
pub struct OfflineArgs {
  /// Path to a SPAM database file. Specify multiple times to search across
  /// several databases
  #[arg(
    long = "db",
    short = 'D',
    value_name = "PATH",
    env = "NH_OFFLINE_DB",
    value_delimiter = ':',
    required = true
  )]
  pub databases: Vec<PathBuf>,

  /// Name of the package or option to search
  #[arg(required = true)]
  pub query: Vec<String>,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum OptionScope {
  /// Search NixOS options and modular services
  Nixpkgs,
  /// Search home-manager options
  #[value(name = "home-manager")]
  HomeManager,
  /// Search all options (NixOS, services, and home-manager)
  All,
}

#[derive(Debug, Clone, Default, ValueEnum)]
pub enum SearchDefault {
  /// Search packages (default)
  #[default]
  Packages,
  /// Search NixOS/home-manager options (scope defaults to `all`)
  Options,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum SearchNixpkgsFrom {
  Flake,
  Path,
}

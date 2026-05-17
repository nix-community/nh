use clap::{Args, ValueEnum};

#[derive(Args, Debug)]
/// Searches packages or NixOS/home-manager options by querying
/// search.nixos.org
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

  #[arg(
    long,
    num_args = 0..=1,
    default_missing_value = "all",
    require_equals = true,
    value_name = "SCOPE"
  )]
  /// Search NixOS and home-manager module options instead of packages
  /// SCOPE: nixpkgs, home-manager, or all (default)
  pub options: Option<OptionScope>,

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

#[derive(Debug, Clone, ValueEnum)]
pub enum SearchNixpkgsFrom {
  Flake,
  Path,
}

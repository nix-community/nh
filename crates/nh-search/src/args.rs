use clap::{Args, ValueEnum};

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

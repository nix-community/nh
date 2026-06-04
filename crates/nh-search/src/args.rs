use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

#[derive(Args, Debug)]
/// Searches packages or NixOS/home-manager options via search.nixos.org,
/// or a local SPAM database
pub struct SearchArgs {
  /// Number of search results to display
  #[arg(long, short, default_value = "30")]
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
  #[command(flatten)]
  pub limit: LimitArg,

  #[command(flatten)]
  pub channel: ChannelArg,

  #[command(flatten)]
  pub platforms: PlatformsArg,

  /// Name of the package to search
  #[arg(required = true)]
  pub query: Vec<String>,
}

#[derive(Args, Debug)]
pub struct OptionsArgs {
  #[command(flatten)]
  pub limit: LimitArg,

  #[command(flatten)]
  pub channel: ChannelArg,

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
  #[command(flatten)]
  pub limit: LimitArg,

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

#[cfg(test)]
mod tests {
  use clap::{Parser, Subcommand, error::ErrorKind};

  use super::{SearchArgs, SearchDefault, SearchMode};

  #[derive(Debug, Parser)]
  struct TestCli {
    #[command(subcommand)]
    command: TestCommand,
  }

  #[derive(Debug, Subcommand)]
  enum TestCommand {
    Search(SearchArgs),
  }

  fn parse_search(args: &[&str]) -> clap::error::Result<SearchArgs> {
    let cli = TestCli::try_parse_from(
      std::iter::once("nh").chain(args.iter().copied()),
    )?;
    match cli.command {
      TestCommand::Search(search) => Ok(search),
    }
  }

  fn parse_search_error(args: &[&str]) -> clap::error::Result<clap::Error> {
    match parse_search(args) {
      Ok(args) => {
        Err(clap::Error::raw(
          ErrorKind::InvalidValue,
          format!("expected parse error, got {args:?}"),
        ))
      },
      Err(err) => Ok(err),
    }
  }

  #[test]
  fn online_root_flags_parse_before_subcommand() -> clap::error::Result<()> {
    let args = parse_search(&[
      "search",
      "packages",
      "--channel",
      "nixos-unstable",
      "hello",
      "--platforms",
    ])?;

    match args.mode {
      Some(SearchMode::Packages(packages)) => {
        assert_eq!(packages.channel.value, "nixos-unstable");
        assert!(packages.platforms.value);
        assert_eq!(packages.query, ["hello"]);
      },
      other => {
        return Err(clap::Error::raw(
          ErrorKind::InvalidValue,
          format!("expected packages mode, got {other:?}"),
        ));
      },
    }
    Ok(())
  }

  #[test]
  fn online_root_flags_parse_after_subcommand() -> clap::error::Result<()> {
    let args = parse_search(&[
      "search",
      "packages",
      "--channel",
      "nixos-unstable",
      "--platforms",
      "hello",
    ])?;

    match args.mode {
      Some(SearchMode::Packages(packages)) => {
        assert_eq!(packages.channel.value, "nixos-unstable");
        assert!(packages.platforms.value);
        assert_eq!(packages.query, ["hello"]);
      },
      other => {
        return Err(clap::Error::raw(
          ErrorKind::InvalidValue,
          format!("expected packages mode, got {other:?}"),
        ));
      },
    }
    Ok(())
  }

  #[test]
  fn global_limit_and_json_parse_after_subcommand() -> clap::error::Result<()> {
    let args =
      parse_search(&["search", "packages", "--limit", "5", "--json", "hello"])?;

    assert!(args.json);
    match args.mode {
      Some(SearchMode::Packages(packages)) => {
        assert_eq!(packages.limit.value, 5);
        assert_eq!(packages.query, ["hello"]);
      },
      other => {
        return Err(clap::Error::raw(
          ErrorKind::InvalidValue,
          format!("expected packages mode, got {other:?}"),
        ));
      },
    }
    Ok(())
  }

  #[test]
  fn shorthand_flags_parse_after_query() -> clap::error::Result<()> {
    let args = parse_search(&[
      "search",
      "hello",
      "--limit",
      "5",
      "--channel",
      "nixos-unstable",
      "--platforms",
      "--default-search",
      "packages",
    ])?;

    assert_eq!(args.limit, 5);
    assert_eq!(args.channel, "nixos-unstable");
    assert!(args.platforms);
    assert!(matches!(args.default_search, SearchDefault::Packages));
    assert_eq!(args.query, ["hello"]);
    assert!(args.mode.is_none());
    Ok(())
  }

  #[test]
  fn default_search_parses_after_shorthand_query() -> clap::error::Result<()> {
    let args =
      parse_search(&["search", "hello", "--default-search", "options"])?;

    assert!(matches!(args.default_search, SearchDefault::Options));
    assert_eq!(args.query, ["hello"]);
    assert!(args.mode.is_none());
    Ok(())
  }

  #[test]
  fn options_reject_platforms() -> clap::error::Result<()> {
    let err =
      parse_search_error(&["search", "options", "hello", "--platforms"])?;

    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
    Ok(())
  }

  #[test]
  fn offline_rejects_channel() -> clap::error::Result<()> {
    let err = parse_search(&[
      "search",
      "offline",
      "--db",
      "db.sqlite",
      "hello",
      "--channel",
      "nixos-unstable",
    ]);
    let err = match err {
      Ok(args) => {
        return Err(clap::Error::raw(
          ErrorKind::InvalidValue,
          format!("expected parse error, got {args:?}"),
        ));
      },
      Err(err) => err,
    };

    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
    Ok(())
  }
}

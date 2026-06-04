use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};
use color_eyre::{Result, eyre::bail};

const DEFAULT_LIMIT: u64 = 30;
const DEFAULT_CHANNEL: &str = "nixos-unstable";

#[derive(Args, Debug)]
/// Searches packages or NixOS/home-manager options via search.nixos.org,
/// or a local SPAM database
pub struct SearchArgs {
  #[command(flatten)]
  pub limit: LimitArg,

  #[command(flatten)]
  pub channel: ChannelArg,

  #[command(flatten)]
  pub platforms: PlatformsArg,

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
  /// Search Nixpkgs pull requests and branch reachability
  Prs(PrsArgs),
  /// Search Nixpkgs issues, excluding pull requests
  Issues(IssuesArgs),
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

#[derive(Args, Debug)]
pub struct PrsArgs {
  #[command(flatten)]
  pub days: DaysArg,

  /// Pull request search query
  #[arg(required = true)]
  pub query: Vec<String>,
}

#[derive(Args, Debug)]
pub struct IssuesArgs {
  #[command(flatten)]
  pub days: DaysArg,

  /// Issue search query
  #[arg(required = true)]
  pub query: Vec<String>,
}

#[derive(Args, Debug, Clone, Copy)]
pub struct LimitArg {
  /// Number of search results to display
  #[arg(
    id = "limit",
    long = "limit",
    short = 'l',
    default_value_t = DEFAULT_LIMIT
  )]
  pub value: u64,
}

#[derive(Args, Debug, Clone)]
pub struct ChannelArg {
  /// Name of the channel to query (e.g nixos-23.11, nixos-unstable, etc)
  #[arg(
    id = "channel",
    long = "channel",
    short = 'c',
    env = "NH_SEARCH_CHANNEL",
    default_value = DEFAULT_CHANNEL
  )]
  pub value: String,
}

#[derive(Args, Debug, Clone, Copy)]
pub struct PlatformsArg {
  /// Show supported platforms for each package
  #[arg(
    id = "platforms",
    long = "platforms",
    short = 'P',
    env = "NH_SEARCH_PLATFORM",
    value_parser = clap::builder::BoolishValueParser::new()
  )]
  pub value: bool,
}

#[derive(Args, Debug, Clone, Copy)]
pub struct DaysArg {
  /// Search GitHub results from the last n days (default: 15).
  #[arg(
    id = "days",
    short = 'd',
    long = "days",
    value_parser = clap::value_parser!(u32).range(1..)
  )]
  pub value: Option<u32>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OptionScope {
  /// Search NixOS options and modular services
  Nixpkgs,
  /// Search home-manager options
  #[value(name = "home-manager")]
  HomeManager,
  /// Search all options (NixOS, services, and home-manager)
  All,
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum SearchDefault {
  /// Search packages (default)
  #[default]
  Packages,
  /// Search NixOS/home-manager options (scope defaults to `all`)
  Options,
}

pub enum ResolvedSearchMode<'a> {
  Packages {
    channel:   &'a str,
    limit:     u64,
    platforms: bool,
    query:     &'a [String],
  },
  Options {
    channel: &'a str,
    limit:   u64,
    scope:   OptionScope,
    query:   &'a [String],
  },
  Offline {
    limit:     u64,
    databases: &'a [PathBuf],
    query:     &'a [String],
  },
  Prs(&'a PrsArgs),
  Issues(&'a IssuesArgs),
}

impl SearchArgs {
  /// Resolve explicit subcommands and shorthand query arguments into one mode.
  ///
  /// # Errors
  ///
  /// Returns an error when shorthand search is used without a query, or when
  /// shorthand option search receives package-only flags.
  pub fn resolved_mode(&self) -> Result<ResolvedSearchMode<'_>> {
    match &self.mode {
      Some(SearchMode::Packages(args)) => {
        Ok(ResolvedSearchMode::Packages {
          channel:   &args.channel.value,
          limit:     args.limit.value,
          platforms: args.platforms.value,
          query:     &args.query,
        })
      },
      Some(SearchMode::Options(args)) => {
        Ok(ResolvedSearchMode::Options {
          channel: &args.channel.value,
          limit:   args.limit.value,
          scope:   args.scope.unwrap_or(OptionScope::All),
          query:   &args.query,
        })
      },
      Some(SearchMode::Offline(args)) => {
        Ok(ResolvedSearchMode::Offline {
          limit:     args.limit.value,
          databases: &args.databases,
          query:     &args.query,
        })
      },
      Some(SearchMode::Prs(args)) => Ok(ResolvedSearchMode::Prs(args)),
      Some(SearchMode::Issues(args)) => Ok(ResolvedSearchMode::Issues(args)),
      None => self.resolved_shorthand_mode(),
    }
  }

  fn resolved_shorthand_mode(&self) -> Result<ResolvedSearchMode<'_>> {
    if self.query.is_empty() {
      bail!(
        "no query provided; try `nh search packages <query>`, `nh search \
         options <query>`, or `nh search --help`"
      );
    }

    match self.default_search {
      SearchDefault::Packages => {
        Ok(ResolvedSearchMode::Packages {
          channel:   &self.channel.value,
          limit:     self.limit.value,
          platforms: self.platforms.value,
          query:     &self.query,
        })
      },
      SearchDefault::Options => {
        if self.platforms.value {
          bail!("--platforms only applies to package search");
        }

        Ok(ResolvedSearchMode::Options {
          channel: &self.channel.value,
          limit:   self.limit.value,
          scope:   OptionScope::All,
          query:   &self.query,
        })
      },
    }
  }
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

    assert_eq!(args.limit.value, 5);
    assert_eq!(args.channel.value, "nixos-unstable");
    assert!(args.platforms.value);
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

  #[test]
  fn prs_accepts_variadic_query_and_days_after_query() -> clap::error::Result<()>
  {
    let args = parse_search(&["search", "prs", "foo", "bar", "--days", "30"])?;

    match args.mode {
      Some(SearchMode::Prs(prs)) => {
        assert_eq!(prs.days.value, Some(30));
        assert_eq!(prs.query, ["foo", "bar"]);
      },
      other => {
        return Err(clap::Error::raw(
          ErrorKind::InvalidValue,
          format!("expected prs mode, got {other:?}"),
        ));
      },
    }
    Ok(())
  }

  #[test]
  fn prs_accepts_json_after_query() -> clap::error::Result<()> {
    let args = parse_search(&["search", "prs", "hello", "--json"])?;

    assert!(args.json);
    match args.mode {
      Some(SearchMode::Prs(prs)) => {
        assert_eq!(prs.query, ["hello"]);
      },
      other => {
        return Err(clap::Error::raw(
          ErrorKind::InvalidValue,
          format!("expected prs mode, got {other:?}"),
        ));
      },
    }
    Ok(())
  }

  #[test]
  fn prs_rejects_limit() -> clap::error::Result<()> {
    let err = parse_search_error(&["search", "prs", "hello", "--limit", "5"])?;

    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
    Ok(())
  }

  #[test]
  fn prs_rejects_zero_days() -> clap::error::Result<()> {
    let err = parse_search_error(&["search", "prs", "hello", "--days", "0"])?;

    assert_eq!(err.kind(), ErrorKind::ValueValidation);
    Ok(())
  }

  #[test]
  fn issues_accepts_variadic_query_and_days_after_query()
  -> clap::error::Result<()> {
    let args =
      parse_search(&["search", "issues", "foo", "bar", "--days", "30"])?;

    match args.mode {
      Some(SearchMode::Issues(issues)) => {
        assert_eq!(issues.days.value, Some(30));
        assert_eq!(issues.query, ["foo", "bar"]);
      },
      other => {
        return Err(clap::Error::raw(
          ErrorKind::InvalidValue,
          format!("expected issues mode, got {other:?}"),
        ));
      },
    }
    Ok(())
  }

  #[test]
  fn issues_rejects_limit() -> clap::error::Result<()> {
    let err =
      parse_search_error(&["search", "issues", "hello", "--limit", "5"])?;

    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
    Ok(())
  }
}

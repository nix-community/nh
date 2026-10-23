use std::path::PathBuf;

use misstep::{Result, bail};
use pound::{Parse, ValueEnum};

const DEFAULT_CHANNEL: &str = "nixos-unstable";

/// Searches packages or NixOS/home-manager options via search.nixos.org,
/// or a local SPAM database
#[derive(Parse, Debug)]
pub struct SearchArgs {
  #[pound(flatten)]
  pub limit: LimitArg,

  #[pound(flatten)]
  pub channel: ChannelArg,

  #[pound(flatten)]
  pub platforms: PlatformsArg,

  #[pound(flatten)]
  pub backend: BackendArgs,

  /// Output results as JSON
  #[pound(long, short = 'j', env = "NH_SEARCH_JSON", global)]
  pub json: bool,

  /// Default search mode used when no subcommand is given. Accepts
  /// `packages` or `options` (scope defaults to `all`).
  #[pound(
    long,
    env = "NH_DEFAULT_SEARCH",
    value_name = "MODE",
    default = SearchDefault::Packages
  )]
  pub default_search: SearchDefault,

  #[pound(subcommand)]
  pub mode: Option<SearchMode>,

  /// Query shorthand: equivalent to `nh search packages <query>` or
  /// `nh search options <query>` depending on `--default-search`
  #[pound(value_name = "QUERY")]
  pub query: Vec<String>,
}

#[derive(Parse, Debug)]
pub enum SearchMode {
  /// Search packages via search.nixos.org
  Packages {
    #[pound(flatten)]
    args: PackagesArgs,
  },
  /// Search NixOS/home-manager options via search.nixos.org
  Options {
    #[pound(flatten)]
    args: OptionsArgs,
  },
  /// Search local SPAM database(s) without network access
  Offline {
    #[pound(flatten)]
    args: OfflineArgs,
  },
  /// Search Nixpkgs pull requests and branch reachability
  Prs {
    #[pound(flatten)]
    args: PrsArgs,
  },
  /// Search Nixpkgs issues, excluding pull requests
  Issues {
    #[pound(flatten)]
    args: IssuesArgs,
  },
}

#[derive(Parse, Debug)]
pub struct PackagesArgs {
  #[pound(flatten)]
  pub limit: LimitArg,

  #[pound(flatten)]
  pub channel: ChannelArg,

  #[pound(flatten)]
  pub platforms: PlatformsArg,

  #[pound(flatten)]
  pub backend: BackendArgs,

  /// Name of the package to search
  #[pound(min_values = 1)]
  pub query: Vec<String>,
}

#[derive(Parse, Debug)]
pub struct OptionsArgs {
  #[pound(flatten)]
  pub limit: LimitArg,

  #[pound(flatten)]
  pub channel: ChannelArg,

  /// Options scope: nixpkgs, home-manager, nix-darwin, or all (default)
  #[pound(long, default_missing = "all", value_name = "SCOPE")]
  pub scope: Option<OptionScope>,

  #[pound(flatten)]
  pub backend: BackendArgs,

  /// Name of the option to search
  #[pound(min_values = 1)]
  pub query: Vec<String>,
}

#[derive(Parse, Debug)]
pub struct OfflineArgs {
  #[pound(flatten)]
  pub limit: LimitArg,

  /// Database scope to search: pkg, opt, lib, or all (default)
  #[pound(long, default = OfflineScope::All, value_name = "SCOPE")]
  pub scope: OfflineScope,

  /// Path to a SPAM database file. Specify multiple times to search across
  /// several databases
  #[pound(
    long = "db",
    short = 'D',
    value_name = "PATH",
    env = "NH_OFFLINE_DB",
    delimiter = ':',
    min_values = 1
  )]
  pub databases: Vec<PathBuf>,

  /// Name of a package file, option, or library function to search
  #[pound(min_values = 1)]
  pub query: Vec<String>,
}

#[derive(Parse, Debug)]
pub struct PrsArgs {
  #[pound(flatten)]
  pub days: DaysArg,

  /// Pull request search query
  #[pound(min_values = 1)]
  pub query: Vec<String>,
}

#[derive(Parse, Debug)]
pub struct IssuesArgs {
  #[pound(flatten)]
  pub days: DaysArg,

  /// Issue search query
  #[pound(min_values = 1)]
  pub query: Vec<String>,
}

#[derive(Parse, Debug, Clone, Copy)]
pub struct LimitArg {
  /// Number of search results to display
  #[pound(long = "limit", short = 'l', default = "30")]
  pub value: u64,
}

#[derive(Parse, Debug, Clone)]
pub struct ChannelArg {
  /// Name of the channel to query (e.g nixos-23.11, nixos-unstable, etc)
  #[pound(
    long = "channel",
    short = 'c',
    env = "NH_SEARCH_CHANNEL",
    default = { DEFAULT_CHANNEL }
  )]
  pub value: String,
}

#[derive(Parse, Debug, Clone, Copy)]
pub struct BackendArgs {
  /// Backend index version to query on search.nixos.org. Defaults to the
  /// newest version available for the channel, or the version bundled with nh
  /// if that cannot be determined
  #[pound(
    long = "backend-version",
    env = "NH_SEARCH_BACKEND_VERSION",
    value_name = "VERSION"
  )]
  pub version: Option<u32>,

  /// Number of newer index versions to try when the requested version is
  /// outdated (missing on the backend)
  #[pound(
    long = "backend-version-fallbacks",
    env = "NH_SEARCH_BACKEND_FALLBACKS",
    default = "1",
    value_name = "COUNT"
  )]
  pub fallbacks: u32,
}

#[derive(Parse, Debug, Clone, Copy)]
pub struct PlatformsArg {
  /// Show supported platforms for each package
  #[pound(long = "platforms", short = 'P', env = "NH_SEARCH_PLATFORM")]
  pub value: bool,
}

#[derive(Parse, Debug, Clone, Copy)]
pub struct DaysArg {
  /// Search GitHub results updated in the last n days (default: 15).
  #[pound(short = 'd', long = "days", min = 1)]
  pub value: Option<u32>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OptionScope {
  /// Search NixOS options and modular services
  Nixpkgs,
  /// Search home-manager options
  #[pound(name = "home-manager")]
  HomeManager,
  /// Search nix-darwin options
  #[pound(name = "nix-darwin")]
  Darwin,
  /// Search all options (NixOS, services, home-manager, and nix-darwin)
  All,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum OfflineScope {
  /// Search all scopes present in each database
  All,
  /// Search package file paths
  Pkg,
  /// Search NixOS options
  Opt,
  /// Search documented Nix library functions
  Lib,
}

impl OfflineScope {
  #[must_use]
  pub(crate) fn includes(self, scope: spam_db::Scope) -> bool {
    match self {
      Self::All => true,
      Self::Pkg => scope == spam_db::Scope::Pkg,
      Self::Opt => scope == spam_db::Scope::Opt,
      Self::Lib => scope == spam_db::Scope::Lib,
    }
  }
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
    backend:   BackendArgs,
    query:     &'a [String],
  },
  Options {
    channel: &'a str,
    limit:   u64,
    scope:   OptionScope,
    backend: BackendArgs,
    query:   &'a [String],
  },
  Offline {
    limit:     u64,
    databases: &'a [PathBuf],
    scope:     OfflineScope,
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
      Some(SearchMode::Packages { args }) => {
        Ok(ResolvedSearchMode::Packages {
          channel:   &args.channel.value,
          limit:     args.limit.value,
          platforms: args.platforms.value,
          backend:   args.backend,
          query:     &args.query,
        })
      },
      Some(SearchMode::Options { args }) => {
        Ok(ResolvedSearchMode::Options {
          channel: &args.channel.value,
          limit:   args.limit.value,
          scope:   args.scope.unwrap_or(OptionScope::All),
          backend: args.backend,
          query:   &args.query,
        })
      },
      Some(SearchMode::Offline { args }) => {
        Ok(ResolvedSearchMode::Offline {
          limit:     args.limit.value,
          databases: &args.databases,
          scope:     args.scope,
          query:     &args.query,
        })
      },
      Some(SearchMode::Prs { args }) => Ok(ResolvedSearchMode::Prs(args)),
      Some(SearchMode::Issues { args }) => Ok(ResolvedSearchMode::Issues(args)),
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
          backend:   self.backend,
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
          backend: self.backend,
          query:   &self.query,
        })
      },
    }
  }
}

#[cfg(test)]
mod tests {
  use pound::{ErrorKind, Parse};

  use super::{OfflineScope, SearchArgs, SearchDefault, SearchMode};

  #[derive(Debug, Parse)]
  struct TestCli {
    #[pound(subcommand)]
    command: TestCommand,
  }

  #[derive(Debug, Parse)]
  enum TestCommand {
    Search {
      #[pound(flatten)]
      args: SearchArgs,
    },
  }

  fn parse_search(
    args: &[&str],
  ) -> Result<SearchArgs, Box<dyn std::error::Error>> {
    let cli = TestCli::try_parse_from(args.iter().copied())?;
    match cli.command {
      TestCommand::Search { args: search } => Ok(search),
    }
  }

  fn parse_search_error(
    args: &[&str],
  ) -> Result<pound::Error, Box<dyn std::error::Error>> {
    match parse_search(args) {
      Ok(args) => Err(format!("expected parse error, got {args:?}").into()),
      Err(err) => Ok(*err.downcast::<pound::Error>()?),
    }
  }

  #[test]
  fn online_root_flags_parse_before_subcommand()
  -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_search(&[
      "search",
      "packages",
      "--channel",
      "nixos-unstable",
      "hello",
      "--platforms",
    ])?;

    match args.mode {
      Some(SearchMode::Packages { args: packages }) => {
        assert_eq!(packages.channel.value, "nixos-unstable");
        assert!(packages.platforms.value);
        assert_eq!(packages.query, ["hello"]);
      },
      other => {
        return Err(format!("expected packages mode, got {other:?}").into());
      },
    }
    Ok(())
  }

  #[test]
  fn online_root_flags_parse_after_subcommand()
  -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_search(&[
      "search",
      "packages",
      "--channel",
      "nixos-unstable",
      "--platforms",
      "hello",
    ])?;

    match args.mode {
      Some(SearchMode::Packages { args: packages }) => {
        assert_eq!(packages.channel.value, "nixos-unstable");
        assert!(packages.platforms.value);
        assert_eq!(packages.query, ["hello"]);
      },
      other => {
        return Err(format!("expected packages mode, got {other:?}").into());
      },
    }
    Ok(())
  }

  #[test]
  fn global_limit_and_json_parse_after_subcommand()
  -> Result<(), Box<dyn std::error::Error>> {
    let args =
      parse_search(&["search", "packages", "--limit", "5", "--json", "hello"])?;

    assert!(args.json);
    match args.mode {
      Some(SearchMode::Packages { args: packages }) => {
        assert_eq!(packages.limit.value, 5);
        assert_eq!(packages.query, ["hello"]);
      },
      other => {
        return Err(format!("expected packages mode, got {other:?}").into());
      },
    }
    Ok(())
  }

  #[test]
  fn shorthand_flags_parse_after_query()
  -> Result<(), Box<dyn std::error::Error>> {
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
  fn default_search_parses_after_shorthand_query()
  -> Result<(), Box<dyn std::error::Error>> {
    let args =
      parse_search(&["search", "hello", "--default-search", "options"])?;

    assert!(matches!(args.default_search, SearchDefault::Options));
    assert_eq!(args.query, ["hello"]);
    assert!(args.mode.is_none());
    Ok(())
  }

  #[test]
  fn backend_version_flags_parse_and_default()
  -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_search(&[
      "search",
      "packages",
      "hello",
      "--backend-version",
      "51",
      "--backend-version-fallbacks",
      "3",
    ])?;

    match args.mode {
      Some(SearchMode::Packages { args: packages }) => {
        assert_eq!(packages.backend.version, Some(51));
        assert_eq!(packages.backend.fallbacks, 3);
      },
      other => {
        return Err(format!("expected packages mode, got {other:?}").into());
      },
    }

    let defaults = parse_search(&["search", "options", "hello"])?;
    match defaults.mode {
      Some(SearchMode::Options { args: options }) => {
        assert_eq!(options.backend.version, None);
        assert_eq!(options.backend.fallbacks, 1);
      },
      other => {
        return Err(format!("expected options mode, got {other:?}").into());
      },
    }
    Ok(())
  }

  #[test]
  fn options_reject_platforms() -> Result<(), Box<dyn std::error::Error>> {
    let err =
      parse_search_error(&["search", "options", "hello", "--platforms"])?;

    assert!(matches!(err.kind, ErrorKind::Unknown { .. }));
    Ok(())
  }

  #[test]
  fn offline_rejects_channel() -> Result<(), Box<dyn std::error::Error>> {
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
        return Err(format!("expected parse error, got {args:?}").into());
      },
      Err(err) => *err.downcast::<pound::Error>()?,
    };

    assert!(matches!(err.kind, ErrorKind::Unknown { .. }));
    Ok(())
  }

  #[test]
  fn offline_scope_flag_selects_scope() -> Result<(), Box<dyn std::error::Error>>
  {
    for (value, expected) in [
      ("pkg", OfflineScope::Pkg),
      ("opt", OfflineScope::Opt),
      ("lib", OfflineScope::Lib),
      ("all", OfflineScope::All),
    ] {
      let args = parse_search(&[
        "search", "offline", "--db", "data.db", "needle", "--scope", value,
      ])?;
      assert!(matches!(
        args.mode,
        Some(SearchMode::Offline { args: offline }) if offline.scope == expected
      ));
    }

    let args =
      parse_search(&["search", "offline", "--db", "data.db", "needle"])?;
    assert!(matches!(
      args.mode,
      Some(SearchMode::Offline { args: offline }) if offline.scope == OfflineScope::All
    ));
    Ok(())
  }

  #[test]
  fn offline_scope_rejects_unknown_values()
  -> Result<(), Box<dyn std::error::Error>> {
    let err = parse_search_error(&[
      "search", "offline", "--db", "data.db", "--scope", "missing", "needle",
    ])?;
    assert!(matches!(err.kind, ErrorKind::Value { .. }));
    Ok(())
  }

  #[test]
  fn offline_scope_selection_excludes_other_sections() {
    use spam_db::Scope;

    assert!(OfflineScope::All.includes(Scope::Pkg));
    assert!(OfflineScope::All.includes(Scope::Opt));
    assert!(OfflineScope::All.includes(Scope::Lib));
    assert!(OfflineScope::Pkg.includes(Scope::Pkg));
    assert!(!OfflineScope::Pkg.includes(Scope::Opt));
    assert!(!OfflineScope::Pkg.includes(Scope::Lib));
    assert!(OfflineScope::Opt.includes(Scope::Opt));
    assert!(!OfflineScope::Opt.includes(Scope::Pkg));
    assert!(!OfflineScope::Opt.includes(Scope::Lib));
    assert!(OfflineScope::Lib.includes(Scope::Lib));
    assert!(!OfflineScope::Lib.includes(Scope::Pkg));
    assert!(!OfflineScope::Lib.includes(Scope::Opt));
  }

  #[test]
  fn prs_accepts_variadic_query_and_days_after_query()
  -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_search(&["search", "prs", "foo", "bar", "--days", "30"])?;

    match args.mode {
      Some(SearchMode::Prs { args: prs }) => {
        assert_eq!(prs.days.value, Some(30));
        assert_eq!(prs.query, ["foo", "bar"]);
      },
      other => {
        return Err(format!("expected prs mode, got {other:?}").into());
      },
    }
    Ok(())
  }

  #[test]
  fn prs_accepts_json_after_query() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_search(&["search", "prs", "hello", "--json"])?;

    assert!(args.json);
    match args.mode {
      Some(SearchMode::Prs { args: prs }) => {
        assert_eq!(prs.query, ["hello"]);
      },
      other => {
        return Err(format!("expected prs mode, got {other:?}").into());
      },
    }
    Ok(())
  }

  #[test]
  fn prs_rejects_limit() -> Result<(), Box<dyn std::error::Error>> {
    let err = parse_search_error(&["search", "prs", "hello", "--limit", "5"])?;

    assert!(matches!(err.kind, ErrorKind::Unknown { .. }));
    Ok(())
  }

  #[test]
  fn prs_rejects_zero_days() -> Result<(), Box<dyn std::error::Error>> {
    let err = parse_search_error(&["search", "prs", "hello", "--days", "0"])?;

    assert!(matches!(err.kind, ErrorKind::Value { .. }));
    Ok(())
  }

  #[test]
  fn issues_accepts_variadic_query_and_days_after_query()
  -> Result<(), Box<dyn std::error::Error>> {
    let args =
      parse_search(&["search", "issues", "foo", "bar", "--days", "30"])?;

    match args.mode {
      Some(SearchMode::Issues { args: issues }) => {
        assert_eq!(issues.days.value, Some(30));
        assert_eq!(issues.query, ["foo", "bar"]);
      },
      other => {
        return Err(format!("expected issues mode, got {other:?}").into());
      },
    }
    Ok(())
  }

  #[test]
  fn issues_rejects_limit() -> Result<(), Box<dyn std::error::Error>> {
    let err =
      parse_search_error(&["search", "issues", "hello", "--limit", "5"])?;

    assert!(matches!(err.kind, ErrorKind::Unknown { .. }));
    Ok(())
  }
}

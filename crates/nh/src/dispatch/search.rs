use misstep::bail;
use nh_cli::search::{
  IssuesArgs,
  OfflineScope,
  OptionScope,
  PackageField,
  PrsArgs,
  SearchArgs,
  SearchDefault,
  SearchMode,
};
use nh_search::search::{self, IssuesOptions, PrsOptions, SearchCommand};
use tracing::trace;

use crate::Result;

const DEFAULT_LIMIT: u64 = 30;

pub(super) fn run(args: &SearchArgs) -> Result<()> {
  trace!("args: {args:?}");
  resolved_mode(args)?.run(args.json)
}

/// Resolve explicit subcommands and shorthand query arguments into one mode.
///
/// # Errors
///
/// Returns an error when shorthand search is used without a query, or when
/// package-only flags are combined with a non-package mode.
fn resolved_mode(search: &SearchArgs) -> Result<SearchCommand> {
  reject_unused_flags(search)?;

  match &search.mode {
    Some(SearchMode::Packages { args }) => {
      Ok(SearchCommand::Packages {
        channel:   args
          .channel
          .as_deref()
          .unwrap_or(&search.channel.value)
          .to_owned(),
        limit:     limit(search),
        platforms: args.platforms || search.platforms.value,
        fields:    args
          .fields
          .as_deref()
          .or(search.fields.as_deref())
          .map(package_fields),
        query:     args.query.clone(),
      })
    },
    Some(SearchMode::Options { args }) => {
      Ok(SearchCommand::Options {
        channel: args
          .channel
          .as_deref()
          .unwrap_or(&search.channel.value)
          .to_owned(),
        limit:   limit(search),
        scope:   option_scope(args.scope.unwrap_or(OptionScope::All)),
        query:   args.query.clone(),
      })
    },
    Some(SearchMode::Offline { args }) => {
      Ok(SearchCommand::Offline {
        limit:     limit(search),
        databases: args.databases.clone(),
        scope:     offline_scope(args.scope),
        query:     args.query.clone(),
      })
    },
    Some(SearchMode::Prs { args }) => Ok(SearchCommand::Prs(prs(args))),
    Some(SearchMode::Issues { args }) => {
      Ok(SearchCommand::Issues(issues(args)))
    },
    None => resolved_shorthand_mode(search),
  }
}

fn limit(search: &SearchArgs) -> u64 {
  search.limit.value.unwrap_or(DEFAULT_LIMIT)
}

/// Reject top-level flags the selected subcommand would ignore.
///
/// # Errors
///
/// Returns an error when `--fields` is given outside package search, or
/// `--limit` with `prs` or `issues`.
fn reject_unused_flags(search: &SearchArgs) -> Result<()> {
  // Env-backed flags (--channel, --platforms) cannot be told apart from
  // their env vars, so only flags without env are checked.
  let packages =
    matches!(search.mode, None | Some(SearchMode::Packages { .. }));
  let github = matches!(
    search.mode,
    Some(SearchMode::Prs { .. } | SearchMode::Issues { .. })
  );

  if search.fields.is_some() && !packages {
    bail!("--fields only applies to package search");
  }
  if search.limit.value.is_some() && github {
    bail!("--limit does not apply to GitHub search");
  }
  Ok(())
}

fn resolved_shorthand_mode(search: &SearchArgs) -> Result<SearchCommand> {
  if search.query.is_empty() {
    bail!(
      "no query provided; try `nh search packages <query>`, `nh search \
       options <query>`, or `nh search --help`"
    );
  }

  match search.default_search {
    SearchDefault::Packages => {
      Ok(SearchCommand::Packages {
        channel:   search.channel.value.clone(),
        limit:     limit(search),
        platforms: search.platforms.value,
        fields:    search.fields.as_deref().map(package_fields),
        query:     search.query.clone(),
      })
    },
    SearchDefault::Options => {
      if search.platforms.value || search.fields.is_some() {
        bail!("--platforms and --fields only apply to package search");
      }

      Ok(SearchCommand::Options {
        channel: search.channel.value.clone(),
        limit:   limit(search),
        scope:   option_scope(OptionScope::All),
        query:   search.query.clone(),
      })
    },
  }
}

fn prs(args: &PrsArgs) -> PrsOptions {
  PrsOptions {
    days:       args.days.value,
    title_only: args.title.only,
    query:      args.query.clone(),
  }
}

fn issues(args: &IssuesArgs) -> IssuesOptions {
  IssuesOptions {
    days:       args.days.value,
    title_only: args.title.only,
    query:      args.query.clone(),
  }
}

fn package_fields(fields: &[PackageField]) -> Vec<search::PackageField> {
  fields.iter().copied().map(package_field).collect()
}

const fn package_field(value: PackageField) -> search::PackageField {
  match value {
    PackageField::AttrName => search::PackageField::AttrName,
    PackageField::Description => search::PackageField::Description,
    PackageField::MainProgram => search::PackageField::MainProgram,
    PackageField::Homepage => search::PackageField::Homepage,
    PackageField::Platforms => search::PackageField::Platforms,
    PackageField::Position => search::PackageField::Position,
    PackageField::GithubLink => search::PackageField::GithubLink,
    PackageField::Programs => search::PackageField::Programs,
    PackageField::LicenseSet => search::PackageField::LicenseSet,
  }
}

const fn option_scope(value: OptionScope) -> search::OptionScope {
  match value {
    OptionScope::Nixpkgs => search::OptionScope::Nixpkgs,
    OptionScope::HomeManager => search::OptionScope::HomeManager,
    OptionScope::Darwin => search::OptionScope::Darwin,
    OptionScope::All => search::OptionScope::All,
  }
}

const fn offline_scope(value: OfflineScope) -> search::OfflineScope {
  match value {
    OfflineScope::All => search::OfflineScope::All,
    OfflineScope::Pkg => search::OfflineScope::Pkg,
    OfflineScope::Opt => search::OfflineScope::Opt,
    OfflineScope::Lib => search::OfflineScope::Lib,
  }
}

#[cfg(test)]
mod tests {
  use nh_cli::search::SearchArgs;
  use pound::Parse;

  use super::resolved_mode;

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

  #[test]
  fn prs_rejects_limit() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_search(&["search", "prs", "hello", "--limit", "5"])?;

    assert!(resolved_mode(&args).is_err());
    Ok(())
  }

  #[test]
  fn issues_rejects_limit() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_search(&["search", "issues", "hello", "--limit", "5"])?;

    assert!(resolved_mode(&args).is_err());
    Ok(())
  }
}

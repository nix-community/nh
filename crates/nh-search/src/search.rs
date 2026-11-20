use std::path::PathBuf;

use misstep::Result;

use crate::{issues, offline, online, prs};

#[derive(Debug, Clone, Copy)]
pub enum PackageField {
  /// Package attribute name and version
  AttrName,
  /// Package description
  Description,
  /// Main program provided by the package
  MainProgram,
  /// Package homepage
  Homepage,
  /// Supported package platforms
  Platforms,
  /// Package source position
  Position,
  /// GitHub link for the package source
  GithubLink,
  /// Programs provided by the package
  Programs,
  /// Package license(s)
  LicenseSet,
}

#[derive(Debug, Clone, Copy)]
pub enum OptionScope {
  /// Search NixOS options and modular services
  Nixpkgs,
  /// Search home-manager options
  HomeManager,
  /// Search nix-darwin options
  Darwin,
  /// Search all options (NixOS, services, home-manager, and nix-darwin)
  All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Options for searching Nixpkgs pull requests.
#[derive(Debug)]
pub struct PrsOptions {
  /// Search results updated in the last n days, if given.
  pub days:       Option<u32>,
  /// Only match the query against titles.
  pub title_only: bool,
  pub query:      Vec<String>,
}

/// Options for searching Nixpkgs issues.
#[derive(Debug)]
pub struct IssuesOptions {
  /// Search results updated in the last n days, if given.
  pub days:       Option<u32>,
  /// Only match the query against titles.
  pub title_only: bool,
  pub query:      Vec<String>,
}

/// A search operation.
#[derive(Debug)]
pub enum SearchCommand {
  /// Search packages via search.nixos.org
  Packages {
    channel:   String,
    limit:     u64,
    platforms: bool,
    fields:    Option<Vec<PackageField>>,
    query:     Vec<String>,
  },
  /// Search NixOS/home-manager options via search.nixos.org
  Options {
    channel: String,
    limit:   u64,
    scope:   OptionScope,
    query:   Vec<String>,
  },
  /// Search local SPAM database(s) without network access
  Offline {
    limit:     u64,
    databases: Vec<PathBuf>,
    scope:     OfflineScope,
    query:     Vec<String>,
  },
  /// Search Nixpkgs pull requests and branch reachability
  Prs(PrsOptions),
  /// Search Nixpkgs issues, excluding pull requests
  Issues(IssuesOptions),
}

impl SearchCommand {
  /// Execute the search.
  ///
  /// # Errors
  ///
  /// Returns an error if the channel is unsupported, or if the underlying
  /// search request fails.
  pub fn run(self, json: bool) -> Result<()> {
    match self {
      Self::Packages {
        channel,
        limit,
        platforms,
        fields,
        query,
      } => {
        online::run_packages(
          &channel,
          limit,
          platforms,
          fields.as_deref(),
          json,
          &query,
        )
      },
      Self::Options {
        channel,
        limit,
        scope,
        query,
      } => online::run_options(&channel, limit, json, scope, &query),
      Self::Offline {
        limit,
        scope,
        databases,
        query,
      } => offline::run(limit, json, scope, &databases, &query),
      Self::Prs(args) => prs::run(json, &args),
      Self::Issues(args) => issues::run(json, &args),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::OfflineScope;

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
}

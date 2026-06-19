use color_eyre::Result;
use tracing::trace;

use crate::{args, issues, offline, online, prs};

impl args::SearchArgs {
  /// Execute the search subcommand.
  ///
  /// # Errors
  ///
  /// Returns an error if no query is provided when using the shorthand form,
  /// if the channel is unsupported, or if the underlying search request fails.
  pub fn run(&self) -> Result<()> {
    trace!("args: {self:?}");
    match self.resolved_mode()? {
      args::ResolvedSearchMode::Packages {
        channel,
        limit,
        platforms,
        query,
      } => online::run_packages(channel, limit, platforms, self.json, query),
      args::ResolvedSearchMode::Options {
        channel,
        limit,
        scope,
        query,
      } => online::run_options(channel, limit, self.json, scope, query),
      args::ResolvedSearchMode::Offline {
        limit,
        databases,
        query,
      } => offline::run(limit, self.json, databases, query),
      args::ResolvedSearchMode::Prs(args) => prs::run(self.json, args),
      args::ResolvedSearchMode::Issues(args) => issues::run(self.json, args),
    }
  }
}

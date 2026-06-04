use color_eyre::{Result, eyre::bail};
use tracing::trace;

use crate::{args, offline, online};

impl args::SearchArgs {
  /// Execute the search subcommand.
  ///
  /// # Errors
  ///
  /// Returns an error if no query is provided when using the shorthand form,
  /// if the channel is unsupported, or if the underlying search request fails.
  pub fn run(&self) -> Result<()> {
    trace!("args: {self:?}");
    match &self.mode {
      Some(args::SearchMode::Packages(args)) => {
        online::run_packages(
          &self.channel,
          self.limit,
          self.platforms,
          self.json,
          &args.query,
        )
      },
      Some(args::SearchMode::Options(args)) => {
        let scope = args.scope.as_ref().unwrap_or(&args::OptionScope::All);
        online::run_options(
          &self.channel,
          self.limit,
          self.json,
          scope,
          &args.query,
        )
      },
      Some(args::SearchMode::Offline(args)) => {
        offline::run(self.limit, self.json, &args.databases, &args.query)
      },
      None => {
        if self.query.is_empty() {
          bail!(
            "no query provided; try `nh search packages <query>`, `nh search \
             options <query>`, or `nh search --help`"
          );
        }
        match self.default_search {
          args::SearchDefault::Packages => {
            online::run_packages(
              &self.channel,
              self.limit,
              self.platforms,
              self.json,
              &self.query,
            )
          },
          args::SearchDefault::Options => {
            online::run_options(
              &self.channel,
              self.limit,
              self.json,
              &args::OptionScope::All,
              &self.query,
            )
          },
        }
      },
    }
  }
}

use std::{path::PathBuf, time::Instant};

use color_eyre::{
  Result,
  eyre::{Context, bail},
};
use spam_db::{FileRecord, OptionRecord, SpamDb};
use tracing::{debug, trace};
use yansi::{Color, Paint};

use crate::{
  args,
  backend::{self, SearchContexts},
  channel,
  online,
  query,
  render,
  types::{
    OfflineJsonOutput,
    OfflineOptionResult,
    OfflinePackageResult,
    OptionJsonOutput,
    OptionSearchResult,
    PackageJsonOutput,
    PackageSearchResult,
  },
};

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
        self.run_offline(&args.databases, &args.query)
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

  #[allow(clippy::cast_possible_truncation)]
  fn run_offline(&self, databases: &[PathBuf], query: &[String]) -> Result<()> {
    let query_s = query.join(" ");
    debug!(?query_s);

    let db_paths: Vec<String> =
      databases.iter().map(|p| p.display().to_string()).collect();

    let then = Instant::now();

    let mut option_results: Vec<(String, OptionRecord)> = Vec::new();
    let mut package_results: Vec<(String, FileRecord)> = Vec::new();

    for db_path in databases {
      let db = SpamDb::open(db_path).with_context(|| {
        format!("opening SPAM database: {}", db_path.display())
      })?;

      let db_label = db_path.display().to_string();

      match db {
        SpamDb::Options(opts_db) => {
          let records = opts_db.query(&query_s).with_context(|| {
            format!("querying options database: {}", db_path.display())
          })?;
          for rec in records {
            option_results.push((db_label.clone(), rec));
          }
        },
        SpamDb::Packages(pkgs_db) => {
          let records = pkgs_db.query(&query_s).with_context(|| {
            format!("querying packages database: {}", db_path.display())
          })?;
          for rec in records {
            package_results.push((db_label.clone(), rec));
          }
        },
      }
    }

    let elapsed = then.elapsed();

    if self.json {
      let limit = self.limit as usize;
      // Split the budget evenly; if one category has fewer results than its
      // half, the surplus flows to the other.
      let half = limit / 2;
      let opt_take = option_results.len().min(half);
      let pkg_take = package_results.len().min(limit - opt_take);
      // Redistribute any budget packages didn't consume back to options.
      let opt_take = opt_take
        + (limit - opt_take - pkg_take).min(option_results.len() - opt_take);

      option_results.truncate(opt_take);
      package_results.truncate(pkg_take);

      let offline_opts: Vec<OfflineOptionResult> = option_results
        .into_iter()
        .map(|(db_path, rec)| {
          OfflineOptionResult {
            db_path,
            name: rec.name,
            summary: rec.summary,
          }
        })
        .collect();

      let offline_pkgs: Vec<OfflinePackageResult> = package_results
        .into_iter()
        .map(|(db_path, rec)| {
          OfflinePackageResult {
            db_path,
            path: rec.path,
            packages: rec.packages,
          }
        })
        .collect();

      let json_output = OfflineJsonOutput {
        query: query_s,
        db_paths,
        elapsed_ms: elapsed.as_millis(),
        options: offline_opts,
        packages: offline_pkgs,
      };

      println!("{}", serde_json::to_string_pretty(&json_output)?);
      return Ok(());
    }

    println!("Searching {} offline database(s)...", databases.len());
    println!("Took {}ms", elapsed.as_millis());
    println!();

    let total_results = option_results.len() + package_results.len();
    if total_results == 0 {
      println!("No results found.");
      return Ok(());
    }

    let limit = self.limit as usize;
    // Same fair split as the JSON path: each category gets at most half,
    // with surplus from one flowing to the other.
    let half = limit / 2;
    let opt_take = option_results.len().min(half);
    let pkg_take = package_results.len().min(limit - opt_take);
    let opt_take = opt_take
      + (limit - opt_take - pkg_take).min(option_results.len() - opt_take);
    option_results.truncate(opt_take);
    package_results.truncate(pkg_take);

    let mut shown = 0usize;

    for (db_path, rec) in &option_results {
      if shown >= limit {
        break;
      }
      shown += 1;

      println!();
      print!("{}", Paint::new(&rec.name).fg(Color::Blue));
      println!();
      println!("  Source: {db_path}");

      if let Some(ref summary) = rec.summary {
        let summary = summary.replace('\n', " ");
        for line in
          textwrap::wrap(&summary, textwrap::Options::with_termwidth())
        {
          println!("  {line}");
        }
      }
    }

    for (db_path, rec) in &package_results {
      if shown >= limit {
        break;
      }
      shown += 1;

      println!();
      print!("{}", Paint::new(&rec.path).fg(Color::Blue));
      println!();
      println!("  Source: {db_path}");

      if !rec.packages.is_empty() {
        let pkgs = rec.packages.join(", ");
        for line in textwrap::wrap(&pkgs, textwrap::Options::with_termwidth()) {
          println!("  Packages: {line}");
        }
      }
    }

    Ok(())
  }
}

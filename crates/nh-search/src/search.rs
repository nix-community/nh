use std::{path::PathBuf, sync::OnceLock, time::Instant};

use color_eyre::{
  Result,
  eyre::{Context, bail},
};
use elasticsearch_dsl::{Operator, Query, Search, TextQueryType};
use regex::Regex;
use spam_db::{FileRecord, OptionRecord, SpamDb};
use subprocess::{Exec, Redirection};
use tracing::{debug, trace, warn};
use yansi::{Color, Paint};

use crate::{
  args,
  backend::{self, SearchContexts},
  channel,
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

// Cache the hyperlink support check result
static HYPERLINKS_SUPPORTED: OnceLock<bool> = OnceLock::new();

/// Prints an underlined link in the terminal, where the visible text may be
/// different from the link - or just print the text if hyperlinks aren't
/// supported
fn print_hyperlink(text: &str, link: &str) {
  let hyperlinks =
    *HYPERLINKS_SUPPORTED.get_or_init(supports_hyperlinks::supports_hyperlinks);

  if hyperlinks {
    print!("\x1b]8;;{link}\x07");
    print!("{}", Paint::new(text).underline());
    println!("\x1b]8;;\x07");
  } else {
    println!("{text}");
  }
}

/// Strips HTML tags from a rendered-html description string
fn strip_html(html: &str) -> String {
  static HTML_TAG: OnceLock<Regex> = OnceLock::new();
  let re = HTML_TAG.get_or_init(|| {
    Regex::new(r"<[^>]*>").unwrap_or_else(|e| {
      warn!("invalid HTML strip regex: {e}");
      #[allow(clippy::expect_used)]
      Regex::new("$^").expect("Regex $^ should always be valid")
    })
  });
  re.replace_all(html, "").trim().to_string()
}

const TYPE_OPTION: &str = "option";
const TYPE_SERVICE: &str = "service";
const TYPE_HOME_MANAGER_OPTION: &str = "home-manager-option";

const NIXPKGS_SCOPE_TYPES: &[&str] = &[TYPE_OPTION, TYPE_SERVICE];
const HOME_MANAGER_SCOPE_TYPES: &[&str] = &[TYPE_HOME_MANAGER_OPTION];
const ALL_SCOPE_TYPES: &[&str] =
  &[TYPE_OPTION, TYPE_SERVICE, TYPE_HOME_MANAGER_OPTION];

/// Returns the ES document type strings for a given option scope.
const fn option_scope_types(
  scope: &args::OptionScope,
) -> &'static [&'static str] {
  match scope {
    args::OptionScope::Nixpkgs => NIXPKGS_SCOPE_TYPES,
    args::OptionScope::HomeManager => HOME_MANAGER_SCOPE_TYPES,
    args::OptionScope::All => ALL_SCOPE_TYPES,
  }
}

const fn option_scope_label(scope: &args::OptionScope) -> &'static str {
  match scope {
    args::OptionScope::Nixpkgs => "nixpkgs",
    args::OptionScope::HomeManager => "home-manager",
    args::OptionScope::All => "all",
  }
}

fn capture_nix_path(args: &[&str]) -> Option<String> {
  let capture = Exec::cmd("nix")
    .args(args)
    .stderr(Redirection::None)
    .stdout(Redirection::Pipe)
    .capture()
    .ok()?;

  capture
    .exit_status
    .success()
    .then(|| capture.stdout_str().trim().to_string())
    .filter(|path| !path.is_empty())
}

fn resolve_nixpkgs_path(channel: &str) -> String {
  let flake_ref = if channel == "nixos-unstable" {
    "github:NixOS/nixpkgs/nixos-unstable".to_string()
  } else if channel.starts_with("nixos-") {
    format!("github:NixOS/nixpkgs/{channel}")
  } else {
    "nixpkgs".to_string()
  };

  if let Some(path) = capture_nix_path(&["eval", "-f", "<nixpkgs>", "path"]) {
    return path;
  }

  let flake_path = format!("{flake_ref}#path");
  capture_nix_path(&["eval", "--raw", &flake_path]).unwrap_or_default()
}

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
      Some(args::SearchMode::Packages(args)) => self.run_packages(&args.query),
      Some(args::SearchMode::Options(args)) => {
        let scope = args.scope.as_ref().unwrap_or(&args::OptionScope::All);
        self.run_options(scope, &args.query)
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
          args::SearchDefault::Packages => self.run_packages(&self.query),
          args::SearchDefault::Options => {
            self.run_options(&args::OptionScope::All, &self.query)
          },
        }
      },
    }
  }

  fn run_packages(&self, query: &[String]) -> Result<()> {
    let channel = channel::validate(&self.channel)?;
    let query_s = query.join(" ");
    debug!(?query_s);

    let search = Search::new().from(0).size(self.limit).query(
      Query::bool().filter(Query::term("type", "package")).must(
        Query::dis_max()
          .tie_breaker(0.7)
          .query(
            Query::multi_match(
              [
                "package_attr_name^9",
                "package_attr_name.*^5.3999999999999995",
                "package_programs^9",
                "package_programs.*^5.3999999999999995",
                "package_pname^6",
                "package_pname.*^3.5999999999999996",
                "package_description^1.3",
                "package_description.*^0.78",
                "package_longDescription^1",
                "package_longDescription.*^0.6",
                "flake_name^0.5",
                "flake_name.*^0.3",
              ],
              query_s.clone(),
            )
            .r#type(TextQueryType::CrossFields)
            .analyzer("whitespace")
            .auto_generate_synonyms_phrase_query(false)
            .operator(Operator::And),
          )
          .query(
            Query::wildcard("package_attr_name", format!("*{}*", &query_s))
              .case_insensitive(true),
          ),
      ),
    );

    if !self.json {
      println!("Querying search.nixos.org, with channel {channel}...");
    }
    let (documents, elapsed) = backend::search_documents::<PackageSearchResult>(
      &search,
      &channel,
      SearchContexts {
        build:   "building search query",
        execute: "querying the elasticsearch API",
        parse:   "parsing search document",
      },
    )?;

    if !self.json {
      println!("Took {}ms", elapsed.as_millis());
      println!("Most relevant results at the end");
      println!();
    }

    if self.json {
      let json_output = PackageJsonOutput {
        query: query_s,
        channel,
        elapsed_ms: elapsed.as_millis(),
        results: documents,
      };

      println!("{}", serde_json::to_string_pretty(&json_output)?);
      return Ok(());
    }

    let nixpkgs_path = resolve_nixpkgs_path(&channel);
    debug!("nixpkgs_path: {:?}", nixpkgs_path);

    for elem in documents.iter().rev() {
      println!();
      trace!("{elem:#?}");

      print!("{}", Paint::new(&elem.package_attr_name).fg(Color::Blue));
      let v = &elem.package_pversion;
      if !v.is_empty() {
        print!(" ({})", Paint::new(v).fg(Color::Green));
      }

      println!();

      if let Some(ref desc) = elem.package_description {
        let desc = desc.replace('\n', " ");
        for line in textwrap::wrap(&desc, textwrap::Options::with_termwidth()) {
          println!("  {line}");
        }
      }

      for url in &elem.package_homepage {
        print!("  Homepage: ");
        print_hyperlink(url, url);
      }

      if self.platforms && !elem.package_platforms.is_empty() {
        println!("  Platforms: {}", elem.package_platforms.join(", "));
      }

      if let Some(package_position) = &elem.package_position {
        match package_position.split(':').next() {
          Some(position) => {
            if !nixpkgs_path.is_empty() {
              print!("  Defined at: ");
              print_hyperlink(
                position,
                &format!("file://{nixpkgs_path}/{position}"),
              );

              let github_nixpkgs_url =
                format!("https://github.com/NixOS/nixpkgs/blob/{channel}");

              print!("  GitHub link: ");
              let url = format!("{github_nixpkgs_url}/{position}");
              print_hyperlink(&url, &url);
            }
          },
          None => {
            warn!(
              "Position should have at least one part; received \
               {package_position}"
            );
          },
        }
      }
    }

    Ok(())
  }

  fn run_options(
    &self,
    scope: &args::OptionScope,
    query: &[String],
  ) -> Result<()> {
    let channel = channel::validate(&self.channel)?;
    let query_s = query.join(" ");
    debug!(?query_s, ?scope);

    let option_types = option_scope_types(scope);

    let search = Search::new().from(0).size(self.limit).query(
      Query::bool()
        .filter(Query::terms("type", option_types))
        .must(
          Query::dis_max()
            .tie_breaker(0.7)
            .query(
              Query::multi_match(
                [
                  "option_name^6",
                  "option_name.*^3.6",
                  "option_name_query^6",
                  "option_name_query.*^3.6",
                  "option_description^1",
                  "option_description.*^0.6",
                  "flake_name^0.5",
                  "flake_name.*^0.3",
                  "service_package^3",
                  "service_package.*^1.8",
                  "service_packages^3",
                  "service_packages.*^1.8",
                ],
                query_s.clone(),
              )
              .r#type(TextQueryType::CrossFields)
              .analyzer("whitespace")
              .auto_generate_synonyms_phrase_query(false)
              .operator(Operator::And),
            )
            .query(
              Query::wildcard("option_name", format!("*{}*", &query_s))
                .case_insensitive(true),
            ),
        ),
    );

    if !self.json {
      println!(
        "Querying options on search.nixos.org, with channel {channel}..."
      );
    }
    let (documents, elapsed) = backend::search_documents::<OptionSearchResult>(
      &search,
      &channel,
      SearchContexts {
        build:   "building option search query",
        execute: "querying the elasticsearch API for options",
        parse:   "parsing option search document",
      },
    )?;

    if !self.json {
      println!("Took {}ms", elapsed.as_millis());
      println!("Most relevant results at the end");
      println!();
    }

    if self.json {
      let json_output = OptionJsonOutput {
        query: query_s,
        channel,
        scope: option_scope_label(scope).to_string(),
        elapsed_ms: elapsed.as_millis(),
        results: documents,
      };

      println!("{}", serde_json::to_string_pretty(&json_output)?);
      return Ok(());
    }

    let nixpkgs_path = resolve_nixpkgs_path(&channel);
    debug!("nixpkgs_path: {:?}", nixpkgs_path);

    for elem in documents.iter().rev() {
      println!();
      trace!("{elem:#?}");

      print!("{}", Paint::new(&elem.option_name).fg(Color::Blue));

      if let Some(ref ot) = elem.option_type {
        print!(" :: {}", Paint::new(ot).fg(Color::Green));
      }

      if let Some(ref oe) = elem.option_example {
        print!(" (example: {})", Paint::new(oe).fg(Color::Yellow));
      }

      println!();
      println!("  Scope: {}", elem.r#type);

      if let Some(ref desc) = elem.option_description {
        let desc = strip_html(desc);
        let desc = desc.replace('\n', " ");
        for line in textwrap::wrap(&desc, textwrap::Options::with_termwidth()) {
          println!("  {line}");
        }
      }

      if let Some(ref default) = elem.option_default {
        let prefix = "  Default: ";
        for (i, line) in
          textwrap::wrap(default, textwrap::Options::with_termwidth())
            .iter()
            .enumerate()
        {
          if i == 0 {
            println!("{prefix}{line}");
          } else {
            println!("           {line}");
          }
        }
      }

      if let Some(ref source) = elem.option_source {
        let is_hm = elem.r#type == "home-manager-option";
        let filepath = source.split(':').next().unwrap_or(source);

        if !is_hm && !nixpkgs_path.is_empty() {
          print!("  Defined at: ");
          print_hyperlink(
            filepath,
            &format!("file://{nixpkgs_path}/{filepath}"),
          );
        }

        print!("  Source: ");
        if is_hm {
          let url = format!(
            "https://github.com/nix-community/home-manager/blob/master/\
             {filepath}"
          );
          print_hyperlink(&url, &url);
        } else {
          let url = format!(
            "https://github.com/NixOS/nixpkgs/blob/{channel}/{filepath}"
          );
          print_hyperlink(&url, &url);
        }
      }
    }

    Ok(())
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

use std::{path::PathBuf, time::Instant};

use color_eyre::{
  Result,
  eyre::{Context, bail},
};
use spam_db::{FileRecord, OptionRecord, SpamDb};
use tracing::debug;
use yansi::{Color, Paint};

use crate::types::{
  OfflineJsonOutput,
  OfflineOptionResult,
  OfflinePackageResult,
};

#[allow(clippy::cast_possible_truncation)]
pub fn run(
  limit: u64,
  json: bool,
  databases: &[PathBuf],
  query: &[String],
) -> Result<()> {
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
      SpamDb::Index(_) => {
        bail!(
          "Invalid database format for {}: expected `options` or `packages`, \
           got `index`.",
          db_path.display()
        );
      },
    }
  }

  let elapsed = then.elapsed();
  let has_results = !option_results.is_empty() || !package_results.is_empty();
  let limit = limit as usize;
  let (opt_take, pkg_take) =
    fair_split(option_results.len(), package_results.len(), limit);
  option_results.truncate(opt_take);
  package_results.truncate(pkg_take);

  if json {
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

  if !has_results {
    println!("No results found.");
    return Ok(());
  }

  for (db_path, rec) in &option_results {
    println!();
    print!("{}", Paint::new(&rec.name).fg(Color::Blue));
    println!();
    println!("  Source: {db_path}");

    if let Some(ref summary) = rec.summary {
      let summary = summary.replace('\n', " ");
      for line in textwrap::wrap(&summary, textwrap::Options::with_termwidth())
      {
        println!("  {line}");
      }
    }
  }

  for (db_path, rec) in &package_results {
    println!();
    print!("{}", Paint::new(&rec.path).fg(Color::Blue));
    println!();
    println!("  Source: {db_path}");

    if !rec.packages.is_empty() {
      let pkgs = rec.packages.join(", ");
      let lines = textwrap::wrap(&pkgs, textwrap::Options::with_termwidth());
      if let Some((first, rest)) = lines.split_first() {
        println!("  Packages: {first}");
        for line in rest {
          println!("            {line}");
        }
      }
    }
  }

  Ok(())
}

fn fair_split(
  option_len: usize,
  package_len: usize,
  limit: usize,
) -> (usize, usize) {
  let half = limit / 2;
  let opt_take = option_len.min(half);
  let pkg_take = package_len.min(limit - opt_take);
  let opt_take =
    opt_take + (limit - opt_take - pkg_take).min(option_len - opt_take);
  (opt_take, pkg_take)
}

#[cfg(test)]
mod tests {
  use super::fair_split;

  #[test]
  fn fair_split_balances_even_budget() {
    assert_eq!(fair_split(10, 10, 6), (3, 3));
  }

  #[test]
  fn fair_split_gives_unused_package_budget_to_options() {
    assert_eq!(fair_split(10, 1, 6), (5, 1));
  }

  #[test]
  fn fair_split_gives_unused_option_budget_to_packages() {
    assert_eq!(fair_split(1, 10, 6), (1, 5));
  }

  #[test]
  fn fair_split_preserves_current_odd_limit_preference() {
    assert_eq!(fair_split(10, 10, 1), (0, 1));
  }

  #[test]
  fn fair_split_handles_zero_budget() {
    assert_eq!(fair_split(10, 10, 0), (0, 0));
  }
}

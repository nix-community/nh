use std::{
  path::{Path, PathBuf},
  time::Instant,
};

use misstep::{Result, ResultExt};
use spam_db::{FileRecord, FunctionRecord, OptionRecord, Scope, SpamDb};
use tracing::debug;
use yansi::{Color, Paint};

use crate::{
  args::OfflineScope,
  types::{
    OfflineFunctionResult,
    OfflineJsonOutput,
    OfflineOptionResult,
    OfflinePackageResult,
  },
};

#[allow(clippy::cast_possible_truncation)]
pub fn run(
  limit: u64,
  json: bool,
  scope: OfflineScope,
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
  let mut function_results: Vec<(String, FunctionRecord)> = Vec::new();

  for db_path in databases {
    query_database(
      db_path,
      scope,
      &query_s,
      &mut option_results,
      &mut package_results,
      &mut function_results,
    )?;
  }

  let elapsed = then.elapsed();
  let has_results = !option_results.is_empty()
    || !package_results.is_empty()
    || !function_results.is_empty();
  let limit = limit as usize;
  let (opt_take, pkg_take, fn_take) = fair_split(
    option_results.len(),
    package_results.len(),
    function_results.len(),
    limit,
  );
  option_results.truncate(opt_take);
  package_results.truncate(pkg_take);
  function_results.truncate(fn_take);

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

    let offline_functions: Vec<OfflineFunctionResult> = function_results
      .into_iter()
      .map(|(db_path, rec)| {
        OfflineFunctionResult {
          db_path,
          name: rec.name,
          summary: rec.summary,
          type_signature: rec.type_signature,
          location: rec.location,
          deprecated: rec.deprecated,
        }
      })
      .collect();

    let json_output = OfflineJsonOutput {
      query: query_s,
      db_paths,
      elapsed_ms: elapsed.as_millis(),
      options: offline_opts,
      packages: offline_pkgs,
      functions: offline_functions,
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
    print_option_result(db_path, rec);
  }

  for (db_path, rec) in &package_results {
    print_package_result(db_path, rec);
  }

  for (db_path, rec) in &function_results {
    print_function_result(db_path, rec);
  }

  Ok(())
}

fn print_header(title: &str, db_path: &str) {
  println!();
  print!("{}", Paint::new(title).fg(Color::Blue));
  println!();
  println!("  Source: {db_path}");
}

fn print_wrapped_summary(summary: &str) {
  let summary = summary.replace('\n', " ");
  for line in textwrap::wrap(&summary, textwrap::Options::with_termwidth()) {
    println!("  {line}");
  }
}

fn print_option_result(db_path: &str, rec: &OptionRecord) {
  print_header(&rec.name, db_path);

  if let Some(summary) = &rec.summary {
    print_wrapped_summary(summary);
  }
}

fn print_function_result(db_path: &str, rec: &FunctionRecord) {
  print_header(&rec.name, db_path);
  println!("  Location: {}", rec.location);
  if rec.deprecated {
    println!("  Deprecated");
  }
  if let Some(signature) = &rec.type_signature {
    println!("  Type: {signature}");
  }
  if let Some(summary) = &rec.summary {
    print_wrapped_summary(summary);
  }
}

fn print_package_result(db_path: &str, rec: &FileRecord) {
  print_header(&rec.path, db_path);

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

fn query_database(
  db_path: &Path,
  scope: OfflineScope,
  query: &str,
  option_results: &mut Vec<(String, OptionRecord)>,
  package_results: &mut Vec<(String, FileRecord)>,
  function_results: &mut Vec<(String, FunctionRecord)>,
) -> Result<()> {
  let db = SpamDb::open(db_path)
    .with_context(|| format!("opening SPAM database: {}", db_path.display()))?;

  let db_label = db_path.display().to_string();

  if scope.includes(Scope::Opt)
    && let Some(opts_db) = db.options().with_context(|| {
      format!("opening options scope: {}", db_path.display())
    })?
  {
    let records = opts_db.query(query).with_context(|| {
      format!("querying options database: {}", db_path.display())
    })?;
    option_results
      .extend(records.into_iter().map(|rec| (db_label.clone(), rec)));
  }

  if scope.includes(Scope::Pkg)
    && let Some(pkgs_db) = db.packages().with_context(|| {
      format!("opening packages scope: {}", db_path.display())
    })?
  {
    let records = pkgs_db.query(query).with_context(|| {
      format!("querying packages database: {}", db_path.display())
    })?;
    package_results
      .extend(records.into_iter().map(|rec| (db_label.clone(), rec)));
  }

  if scope.includes(Scope::Lib)
    && let Some(functions_db) = db.functions().with_context(|| {
      format!("opening functions scope: {}", db_path.display())
    })?
  {
    let records = functions_db.query(query).with_context(|| {
      format!("querying functions database: {}", db_path.display())
    })?;
    function_results
      .extend(records.into_iter().map(|rec| (db_label.clone(), rec)));
  }

  Ok(())
}

fn fair_split(
  option_len: usize,
  package_len: usize,
  function_len: usize,
  limit: usize,
) -> (usize, usize, usize) {
  let lengths = [option_len, package_len, function_len];
  let mut take = [0; 3];
  let mut remaining = limit;

  while remaining > 0 {
    let active = lengths
      .iter()
      .zip(take)
      .filter(|(len, taken)| **len > *taken)
      .count();
    if active == 0 {
      break;
    }

    let share = (remaining / active).max(1);
    // Packages first so an odd leftover share keeps going to packages,
    // matching the pre-multi-scope split behavior.
    for index in [1, 0, 2] {
      let count = (lengths[index] - take[index]).min(share).min(remaining);
      take[index] += count;
      remaining -= count;
    }
  }
  take.into()
}

#[cfg(test)]
mod tests {
  use super::fair_split;

  #[test]
  fn fair_split_balances_even_budget() {
    assert_eq!(fair_split(10, 10, 0, 6), (3, 3, 0));
  }

  #[test]
  fn fair_split_gives_unused_package_budget_to_options() {
    assert_eq!(fair_split(10, 1, 0, 6), (5, 1, 0));
  }

  #[test]
  fn fair_split_gives_unused_option_budget_to_packages() {
    assert_eq!(fair_split(1, 10, 0, 6), (1, 5, 0));
  }

  #[test]
  fn fair_split_preserves_current_odd_limit_preference() {
    assert_eq!(fair_split(10, 10, 0, 1), (0, 1, 0));
  }

  #[test]
  fn fair_split_handles_zero_budget() {
    assert_eq!(fair_split(10, 10, 0, 0), (0, 0, 0));
  }

  #[test]
  fn fair_split_balances_three_scopes() {
    assert_eq!(fair_split(10, 10, 10, 8), (3, 3, 2));
  }

  #[test]
  fn fair_split_uses_library_scope_without_other_results() {
    assert_eq!(fair_split(0, 0, 10, 4), (0, 0, 4));
  }

  #[test]
  fn fair_split_reallocates_unused_capacity() {
    assert_eq!(fair_split(10, 1, 10, 8), (4, 1, 3));
  }
}

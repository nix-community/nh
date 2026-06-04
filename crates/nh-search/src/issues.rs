use std::time::Instant;

use color_eyre::Result;
use serde::Serialize;

use crate::{
  args,
  github::{self, GitHubClient, Issue},
  render,
  terminal::SearchProgress,
};

const DEFAULT_DAYS: u32 = 15;

pub fn run(json: bool, args: &args::IssuesArgs) -> Result<()> {
  let query = args.query.join(" ");
  let days = args.days.value.unwrap_or(DEFAULT_DAYS);
  let token = github::auth::token()?;
  let client = GitHubClient::new(token)?;
  let then = Instant::now();

  let progress = SearchProgress::start(
    json,
    format!("Searching NixOS/nixpkgs issues for {query}..."),
  );
  let issues = client.search_issues(&query, days)?;
  progress.finish();

  if json {
    return print_json(query, days, then.elapsed().as_millis(), &issues);
  }

  if issues.is_empty() {
    println!("No issues found for '{query}' in the last {days} days.");
    return Ok(());
  }

  render::issues::print(&issues);
  Ok(())
}

#[derive(Debug, Serialize)]
struct IssueSearchJsonOutput<'a> {
  query:      String,
  days:       u32,
  elapsed_ms: u128,
  results:    Vec<IssueJsonResult<'a>>,
}

#[derive(Debug, Serialize)]
struct IssueJsonResult<'a> {
  number: u64,
  title:  &'a str,
  url:    &'a str,
  state:  &'static str,
}

fn print_json(
  query: String,
  days: u32,
  elapsed_ms: u128,
  issues: &[Issue],
) -> Result<()> {
  let results = issues
    .iter()
    .map(|issue| {
      IssueJsonResult {
        number: issue.number,
        title:  issue.title.as_str(),
        url:    issue.url.as_str(),
        state:  issue.state.as_str(),
      }
    })
    .collect();

  let output = IssueSearchJsonOutput {
    query,
    days,
    elapsed_ms,
    results,
  };

  println!("{}", serde_json::to_string_pretty(&output)?);
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::github::IssueState;

  #[test]
  fn issue_json_uses_plain_urls() -> Result<()> {
    let issue = Issue {
      number: 43,
      title:  "bug report".to_string(),
      url:    "https://github.com/NixOS/nixpkgs/issues/43".to_string(),
      state:  IssueState::Open,
    };

    let output = IssueSearchJsonOutput {
      query:      "bug".to_string(),
      days:       15,
      elapsed_ms: 1,
      results:    vec![IssueJsonResult {
        number: issue.number,
        title:  issue.title.as_str(),
        url:    issue.url.as_str(),
        state:  issue.state.as_str(),
      }],
    };

    let json = serde_json::to_value(output)?;

    assert_eq!(
      json["results"][0]["url"],
      "https://github.com/NixOS/nixpkgs/issues/43"
    );
    Ok(())
  }
}

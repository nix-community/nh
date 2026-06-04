use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
use color_eyre::{Result, eyre::bail};
use serde::Deserialize;
use serde_json::json;

use super::transport::GraphqlClient;

const SEARCH_LIMIT: i64 = 100;

const SEARCH_ISSUES_QUERY: &str = "
query($query: String!, $first: Int!) {
  search(type: ISSUE, query: $query, first: $first) {
    nodes {
      ... on Issue {
        number
        title
        url
        state
      }
    }
  }
}
";

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Issue {
  pub number: u64,
  pub title:  String,
  pub url:    String,
  pub state:  IssueState,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum IssueState {
  Open,
  Closed,
}

impl IssueState {
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Open => "open",
      Self::Closed => "closed",
    }
  }
}

#[derive(Debug, Deserialize)]
struct SearchIssuesData {
  search: SearchNodes<IssueNode>,
}

#[derive(Debug, Deserialize)]
struct SearchNodes<T> {
  nodes: Vec<Option<T>>,
}

#[derive(Debug, Deserialize)]
struct IssueNode {
  number: u64,
  title:  String,
  url:    String,
  state:  String,
}

pub(super) fn search(
  client: &GraphqlClient,
  query: &str,
  days: u32,
) -> Result<Vec<Issue>> {
  let date = (Utc::now() - ChronoDuration::days(i64::from(days)))
    .to_rfc3339_opts(SecondsFormat::Secs, true);
  let github_query = format!(
    "repo:NixOS/nixpkgs {query} type:issue created:>{date} sort:created-desc"
  );
  let data = client.query::<SearchIssuesData>(
    SEARCH_ISSUES_QUERY,
    &json!({
      "query": github_query,
      "first": SEARCH_LIMIT,
    }),
  )?;

  data
    .search
    .nodes
    .into_iter()
    .flatten()
    .map(IssueNode::try_into_issue)
    .collect()
}

impl IssueNode {
  fn try_into_issue(self) -> Result<Issue> {
    Ok(Issue {
      number: self.number,
      title:  self.title,
      url:    self.url,
      state:  parse_state(&self.state)?,
    })
  }
}

fn parse_state(state: &str) -> Result<IssueState> {
  match state {
    "OPEN" | "open" => Ok(IssueState::Open),
    "CLOSED" | "closed" => Ok(IssueState::Closed),
    other => bail!("unknown GitHub issue state {other}"),
  }
}

#[cfg(test)]
mod tests {
  use color_eyre::Result;
  use serde_json::json;

  use super::{IssueNode, IssueState};

  #[test]
  fn parses_open_issue() -> Result<()> {
    let node = serde_json::from_value::<IssueNode>(json!({
      "number": 43,
      "title": "bug report",
      "url": "https://github.com/NixOS/nixpkgs/issues/43",
      "state": "OPEN"
    }))?;
    let issue = node.try_into_issue()?;

    assert_eq!(43, issue.number);
    assert_eq!(IssueState::Open, issue.state);
    Ok(())
  }

  #[test]
  fn rejects_unknown_issue_state() -> Result<()> {
    let node = serde_json::from_value::<IssueNode>(json!({
      "number": 43,
      "title": "bug report",
      "url": "https://github.com/NixOS/nixpkgs/issues/43",
      "state": "LOCKED"
    }))?;

    assert!(node.try_into_issue().is_err());
    Ok(())
  }
}

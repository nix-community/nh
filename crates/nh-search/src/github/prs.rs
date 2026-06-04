use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
use color_eyre::{Result, eyre::bail};
use serde::Deserialize;
use serde_json::json;

use super::transport::GraphqlClient;
pub use super::types::{PullRequest, PullRequestState};

const SEARCH_LIMIT: i64 = 100;

const SEARCH_PULL_REQUESTS_QUERY: &str = "
query($query: String!, $first: Int!) {
  search(type: ISSUE, query: $query, first: $first) {
    nodes {
      ... on PullRequest {
        number
        title
        url
        state
        merged
        baseRefName
        mergeCommit {
          oid
        }
      }
    }
  }
}
";

const PULL_REQUEST_QUERY: &str = "
query($number: Int!) {
  repository(owner: \"NixOS\", name: \"nixpkgs\") {
    pullRequest(number: $number) {
      number
      title
      url
      state
      merged
      baseRefName
      mergeCommit {
        oid
      }
    }
  }
}
";

#[derive(Debug, Deserialize)]
struct SearchPullRequestsData {
  search: SearchNodes<PullRequestNode>,
}

#[derive(Debug, Deserialize)]
struct SearchNodes<T> {
  nodes: Vec<Option<T>>,
}

#[derive(Debug, Deserialize)]
struct PullRequestData {
  repository: RepositoryData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryData {
  pull_request: Option<PullRequestNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestNode {
  number:        u64,
  title:         String,
  url:           String,
  state:         String,
  merged:        bool,
  base_ref_name: String,
  merge_commit:  Option<MergeCommitNode>,
}

#[derive(Debug, Deserialize)]
struct MergeCommitNode {
  oid: String,
}

pub(super) fn search(
  client: &GraphqlClient,
  query: &str,
  days: u32,
) -> Result<Vec<PullRequest>> {
  let date = (Utc::now() - ChronoDuration::days(i64::from(days)))
    .to_rfc3339_opts(SecondsFormat::Secs, true);
  let github_query = format!(
    "repo:NixOS/nixpkgs {query} type:pr created:>{date} sort:created-desc"
  );
  let data = client.query::<SearchPullRequestsData>(
    SEARCH_PULL_REQUESTS_QUERY,
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
    .map(PullRequestNode::try_into_pull_request)
    .collect()
}

pub(super) fn pull_request(
  client: &GraphqlClient,
  number: u64,
) -> Result<Option<PullRequest>> {
  let data = client.query::<PullRequestData>(
    PULL_REQUEST_QUERY,
    &json!({
      "number": number,
    }),
  )?;

  data
    .repository
    .pull_request
    .map(PullRequestNode::try_into_pull_request)
    .transpose()
}

pub fn parse_direct_pr_number(query: &str) -> Option<u64> {
  let query = query.trim();
  let number = query.strip_prefix('#').unwrap_or(query);
  (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
    .then(|| number.parse().ok())
    .flatten()
}

impl PullRequestNode {
  fn try_into_pull_request(self) -> Result<PullRequest> {
    let state = parse_state(self.merged, &self.state)?;
    let merge_commit_sha = self.merge_commit.map(|commit| commit.oid);

    Ok(PullRequest {
      number: self.number,
      title: self.title,
      url: self.url,
      state,
      base_branch: self.base_ref_name,
      merge_commit_sha,
    })
  }
}

fn parse_state(merged: bool, state: &str) -> Result<PullRequestState> {
  if merged {
    return Ok(PullRequestState::Merged);
  }

  match state {
    "OPEN" | "open" => Ok(PullRequestState::Open),
    "CLOSED" | "closed" => Ok(PullRequestState::Closed),
    other => bail!("unknown GitHub pull request state {other}"),
  }
}

#[cfg(test)]
mod tests {
  use color_eyre::Result;
  use serde_json::json;

  use super::{PullRequestNode, PullRequestState, parse_direct_pr_number};

  #[test]
  fn parses_direct_pr_numbers() {
    assert_eq!(Some(123), parse_direct_pr_number("123"));
    assert_eq!(Some(123), parse_direct_pr_number("#123"));
    assert_eq!(None, parse_direct_pr_number("foo 123"));
    assert_eq!(None, parse_direct_pr_number("#"));
  }

  #[test]
  fn parses_merged_pull_request() -> Result<()> {
    let node = serde_json::from_value::<PullRequestNode>(json!({
      "number": 42,
      "title": "hello: 1.0 -> 1.1",
      "url": "https://github.com/NixOS/nixpkgs/pull/42",
      "state": "CLOSED",
      "merged": true,
      "baseRefName": "master",
      "mergeCommit": { "oid": "abc123" }
    }))?;
    let pr = node.try_into_pull_request()?;

    assert_eq!(42, pr.number);
    assert_eq!(PullRequestState::Merged, pr.state);
    assert_eq!(Some("abc123"), pr.merge_commit_sha.as_deref());
    Ok(())
  }

  #[test]
  fn rejects_unknown_pull_request_state() -> Result<()> {
    let node = serde_json::from_value::<PullRequestNode>(json!({
      "number": 42,
      "title": "hello: 1.0 -> 1.1",
      "url": "https://github.com/NixOS/nixpkgs/pull/42",
      "state": "DRAFT",
      "merged": false,
      "baseRefName": "master",
      "mergeCommit": null
    }))?;

    assert!(node.try_into_pull_request().is_err());
    Ok(())
  }
}

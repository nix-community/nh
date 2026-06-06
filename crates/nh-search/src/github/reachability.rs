use std::thread;

use color_eyre::{Report, Result, eyre::ContextCompat};
use serde_json::{Map, Value, json};

use super::transport::GraphqlClient;
pub use super::types::{
  BranchReachability,
  BranchReachabilityRequest,
  BranchReachabilityStatus,
};

const COMPARE_CHUNK_SIZE: usize = 10;
const MAX_PARALLEL_COMPARE_CHUNKS: usize = 4;

pub(super) fn probe(
  client: &GraphqlClient,
  requests: &[BranchReachabilityRequest],
) -> Vec<BranchReachability> {
  let mut results = Vec::with_capacity(requests.len());

  for group in requests.chunks(COMPARE_CHUNK_SIZE * MAX_PARALLEL_COMPARE_CHUNKS)
  {
    let mut group_results = thread::scope(|scope| {
      let mut handles = Vec::new();
      for chunk in group.chunks(COMPARE_CHUNK_SIZE) {
        handles.push(scope.spawn(move || probe_chunk(client, chunk)));
      }

      handles
        .into_iter()
        .flat_map(|handle| {
          handle.join().unwrap_or_else(|payload| {
            std::panic::resume_unwind(payload);
          })
        })
        .collect::<Vec<_>>()
    });

    results.append(&mut group_results);
  }

  results
}

fn probe_chunk(
  client: &GraphqlClient,
  requests: &[BranchReachabilityRequest],
) -> Vec<BranchReachability> {
  match compare_branches(client, requests) {
    Ok(results) => results,
    Err(err) => {
      let reason = error_chain(&err);
      requests
        .iter()
        .map(|request| {
          BranchReachability {
            branch: request.branch.clone(),
            status: BranchReachabilityStatus::Unknown(reason.clone()),
          }
        })
        .collect()
    },
  }
}

fn compare_branches(
  client: &GraphqlClient,
  requests: &[BranchReachabilityRequest],
) -> Result<Vec<BranchReachability>> {
  if requests.is_empty() {
    return Ok(Vec::new());
  }

  let (query, variables) = compare_query(requests);
  let variables = Value::Object(variables);
  let data = client.query::<Value>(&query, &variables)?;
  let repository = data
    .get("repository")
    .context("GitHub GraphQL response missing repository")?;

  requests
    .iter()
    .enumerate()
    .map(|(index, request)| {
      let alias = compare_alias(index);
      let status = repository
        .get(&alias)
        .and_then(|branch| branch.get("compare"))
        .and_then(|compare| compare.get("status"))
        .and_then(Value::as_str)
        .map_or_else(
          || {
            BranchReachabilityStatus::Unknown(
              "comparison unavailable".to_string(),
            )
          },
          compare_status_to_reachability,
        );

      Ok(BranchReachability {
        branch: request.branch.clone(),
        status,
      })
    })
    .collect()
}

fn compare_query(
  requests: &[BranchReachabilityRequest],
) -> (String, Map<String, Value>) {
  let mut variables = Map::new();
  let mut definitions = Vec::new();
  let mut fields = Vec::new();

  for (index, request) in requests.iter().enumerate() {
    let branch_var = format!("branch{index}");
    let commit_var = format!("commit{index}");
    let alias = compare_alias(index);

    definitions.push(format!("${branch_var}: String!, ${commit_var}: String!"));
    fields.push(format!(
      "{alias}: ref(qualifiedName: ${branch_var}) {{ compare(headRef: \
       ${commit_var}) {{ status }} }}"
    ));
    variables.insert(branch_var, json!(request.branch));
    variables.insert(commit_var, json!(request.commit_sha));
  }

  let query = format!(
    "query({}) {{ repository(owner: \"NixOS\", name: \"nixpkgs\") {{ {} }} }}",
    definitions.join(", "),
    fields.join("\n")
  );

  (query, variables)
}

fn compare_alias(index: usize) -> String {
  format!("branch{index}")
}

fn compare_status_to_reachability(status: &str) -> BranchReachabilityStatus {
  match status {
    "BEHIND" | "IDENTICAL" => BranchReachabilityStatus::Contains,
    "AHEAD" | "DIVERGED" => BranchReachabilityStatus::Missing,
    other => {
      BranchReachabilityStatus::Unknown(format!(
        "unknown comparison status {other}"
      ))
    },
  }
}

fn error_chain(error: &Report) -> String {
  error
    .chain()
    .map(ToString::to_string)
    .collect::<Vec<_>>()
    .join(": ")
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::{
    BranchReachabilityRequest,
    BranchReachabilityStatus,
    compare_query,
    compare_status_to_reachability,
    error_chain,
  };

  #[test]
  fn graphql_compare_mapping_tracks_when_branch_contains_commit() {
    assert_eq!(
      BranchReachabilityStatus::Contains,
      compare_status_to_reachability("BEHIND")
    );
    assert_eq!(
      BranchReachabilityStatus::Contains,
      compare_status_to_reachability("IDENTICAL")
    );
    assert_eq!(
      BranchReachabilityStatus::Missing,
      compare_status_to_reachability("AHEAD")
    );
    assert_eq!(
      BranchReachabilityStatus::Missing,
      compare_status_to_reachability("DIVERGED")
    );
  }

  #[test]
  fn compare_query_aliases_branch_checks() {
    let requests = vec![BranchReachabilityRequest {
      branch:     "master".to_string(),
      commit_sha: "abc123".to_string(),
    }];

    let (query, variables) = compare_query(&requests);

    assert!(query.contains("branch0: ref(qualifiedName: $branch0)"));
    assert!(query.contains("compare(headRef: $commit0)"));
    assert_eq!(variables.get("branch0"), Some(&json!("master")));
    assert_eq!(variables.get("commit0"), Some(&json!("abc123")));
  }

  #[test]
  fn unknown_reason_includes_error_chain() {
    use color_eyre::eyre::eyre;

    let error = eyre!("transport timed out").wrap_err("failed to send request");

    assert_eq!(
      error_chain(&error),
      "failed to send request: transport timed out"
    );
  }
}

pub mod auth;

mod issues;
mod prs;
mod reachability;
mod transport;
mod types;

use color_eyre::Result;
use secrecy::SecretString;

use self::transport::GraphqlClient;
pub use self::{
  issues::{Issue, IssueState},
  prs::{PullRequest, PullRequestState, parse_direct_pr_number},
  reachability::{
    BranchReachability,
    BranchReachabilityRequest,
    BranchReachabilityStatus,
  },
};

pub struct GitHubClient {
  graphql: GraphqlClient,
}

impl GitHubClient {
  /// Create a GitHub GraphQL client.
  ///
  /// # Errors
  ///
  /// Returns an error if the HTTP client cannot be built.
  pub fn new(token: SecretString) -> Result<Self> {
    Ok(Self {
      graphql: GraphqlClient::new(token)?,
    })
  }

  /// Search recent Nixpkgs pull requests for a query.
  ///
  /// # Errors
  ///
  /// Returns an error when GitHub search fails or the response shape is
  /// invalid.
  pub fn search_prs(&self, query: &str, days: u32) -> Result<Vec<PullRequest>> {
    prs::search(&self.graphql, query, days)
  }

  /// Fetch a single Nixpkgs pull request by number.
  ///
  /// # Errors
  ///
  /// Returns an error when GitHub lookup fails or the response shape is
  /// invalid.
  pub fn pull_request(&self, number: u64) -> Result<Option<PullRequest>> {
    prs::pull_request(&self.graphql, number)
  }

  /// Search recent Nixpkgs issues for a query.
  ///
  /// # Errors
  ///
  /// Returns an error when GitHub search fails or the response shape is
  /// invalid.
  pub fn search_issues(&self, query: &str, days: u32) -> Result<Vec<Issue>> {
    issues::search(&self.graphql, query, days)
  }

  pub fn probe_branch_reachability(
    &self,
    requests: &[BranchReachabilityRequest],
  ) -> Vec<BranchReachability> {
    reachability::probe(&self.graphql, requests)
  }
}

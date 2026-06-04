#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PullRequest {
  pub number:           u64,
  pub title:            String,
  pub url:              String,
  pub state:            PullRequestState,
  pub base_branch:      String,
  pub merge_commit_sha: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PullRequestState {
  Open,
  Closed,
  Merged,
}

impl PullRequestState {
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::Open => "open",
      Self::Closed => "closed",
      Self::Merged => "merged",
    }
  }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BranchReachability {
  pub branch: String,
  pub status: BranchReachabilityStatus,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BranchReachabilityRequest {
  pub branch:     String,
  pub commit_sha: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BranchReachabilityStatus {
  Contains,
  Missing,
  Unknown(String),
}

impl BranchReachabilityStatus {
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::Contains => "contains",
      Self::Missing => "missing",
      Self::Unknown(_) => "unknown",
    }
  }
}

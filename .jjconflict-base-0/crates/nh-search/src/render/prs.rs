use std::collections::HashMap;

use yansi::{Color, Paint};

use super::common;
use crate::{
  branches::BranchPlan,
  github::{
    BranchReachability,
    BranchReachabilityStatus,
    PullRequest,
    PullRequestState,
  },
};

pub fn print(
  prs: &[PullRequest],
  plans: &[BranchPlan],
  reachability_by_pr: &[Vec<BranchReachability>],
) {
  for ((pr, plan), reachability) in
    prs.iter().zip(plans).zip(reachability_by_pr)
  {
    println!(
      "{} ({}) {}",
      Paint::new(&pr.title).fg(Color::Blue),
      colored_status(pr.state),
      common::hyperlink(&format!("#{}", pr.number), &pr.url),
    );

    if matches!(pr.state, PullRequestState::Merged)
      && pr.merge_commit_sha.is_some()
    {
      print_branch_reachability(reachability, plan.summary_targets());
    }
  }
}

fn colored_status(state: PullRequestState) -> String {
  match state {
    PullRequestState::Open => format!("{}", Paint::new("open").fg(Color::Blue)),
    PullRequestState::Closed => {
      format!("{}", Paint::new("closed").fg(Color::Red))
    },
    PullRequestState::Merged => {
      format!("{}", Paint::new("merged").fg(Color::Green))
    },
  }
}

fn print_branch_reachability(
  reachability: &[BranchReachability],
  summary_targets: &[String],
) {
  let reachability_by_branch = reachability
    .iter()
    .map(|branch| (branch.branch.as_str(), branch))
    .collect::<HashMap<_, _>>();
  let summary_reachability = summary_targets
    .iter()
    .filter_map(|branch| reachability_by_branch.get(branch.as_str()).copied())
    .collect::<Vec<_>>();

  let reached = summary_reachability
    .iter()
    .filter(|branch| {
      matches!(branch.status, BranchReachabilityStatus::Contains)
    })
    .map(|branch| branch.branch.as_str())
    .collect::<Vec<_>>();
  let unknown = summary_reachability
    .iter()
    .filter(|branch| {
      matches!(branch.status, BranchReachabilityStatus::Unknown(_))
    })
    .copied()
    .collect::<Vec<_>>();

  if reached.is_empty() && !unknown.is_empty() {
    println!(
      "   └─ Reachable in: {}",
      Paint::new("Unknown").fg(Color::Yellow)
    );
  } else if reached.is_empty() {
    println!(
      "   └─ Reachable in: {}",
      Paint::new("None").fg(Color::Yellow)
    );
  } else {
    let branches = reached
      .iter()
      .map(|branch| format!("{}", Paint::new(branch).fg(Color::Green)))
      .collect::<Vec<_>>()
      .join(", ");

    println!("   └─ Reachable in: {branches}");
  }

  if !unknown.is_empty() {
    let branches = unknown
      .iter()
      .map(|branch| format_unknown_check(branch))
      .collect::<Vec<_>>()
      .join(", ");

    println!("   └─ Unknown checks: {branches}");
  }
}

fn format_unknown_check(branch: &BranchReachability) -> String {
  let error = match &branch.status {
    BranchReachabilityStatus::Unknown(error) => error,
    BranchReachabilityStatus::Contains | BranchReachabilityStatus::Missing => {
      return format!("{}", Paint::new(&branch.branch).fg(Color::Yellow));
    },
  };
  let error = error.lines().next().unwrap_or("unknown error");
  let mut chars = error.chars();
  let truncated = chars.by_ref().take(96).collect::<String>();
  let error = if chars.next().is_some() {
    format!("{truncated}...")
  } else {
    truncated
  };

  format!("{} ({error})", Paint::new(&branch.branch).fg(Color::Yellow))
}

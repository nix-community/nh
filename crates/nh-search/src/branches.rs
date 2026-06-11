use std::collections::HashSet;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BranchPlan {
  probe_targets:   Vec<String>,
  summary_targets: Vec<String>,
}

impl BranchPlan {
  pub fn from_base_branch(branch: &str) -> Self {
    let mut path = HashSet::new();
    let mut seen = HashSet::new();
    let mut probe_targets = Vec::new();
    collect_branches(branch, &mut path, &mut seen, &mut probe_targets);

    let summary_targets = probe_targets
      .iter()
      .filter(|target| target.as_str() != branch)
      .cloned()
      .collect();

    Self {
      probe_targets,
      summary_targets,
    }
  }

  pub fn probe_targets(&self) -> &[String] {
    &self.probe_targets
  }

  pub fn summary_targets(&self) -> &[String] {
    &self.summary_targets
  }
}

fn collect_branches(
  branch: &str,
  path: &mut HashSet<String>,
  seen: &mut HashSet<String>,
  branches: &mut Vec<String>,
) {
  if !seen.insert(branch.to_string()) {
    return;
  }

  branches.push(branch.to_string());
  path.insert(branch.to_string());

  for next in next_branches(branch) {
    if !path.contains(&next) {
      collect_branches(&next, path, seen, branches);
    }
  }

  path.remove(branch);
}

fn next_branches(branch: &str) -> Vec<String> {
  match branch {
    "staging" => return strings(["staging-next"]),
    "staging-next" | "staging-nixos" => return strings(["master"]),
    "master" => return strings(["nixpkgs-unstable", "nixos-unstable-small"]),
    _ => {},
  }

  if let Some(version) = branch.strip_prefix("staging-next-")
    && is_version_number(version)
  {
    return vec![format!("release-{version}")];
  }

  if let Some(version) = branch.strip_prefix("release-")
    && is_version_number(version)
  {
    return vec![
      format!("nixpkgs-{version}-darwin"),
      format!("nixos-{version}-small"),
    ];
  }

  if let Some(channel) = branch
    .strip_prefix("nixos-")
    .and_then(|s| s.strip_suffix("-small"))
  {
    return vec![format!("nixos-{channel}")];
  }

  if let Some(version) = branch.strip_prefix("staging-")
    && let Some(major) = release_major(version)
  {
    let prefix = if major <= 20 {
      "release-"
    } else {
      "staging-next-"
    };
    return vec![format!("{prefix}{version}")];
  }

  Vec::new()
}

fn strings<const N: usize>(values: [&str; N]) -> Vec<String> {
  values.into_iter().map(str::to_string).collect()
}

fn release_major(version: &str) -> Option<u8> {
  if version.len() != 5 || version.as_bytes().get(2) != Some(&b'.') {
    return None;
  }

  let major = version.get(0..2)?.parse().ok()?;
  version.get(3..5)?.parse::<u8>().ok()?;
  Some(major)
}

fn is_version_number(version: &str) -> bool {
  !version.is_empty()
    && version
      .bytes()
      .all(|byte| byte == b'.' || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
  use super::{BranchPlan, next_branches};

  fn expect_next(branch: &str, expected: &[&str]) {
    let actual = next_branches(branch);
    let expected: Vec<String> = expected
      .iter()
      .map(std::string::ToString::to_string)
      .collect();
    assert_eq!(expected, actual);
  }

  #[test]
  fn branch_flow() {
    expect_next("staging", &["staging-next"]);
    expect_next("staging-next", &["master"]);
    expect_next("master", &["nixpkgs-unstable", "nixos-unstable-small"]);
    expect_next("nixos-unstable-small", &["nixos-unstable"]);
    expect_next("release-25.11", &[
      "nixpkgs-25.11-darwin",
      "nixos-25.11-small",
    ]);
    expect_next("staging-20.09", &["release-20.09"]);
    expect_next("staging-25.11", &["staging-next-25.11"]);
  }

  #[test]
  fn plan_collects_probe_targets_from_base_branch() {
    let plan = BranchPlan::from_base_branch("release-25.11");

    assert_eq!(
      &[
        "release-25.11",
        "nixpkgs-25.11-darwin",
        "nixos-25.11-small",
        "nixos-25.11",
      ],
      plan.probe_targets()
    );
  }

  #[test]
  fn plan_separates_probe_targets_from_summary_targets() {
    let plan = BranchPlan::from_base_branch("release-25.11");

    assert_eq!(
      &[
        "release-25.11",
        "nixpkgs-25.11-darwin",
        "nixos-25.11-small",
        "nixos-25.11",
      ],
      plan.probe_targets()
    );
    assert_eq!(
      &["nixpkgs-25.11-darwin", "nixos-25.11-small", "nixos-25.11"],
      plan.summary_targets()
    );
  }
}

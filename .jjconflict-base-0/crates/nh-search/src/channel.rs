use color_eyre::{Result, eyre::bail};
use tracing::warn;

// List of deprecated NixOS versions.
// Add new versions as they become deprecated.
const DEPRECATED_VERSIONS: &[&str] = &[
  "nixos-23.11",
  "nixos-24.05",
  "nixos-24.11",
  "nixos-25.05",
  "nixos-25.11",
];

/// Validates the channel, applying fallback for deprecated versions.
///
/// # Returns
///
/// The effective channel string, after substituting any deprecated alias with
/// `nixos-unstable`.
///
/// # Errors
///
/// Returns an error if `channel` (post-substitution) is not a recognized
/// branch according to [`supported_branch`].
pub fn validate(channel: &str) -> Result<String> {
  let mut channel = channel.to_string();
  if DEPRECATED_VERSIONS.contains(&channel.as_str()) {
    warn!(
      "Channel '{channel}' is deprecated or unavailable, falling back to \
       'nixos-unstable'"
    );
    channel = "nixos-unstable".to_string();
  }
  if !supported_branch(&channel) {
    bail!("Channel {channel} is not supported!");
  }
  Ok(channel)
}

fn supported_branch<S: AsRef<str>>(branch: S) -> bool {
  let branch = branch.as_ref();

  if branch == "nixos-unstable" {
    return true;
  }

  if DEPRECATED_VERSIONS.contains(&branch) {
    warn!("Channel {} is deprecated and not supported", branch);
    return false;
  }

  branch
    .strip_prefix("nixos-")
    .and_then(|version| version.split_once('.'))
    .is_some_and(|(major, minor)| {
      !major.is_empty()
        && !minor.is_empty()
        && major.bytes().all(|byte| byte.is_ascii_digit())
        && minor.bytes().all(|byte| byte.is_ascii_digit())
    })
}

#[test]
fn test_supported_branch() {
  assert!(supported_branch("nixos-unstable"));
  assert!(supported_branch("nixos-26.05"));
  assert!(!supported_branch("nixos-unstable-small"));
  assert!(!supported_branch("nixos-24.05"));
  assert!(!supported_branch("nixos-24.11"));
  assert!(!supported_branch("nixos-25.05"));
  assert!(!supported_branch("nixos-25.11"));
  assert!(!supported_branch("24.05"));
  assert!(!supported_branch("nixos-26"));
  assert!(!supported_branch("nixos-.05"));
  assert!(!supported_branch("nixos-26."));
  assert!(!supported_branch("nixos-26.05.1"));
  assert!(!supported_branch("nixpkgs-darwin"));
  assert!(!supported_branch("nixpks-21.11-darwin"));
}

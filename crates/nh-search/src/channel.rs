use std::sync::OnceLock;

use color_eyre::{Result, eyre::bail};
use regex::Regex;
use tracing::warn;

// List of deprecated NixOS versions.
// Add new versions as they become deprecated.
const DEPRECATED_VERSIONS: &[&str] =
  &["nixos-23.11", "nixos-24.05", "nixos-24.11", "nixos-25.05"];

static SUPPORTED_BRANCH_REGEX: OnceLock<Regex> = OnceLock::new();

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

  let re = SUPPORTED_BRANCH_REGEX.get_or_init(|| {
    Regex::new(r"^nixos-\d+\.\d+$").unwrap_or_else(|e| {
      warn!("invalid regex in supported_branch: {e}");
      #[allow(clippy::expect_used)]
      Regex::new("$^").expect("Regex $^ should always be valid")
    })
  });
  re.is_match(branch)
}

#[test]
fn test_supported_branch() {
  assert!(supported_branch("nixos-unstable"));
  assert!(supported_branch("nixos-25.11"));
  assert!(!supported_branch("nixos-unstable-small"));
  assert!(!supported_branch("nixos-24.05"));
  assert!(!supported_branch("nixos-24.11"));
  assert!(!supported_branch("nixos-25.05"));
  assert!(!supported_branch("24.05"));
  assert!(!supported_branch("nixpkgs-darwin"));
  assert!(!supported_branch("nixpks-21.11-darwin"));
}

//! Running `switch-to-configuration` in a transient `systemd-run` unit.

use nh_static::EnvVars;
use tracing::debug;

/// Arguments for the transient unit. Combine with [`output_args`].
pub const UNIT_ARGS: &[&str] = &[
  "-E",
  EnvVars::LOCALE_ARCHIVE,
  "-E",
  EnvVars::NIXOS_INSTALL_BOOTLOADER,
  "-E",
  EnvVars::NIXOS_NO_CHECK,
  "--collect",
  "--no-ask-password",
  "--quiet",
  "--service-type=exec",
  "--unit=nixos-rebuild-switch-to-configuration",
];

/// Output arguments for systemd 261 and newer.
///
/// `--pipe` would tie the unit's output to the caller's session, so a dropped
/// SSH connection kills activation midway (NixOS/nixpkgs#506897).
const OUTPUT_CAT_ARGS: &[&str] = &["--wait", "--verbose", "--output=cat"];

/// Output arguments for systemd older than 261, which lacks `--output`.
const PIPE_ARGS: &[&str] = &["--pipe"];

/// First systemd version whose `systemd-run` supports `--output=cat`.
const OUTPUT_CAT_MIN_VERSION: u32 = 261;

/// Parses the major version from `systemd-run --version` output.
fn parse_version(output: &str) -> Option<u32> {
  output.lines().find_map(|line| {
    let digits: String = line
      .strip_prefix("systemd ")?
      .chars()
      .take_while(char::is_ascii_digit)
      .collect();
    digits.parse().ok()
  })
}

/// Picks the output arguments from `systemd-run --version` output, falling
/// back to `--pipe` when the version is unknown or older than 261.
#[must_use]
pub fn output_args(version_output: Option<&str>) -> &'static [&'static str] {
  let version = version_output.and_then(parse_version);
  debug!(?version, "Detected systemd-run version");

  if version.is_some_and(|v| v >= OUTPUT_CAT_MIN_VERSION) {
    OUTPUT_CAT_ARGS
  } else {
    PIPE_ARGS
  }
}

use std::{path::PathBuf, sync::OnceLock};

use nh_core::command::{CommandKind, NixCommand};
use regex::Regex;
use tracing::warn;

static HYPERLINKS_SUPPORTED: OnceLock<bool> = OnceLock::new();
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub(super) fn hyperlink(text: &str, link: &str) -> String {
  let hyperlinks =
    *HYPERLINKS_SUPPORTED.get_or_init(supports_hyperlinks::supports_hyperlinks);
  let text = format!("{DIM}{text}{RESET}");

  if hyperlinks {
    format!("\x1b]8;;{link}\x1b\\{text}\x1b]8;;\x1b\\")
  } else {
    text
  }
}

pub(super) fn print_field_link(label: &str, url: &str) {
  print_field_hyperlink(label, url, url);
}

pub(super) fn print_field_hyperlink(label: &str, text: &str, link: &str) {
  print!("  {label}: ");
  println!("{}", hyperlink(text, link));
}

pub(super) fn print_wrapped(text: &str) {
  for line in textwrap::wrap(text, textwrap::Options::with_termwidth()) {
    println!("  {line}");
  }
}

pub(super) fn print_wrapped_field(label: &str, value: &str) {
  let prefix = format!("  {label}: ");
  let indent = " ".repeat(prefix.chars().count());

  for (index, line) in
    textwrap::wrap(value, textwrap::Options::with_termwidth())
      .iter()
      .enumerate()
  {
    if index == 0 {
      println!("{prefix}{line}");
    } else {
      println!("{indent}{line}");
    }
  }
}

pub(super) fn strip_html(html: &str) -> String {
  static HTML_TAG: OnceLock<Regex> = OnceLock::new();
  let re = HTML_TAG.get_or_init(|| {
    Regex::new(r"<[^>]*>").unwrap_or_else(|err| {
      warn!("invalid HTML strip regex: {err}");
      #[allow(clippy::expect_used)]
      Regex::new("$^").expect("Regex $^ should always be valid")
    })
  });
  re.replace_all(html, "").trim().to_string()
}

/// Resolve the ambient nixpkgs lookup path without fetching a mutable channel.
///
/// This path only backs the local `file://` link. The channel-specific source
/// link is rendered separately, so failure here should not block search output.
pub(super) fn resolve_nixpkgs_path() -> Option<PathBuf> {
  let output = nixpkgs_path_command().output().ok()?;
  if !output.status.success() {
    return None;
  }

  let path = std::str::from_utf8(&output.stdout).ok()?.trim();
  if path.is_empty() {
    None
  } else {
    Some(PathBuf::from(path))
  }
}

fn nixpkgs_path_command() -> NixCommand {
  NixCommand::new(CommandKind::Eval).impure(true).args([
    "--offline",
    "--raw",
    "--expr",
    "toString <nixpkgs>",
  ])
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn nixpkgs_path_lookup_is_local_and_offline() {
    let argv = nixpkgs_path_command().argv();

    assert_eq!(argv, [
      "nix",
      "eval",
      "--impure",
      "--offline",
      "--raw",
      "--expr",
      "toString <nixpkgs>"
    ]);
    assert!(
      !argv
        .iter()
        .any(|arg| arg.to_string_lossy().contains("github:"))
    );
  }
}

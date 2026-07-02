use std::sync::OnceLock;

use nh_core::command::{CommandKind, NixCommand};
use regex::Regex;
use subprocess::Redirection;
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

pub(super) fn resolve_nixpkgs_path(channel: &str) -> String {
  let flake_ref = if channel == "nixos-unstable" {
    "github:NixOS/nixpkgs/nixos-unstable".to_string()
  } else if channel.starts_with("nixos-") {
    format!("github:NixOS/nixpkgs/{channel}")
  } else {
    "nixpkgs".to_string()
  };

  let flake_path = format!("{flake_ref}#path");
  capture_nix_eval(&["--raw", &flake_path])
    .or_else(|| capture_nix_eval(&["-f", "<nixpkgs>", "path"]))
    .unwrap_or_default()
}

fn capture_nix_eval(args: &[&str]) -> Option<String> {
  let capture = NixCommand::new(CommandKind::Eval)
    .args(args)
    .to_exec()
    .stderr(Redirection::None)
    .stdout(Redirection::Pipe)
    .capture()
    .ok()?;

  capture
    .exit_status
    .success()
    .then(|| capture.stdout_str().trim().to_string())
    .filter(|path| !path.is_empty())
}

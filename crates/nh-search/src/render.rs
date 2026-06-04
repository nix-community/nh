use std::sync::OnceLock;

use regex::Regex;
use subprocess::{Exec, Redirection};
use tracing::{debug, trace, warn};
use yansi::{Color, Paint};

use crate::types::{OptionSearchResult, PackageSearchResult};

static HYPERLINKS_SUPPORTED: OnceLock<bool> = OnceLock::new();

/// Prints an underlined link in the terminal, where the visible text may be
/// different from the link - or just print the text if hyperlinks aren't
/// supported
fn print_hyperlink(text: &str, link: &str) {
  let hyperlinks =
    *HYPERLINKS_SUPPORTED.get_or_init(supports_hyperlinks::supports_hyperlinks);

  if hyperlinks {
    print!("\x1b]8;;{link}\x07");
    print!("{}", Paint::new(text).underline());
    println!("\x1b]8;;\x07");
  } else {
    println!("{text}");
  }
}

/// Strips HTML tags from a rendered-html description string
fn strip_html(html: &str) -> String {
  static HTML_TAG: OnceLock<Regex> = OnceLock::new();
  let re = HTML_TAG.get_or_init(|| {
    Regex::new(r"<[^>]*>").unwrap_or_else(|e| {
      warn!("invalid HTML strip regex: {e}");
      #[allow(clippy::expect_used)]
      Regex::new("$^").expect("Regex $^ should always be valid")
    })
  });
  re.replace_all(html, "").trim().to_string()
}

fn capture_nix_path(args: &[&str]) -> Option<String> {
  let capture = Exec::cmd("nix")
    .args(args)
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

fn resolve_nixpkgs_path(channel: &str) -> String {
  let flake_ref = if channel == "nixos-unstable" {
    "github:NixOS/nixpkgs/nixos-unstable".to_string()
  } else if channel.starts_with("nixos-") {
    format!("github:NixOS/nixpkgs/{channel}")
  } else {
    "nixpkgs".to_string()
  };

  let flake_path = format!("{flake_ref}#path");
  capture_nix_path(&["eval", "--raw", &flake_path])
    .or_else(|| capture_nix_path(&["eval", "-f", "<nixpkgs>", "path"]))
    .unwrap_or_default()
}

pub fn print_package_results(
  channel: &str,
  platforms: bool,
  documents: &[PackageSearchResult],
) {
  let nixpkgs_path = resolve_nixpkgs_path(channel);
  debug!("nixpkgs_path: {:?}", nixpkgs_path);

  for elem in documents.iter().rev() {
    println!();
    trace!("{elem:#?}");

    print!("{}", Paint::new(&elem.package_attr_name).fg(Color::Blue));
    let v = &elem.package_pversion;
    if !v.is_empty() {
      print!(" ({})", Paint::new(v).fg(Color::Green));
    }

    println!();

    if let Some(ref desc) = elem.package_description {
      let desc = desc.replace('\n', " ");
      for line in textwrap::wrap(&desc, textwrap::Options::with_termwidth()) {
        println!("  {line}");
      }
    }

    for url in &elem.package_homepage {
      print!("  Homepage: ");
      print_hyperlink(url, url);
    }

    if platforms && !elem.package_platforms.is_empty() {
      println!("  Platforms: {}", elem.package_platforms.join(", "));
    }

    if let Some(package_position) = &elem.package_position {
      match package_position.split(':').next() {
        Some(position) => {
          if !nixpkgs_path.is_empty() {
            print!("  Defined at: ");
            print_hyperlink(
              position,
              &format!("file://{nixpkgs_path}/{position}"),
            );
          }

          let github_nixpkgs_url =
            format!("https://github.com/NixOS/nixpkgs/blob/{channel}");

          print!("  GitHub link: ");
          let url = format!("{github_nixpkgs_url}/{position}");
          print_hyperlink(&url, &url);
        },
        None => {
          warn!(
            "Position should have at least one part; received \
             {package_position}"
          );
        },
      }
    }
  }
}

pub fn print_option_results(channel: &str, documents: &[OptionSearchResult]) {
  let nixpkgs_path = resolve_nixpkgs_path(channel);
  debug!("nixpkgs_path: {:?}", nixpkgs_path);

  for elem in documents.iter().rev() {
    println!();
    trace!("{elem:#?}");

    print!("{}", Paint::new(&elem.option_name).fg(Color::Blue));

    if let Some(ref ot) = elem.option_type {
      print!(" :: {}", Paint::new(ot).fg(Color::Green));
    }

    if let Some(ref oe) = elem.option_example {
      print!(" (example: {})", Paint::new(oe).fg(Color::Yellow));
    }

    println!();
    println!("  Scope: {}", elem.r#type);

    if let Some(ref desc) = elem.option_description {
      let desc = strip_html(desc);
      let desc = desc.replace('\n', " ");
      for line in textwrap::wrap(&desc, textwrap::Options::with_termwidth()) {
        println!("  {line}");
      }
    }

    if let Some(ref default) = elem.option_default {
      let prefix = "  Default: ";
      for (i, line) in
        textwrap::wrap(default, textwrap::Options::with_termwidth())
          .iter()
          .enumerate()
      {
        if i == 0 {
          println!("{prefix}{line}");
        } else {
          println!("           {line}");
        }
      }
    }

    if let Some(ref source) = elem.option_source {
      let is_hm = elem.r#type == "home-manager-option";
      let filepath = source.split(':').next().unwrap_or(source);

      if !is_hm && !nixpkgs_path.is_empty() {
        print!("  Defined at: ");
        print_hyperlink(filepath, &format!("file://{nixpkgs_path}/{filepath}"));
      }

      print!("  Source: ");
      if is_hm {
        let url = format!(
          "https://github.com/nix-community/home-manager/blob/master/{filepath}"
        );
        print_hyperlink(&url, &url);
      } else {
        let url =
          format!("https://github.com/NixOS/nixpkgs/blob/{channel}/{filepath}");
        print_hyperlink(&url, &url);
      }
    }
  }
}

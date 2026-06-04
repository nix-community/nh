use tracing::{debug, trace, warn};
use yansi::{Color, Paint};

use super::common;
use crate::types::PackageSearchResult;

pub fn print(
  channel: &str,
  platforms: bool,
  documents: &[PackageSearchResult],
) {
  let nixpkgs_path = common::resolve_nixpkgs_path(channel);
  debug!("nixpkgs_path: {:?}", nixpkgs_path);

  for elem in documents.iter().rev() {
    println!();
    trace!("{elem:#?}");

    print!("{}", Paint::new(&elem.package_attr_name).fg(Color::Blue));
    let version = &elem.package_pversion;
    if !version.is_empty() {
      print!(" ({})", Paint::new(version).fg(Color::Green));
    }

    println!();

    if let Some(description) = &elem.package_description {
      common::print_wrapped(&description.replace('\n', " "));
    }

    for url in &elem.package_homepage {
      common::print_field_link("Homepage", url);
    }

    if platforms && !elem.package_platforms.is_empty() {
      println!("  Platforms: {}", elem.package_platforms.join(", "));
    }

    if let Some(package_position) = &elem.package_position {
      match package_position.split(':').next() {
        Some(position) => {
          if !nixpkgs_path.is_empty() {
            common::print_field_hyperlink(
              "Defined at",
              position,
              &format!("file://{nixpkgs_path}/{position}"),
            );
          }

          let github_nixpkgs_url =
            format!("https://github.com/NixOS/nixpkgs/blob/{channel}");
          let url = format!("{github_nixpkgs_url}/{position}");
          common::print_field_link("GitHub link", &url);
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

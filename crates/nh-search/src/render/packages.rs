use tracing::{debug, trace, warn};
use yansi::{Color, Paint};

use super::common;
use crate::{args::PackageField, types::PackageSearchResult};

pub fn print(
  channel: &str,
  platforms: bool,
  fields: Option<&[PackageField]>,
  documents: &[PackageSearchResult],
) {
  let nixpkgs_path = common::resolve_nixpkgs_path();
  debug!("nixpkgs_path: {:?}", nixpkgs_path);

  let default_fields = default_fields(platforms);
  let fields = fields.unwrap_or(&default_fields);

  for elem in documents.iter().rev() {
    println!();
    trace!("{elem:#?}");

    for field in fields {
      match field {
        PackageField::AttrName => {
          print!("{}", Paint::new(&elem.package_attr_name).fg(Color::Blue));
          let version = &elem.package_pversion;
          if !version.is_empty() {
            print!(" ({})", Paint::new(version).fg(Color::Green));
          }
          println!();
        },
        PackageField::Description => {
          if let Some(description) = &elem.package_description {
            common::print_wrapped(&description.replace('\n', " "));
          }
        },
        PackageField::MainProgram => {
          if let Some(main_program) = &elem.package_mainProgram {
            common::print_wrapped_field("Main program", main_program);
          }
        },
        PackageField::Homepage => {
          for url in &elem.package_homepage {
            common::print_field_link("Homepage", url);
          }
        },
        PackageField::Platforms => {
          if !elem.package_platforms.is_empty() {
            println!("  Platforms: {}", elem.package_platforms.join(", "));
          }
        },
        PackageField::Position => {
          print_position(
            &nixpkgs_path,
            elem.package_position.as_deref(),
            channel,
            false,
          );
        },
        PackageField::GithubLink => {
          print_position(
            &nixpkgs_path,
            elem.package_position.as_deref(),
            channel,
            true,
          );
        },
        PackageField::Programs => {
          if !elem.package_programs.is_empty() {
            common::print_wrapped_field(
              "Package programs",
              &elem.package_programs.join(" "),
            );
          }
        },
        PackageField::LicenseSet => {
          if !elem.package_license_set.is_empty() {
            common::print_wrapped_field(
              "License",
              &elem.package_license_set.join(", "),
            );
          }
        },
      }
    }
  }
}

fn default_fields(platforms: bool) -> Vec<PackageField> {
  let mut fields = vec![
    PackageField::AttrName,
    PackageField::Description,
    PackageField::MainProgram,
    PackageField::Homepage,
  ];

  if platforms {
    fields.push(PackageField::Platforms);
  }

  fields.extend([PackageField::Position, PackageField::GithubLink]);
  fields
}

fn print_position(
  nixpkgs_path: &Option<std::path::PathBuf>,
  package_position: Option<&str>,
  channel: &str,
  github_link: bool,
) {
  let Some(package_position) = package_position else {
    return;
  };

  match package_position.split(':').next() {
    Some(position) => {
      if github_link {
        let github_nixpkgs_url =
          format!("https://github.com/NixOS/nixpkgs/blob/{channel}");
        let url = format!("{github_nixpkgs_url}/{position}");
        common::print_field_link("GitHub link", &url);
      } else if let Some(nixpkgs_path) = nixpkgs_path {
        common::print_field_hyperlink(
          "Defined at",
          position,
          &format!("file://{}/{position}", nixpkgs_path.display()),
        );
      }
    },
    None => {
      warn!(
        "Position should have at least one part; received {package_position}"
      );
    },
  }
}

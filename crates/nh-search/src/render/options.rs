use tracing::{debug, trace};
use yansi::{Color, Paint};

use super::common;
use crate::types::OptionSearchResult;

pub fn print(channel: &str, documents: &[OptionSearchResult]) {
  let nixpkgs_path = common::resolve_nixpkgs_path(channel);
  debug!("nixpkgs_path: {:?}", nixpkgs_path);

  for elem in documents.iter().rev() {
    println!();
    trace!("{elem:#?}");

    print!("{}", Paint::new(&elem.option_name).fg(Color::Blue));

    if let Some(option_type) = &elem.option_type {
      print!(" :: {}", Paint::new(option_type).fg(Color::Green));
    }

    if let Some(example) = &elem.option_example {
      print!(" (example: {})", Paint::new(example).fg(Color::Yellow));
    }

    println!();
    println!("  Scope: {}", elem.r#type);

    if let Some(description) = &elem.option_description {
      let description = common::strip_html(description);
      common::print_wrapped(&description.replace('\n', " "));
    }

    if let Some(default) = &elem.option_default {
      common::print_wrapped_field("Default", default);
    }

    if let Some(source) = &elem.option_source {
      let is_home_manager = elem.r#type == "home-manager-option";
      let filepath = source.split(':').next().unwrap_or(source);

      if !is_home_manager && !nixpkgs_path.is_empty() {
        common::print_field_hyperlink(
          "Defined at",
          filepath,
          &format!("file://{nixpkgs_path}/{filepath}"),
        );
      }

      if is_home_manager {
        let url = format!(
          "https://github.com/nix-community/home-manager/blob/master/{filepath}"
        );
        common::print_field_link("Source", &url);
      } else {
        let url =
          format!("https://github.com/NixOS/nixpkgs/blob/{channel}/{filepath}");
        common::print_field_link("Source", &url);
      }
    }
  }
}

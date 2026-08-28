use elasticsearch_dsl::{Operator, Query, Search, TextQueryType};

use crate::args;

const TYPE_OPTION: &str = "option";
const TYPE_SERVICE: &str = "service";
const TYPE_HOME_MANAGER_OPTION: &str = "home-manager-option";
const TYPE_DARWIN_OPTION: &str = "darwin-option";

const NIXPKGS_SCOPE_TYPES: &[&str] = &[TYPE_OPTION, TYPE_SERVICE];
const HOME_MANAGER_SCOPE_TYPES: &[&str] = &[TYPE_HOME_MANAGER_OPTION];
const DARWIN_SCOPE_TYPES: &[&str] = &[TYPE_DARWIN_OPTION];
const ALL_SCOPE_TYPES: &[&str] = &[
  TYPE_OPTION,
  TYPE_SERVICE,
  TYPE_HOME_MANAGER_OPTION,
  TYPE_DARWIN_OPTION,
];

pub fn packages(query: &str, limit: u64) -> Search {
  Search::new().from(0).size(limit).query(
    Query::bool().filter(Query::term("type", "package")).must(
      Query::dis_max()
        .tie_breaker(0.7)
        .query(
          Query::multi_match(
            [
              "package_attr_name^9",
              "package_attr_name.*^5.3999999999999995",
              "package_programs^9",
              "package_programs.*^5.3999999999999995",
              "package_pname^6",
              "package_pname.*^3.5999999999999996",
              "package_description^1.3",
              "package_description.*^0.78",
              "package_longDescription^1",
              "package_longDescription.*^0.6",
              "flake_name^0.5",
              "flake_name.*^0.3",
            ],
            query.to_string(),
          )
          .r#type(TextQueryType::CrossFields)
          .analyzer("whitespace")
          .auto_generate_synonyms_phrase_query(false)
          .operator(Operator::And),
        )
        .query(
          Query::wildcard("package_attr_name", format!("*{query}*"))
            .case_insensitive(true),
        ),
    ),
  )
}

pub fn options(scope: args::OptionScope, query: &str, limit: u64) -> Search {
  Search::new().from(0).size(limit).query(
    Query::bool()
      .filter(Query::terms("type", option_scope_types(scope)))
      .must(
        Query::dis_max()
          .tie_breaker(0.7)
          .query(
            Query::multi_match(
              [
                "option_name^6",
                "option_name.*^3.6",
                "option_name_query^6",
                "option_name_query.*^3.6",
                "option_description^1",
                "option_description.*^0.6",
                "flake_name^0.5",
                "flake_name.*^0.3",
                "service_package^3",
                "service_package.*^1.8",
                "service_packages^3",
                "service_packages.*^1.8",
              ],
              query.to_string(),
            )
            .r#type(TextQueryType::CrossFields)
            .analyzer("whitespace")
            .auto_generate_synonyms_phrase_query(false)
            .operator(Operator::And),
          )
          .query(
            Query::wildcard("option_name", format!("*{query}*"))
              .case_insensitive(true),
          ),
      ),
  )
}

pub const fn option_scope_label(scope: args::OptionScope) -> &'static str {
  match scope {
    args::OptionScope::Nixpkgs => "nixpkgs",
    args::OptionScope::HomeManager => "home-manager",
    args::OptionScope::Darwin => "nix-darwin",
    args::OptionScope::All => "all",
  }
}

/// Returns the ES document type strings for a given option scope.
const fn option_scope_types(
  scope: args::OptionScope,
) -> &'static [&'static str] {
  match scope {
    args::OptionScope::Nixpkgs => NIXPKGS_SCOPE_TYPES,
    args::OptionScope::HomeManager => HOME_MANAGER_SCOPE_TYPES,
    args::OptionScope::Darwin => DARWIN_SCOPE_TYPES,
    args::OptionScope::All => ALL_SCOPE_TYPES,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn darwin_scope_maps_to_darwin_option_type() {
    assert_eq!(option_scope_types(args::OptionScope::Darwin), &[
      TYPE_DARWIN_OPTION
    ]);
    assert_eq!(option_scope_label(args::OptionScope::Darwin), "nix-darwin");
  }

  #[test]
  fn all_scope_includes_darwin_option_type() {
    assert!(
      option_scope_types(args::OptionScope::All).contains(&TYPE_DARWIN_OPTION)
    );
  }
}

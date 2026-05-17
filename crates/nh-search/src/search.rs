pub mod args;

use std::{
  sync::OnceLock,
  time::{Duration, Instant},
};

use color_eyre::{
  Result,
  eyre::{Context, bail, eyre},
};
use elasticsearch_dsl::{
  Operator,
  Query,
  Search,
  SearchResponse,
  TextQueryType,
};
use regex::Regex;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use subprocess::{Exec, Redirection};
use tracing::{debug, trace, warn};
use yansi::{Color, Paint};

const NH_VERSION: &str = env!("CARGO_PKG_VERSION");

// List of deprecated NixOS versions
// Add new versions as they become deprecated.
const DEPRECATED_VERSIONS: &[&str] =
  &["nixos-23.11", "nixos-24.05", "nixos-24.11", "nixos-25.05"];

// Support for current version pattern
static SUPPORTED_BRANCH_REGEX: OnceLock<Regex> = OnceLock::new();

#[derive(Debug, Deserialize, Serialize)]
#[allow(non_snake_case, dead_code, clippy::struct_field_names)]
struct SearchResult {
  // r#type: String,
  package_attr_name:       String,
  package_attr_set:        String,
  package_pname:           String,
  package_pversion:        String,
  package_platforms:       Vec<String>,
  package_outputs:         Vec<String>,
  package_default_output:  Option<String>,
  package_programs:        Vec<String>,
  // package_license: Vec<License>,
  package_license_set:     Vec<String>,
  // package_maintainers: Vec<HashMap<String, String>>,
  package_description:     Option<String>,
  package_longDescription: Option<String>,
  package_hydra:           (),
  package_system:          String,
  package_homepage:        Vec<String>,
  package_position:        Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(non_snake_case, dead_code)]
struct OptionSearchResult {
  r#type:             String,
  option_name:        String,
  option_description: Option<String>,
  option_type:        Option<String>,
  option_default:     Option<String>,
  option_example:     Option<String>,
  option_source:      Option<String>,
  option_flake:       Option<Vec<String>>,
  flake_name:         Option<String>,
  flake_description:  Option<String>,
}

// Cache the hyperlink support check result
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

#[derive(Debug, Serialize)]
struct JSONOutput {
  query:      String,
  channel:    String,
  elapsed_ms: u128,
  results:    Vec<SearchResult>,
}

#[derive(Debug, Serialize)]
struct JSONOutputOptions {
  query:      String,
  channel:    String,
  scope:      String,
  elapsed_ms: u128,
  results:    Vec<OptionSearchResult>,
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

/// Returns the ES document type strings for a given option scope
const fn option_scope_types(
  scope: &args::OptionScope,
) -> &'static [&'static str] {
  match scope {
    args::OptionScope::Nixpkgs => &["option", "service"],
    args::OptionScope::HomeManager => &["home-manager-option"],
    args::OptionScope::All => &["option", "service", "home-manager-option"],
  }
}

const fn option_scope_label(scope: &args::OptionScope) -> &'static str {
  match scope {
    args::OptionScope::Nixpkgs => "nixpkgs",
    args::OptionScope::HomeManager => "home-manager",
    args::OptionScope::All => "all",
  }
}

fn search_documents<T>(
  query: &Search,
  channel: &str,
  build_context: &'static str,
  execute_context: &'static str,
  parse_context: &'static str,
) -> Result<(Vec<T>, Duration)>
where
  T: DeserializeOwned,
{
  let then = Instant::now();
  let client = reqwest::blocking::Client::new();
  let req = client
    // NOTE: when the version of the backend API changes,
    // this file and the corresponding workflow called
    // nixos-search.yaml have to be updated accordingly.
    .post(format!(
      "https://search.nixos.org/backend/latest-48-{channel}/_search"
    ))
    .json(query)
    .header("User-Agent", format!("nh/{NH_VERSION}"))
    // Hardcoded upstream
    // https://github.com/NixOS/nixos-search/blob/744ec58e082a3fcdd741b2c9b0654a0f7fda4603/frontend/src/index.js
    .basic_auth("aWVSALXpZv", Some("X8gPHnzL52wFEekuxsfQ9cSh"))
    .build()
    .context(build_context)?;

  debug!(?req);

  let response = client.execute(req).context(execute_context)?;
  let elapsed = then.elapsed();
  debug!(?elapsed);
  trace!(?response);

  if !response.status().is_success() {
    eprintln!(
      "Error: search.nixos.org returned HTTP {} for channel '{channel}'. This \
       usually means the channel does not exist, is not indexed, or the \
       request was malformed.",
      response.status(),
    );
    return Err(eyre!(
      "search.nixos.org returned HTTP {} for channel '{channel}'",
      response.status(),
    ));
  }

  let parsed_response: SearchResponse = response
    .json()
    .context("parsing response into the elasticsearch format")?;
  trace!(?parsed_response);

  let documents = parsed_response.documents::<T>().context(parse_context)?;
  Ok((documents, elapsed))
}

impl args::SearchArgs {
  #[allow(clippy::missing_errors_doc)]
  pub fn run(&self) -> Result<()> {
    trace!("args: {self:?}");

    let mut channel = self.channel.clone();
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

    match &self.options {
      Some(scope) => self.run_options(&channel, scope),
      None => self.run_packages(&channel),
    }
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

    if let Some(path) =
      Self::capture_nix_path(&["eval", "-f", "<nixpkgs>", "path"])
    {
      return path;
    }

    let flake_path = format!("{flake_ref}#path");
    Self::capture_nix_path(&["eval", "--raw", &flake_path]).unwrap_or_default()
  }

  fn run_packages(&self, channel: &str) -> Result<()> {
    let query_s = self.query.join(" ");
    debug!(?query_s);

    let query = Search::new().from(0).size(self.limit).query(
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
              query_s.clone(),
            )
            .r#type(TextQueryType::CrossFields)
            .analyzer("whitespace")
            .auto_generate_synonyms_phrase_query(false)
            .operator(Operator::And),
          )
          .query(
            Query::wildcard("package_attr_name", format!("*{}*", &query_s))
              .case_insensitive(true),
          ),
      ),
    );

    if !self.json {
      println!("Querying search.nixos.org, with channel {channel}...");
    }
    let (documents, elapsed) = search_documents::<SearchResult>(
      &query,
      channel,
      "building search query",
      "querying the elasticsearch API",
      "parsing search document",
    )?;

    if !self.json {
      println!("Took {}ms", elapsed.as_millis());
      println!("Most relevant results at the end");
      println!();
    }

    if self.json {
      // Output as JSON
      let json_output = JSONOutput {
        query:      query_s,
        channel:    channel.to_string(),
        elapsed_ms: elapsed.as_millis(),
        results:    documents,
      };

      println!("{}", serde_json::to_string_pretty(&json_output)?);
      return Ok(());
    }

    let nixpkgs_path = Self::resolve_nixpkgs_path(channel);

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

      if self.platforms && !elem.package_platforms.is_empty() {
        println!("  Platforms: {}", elem.package_platforms.join(", "));
      }

      if let Some(package_position) = &elem.package_position {
        match package_position.split(':').next() {
          Some(position) => {
            // Position from search.nixos.org is already a relative path
            // like "pkgs/by-name/..."
            if !nixpkgs_path.is_empty() {
              print!("  Defined at: ");
              print_hyperlink(
                position,
                &format!("file://{nixpkgs_path}/{position}"),
              );

              let github_nixpkgs_url =
                format!("https://github.com/NixOS/nixpkgs/blob/{channel}");

              print!("  GitHub link: ");
              let url = format!("{github_nixpkgs_url}/{position}");
              print_hyperlink(&url, &url);
            }
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

    Ok(())
  }

  fn run_options(
    &self,
    channel: &str,
    scope: &args::OptionScope,
  ) -> Result<()> {
    let query_s = self.query.join(" ");
    debug!(?query_s, ?scope);

    let option_types = option_scope_types(scope);

    let query = Search::new().from(0).size(self.limit).query(
      Query::bool()
        .filter(Query::terms("type", option_types))
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
                query_s.clone(),
              )
              .r#type(TextQueryType::CrossFields)
              .analyzer("whitespace")
              .auto_generate_synonyms_phrase_query(false)
              .operator(Operator::And),
            )
            .query(
              Query::wildcard("option_name", format!("*{}*", &query_s))
                .case_insensitive(true),
            ),
        ),
    );

    if !self.json {
      println!(
        "Querying options on search.nixos.org, with channel {channel}..."
      );
    }
    let (documents, elapsed) = search_documents::<OptionSearchResult>(
      &query,
      channel,
      "building option search query",
      "querying the elasticsearch API for options",
      "parsing option search document",
    )?;

    if !self.json {
      println!("Took {}ms", elapsed.as_millis());
      println!("Most relevant results at the end");
      println!();
    }

    if self.json {
      let json_output = JSONOutputOptions {
        query:      query_s,
        channel:    channel.to_string(),
        scope:      option_scope_label(scope).to_string(),
        elapsed_ms: elapsed.as_millis(),
        results:    documents,
      };

      println!("{}", serde_json::to_string_pretty(&json_output)?);
      return Ok(());
    }

    let nixpkgs_path = Self::resolve_nixpkgs_path(channel);
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
          print_hyperlink(
            filepath,
            &format!("file://{nixpkgs_path}/{filepath}"),
          );
        }

        print!("  Source: ");
        if is_hm {
          let url = format!(
            "https://github.com/nix-community/home-manager/blob/master/\
             {filepath}"
          );
          print_hyperlink(&url, &url);
        } else {
          let url = format!(
            "https://github.com/NixOS/nixpkgs/blob/{channel}/{filepath}"
          );
          print_hyperlink(&url, &url);
        }
      }
    }

    Ok(())
  }
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

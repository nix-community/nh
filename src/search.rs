use std::{process::Stdio, sync::OnceLock, time::Instant};

use color_eyre::eyre::{Context, bail};
use elasticsearch_dsl::{
  Operator,
  Query,
  Search,
  SearchResponse,
  TextQueryType,
};
use interface::SearchArgs;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace, warn};
use yansi::{Color, Paint};

use crate::{Result, interface};

// List of deprecated NixOS versions
// Add new versions as they become deprecated.
const DEPRECATED_VERSIONS: &[&str] =
  &["nixos-23.11", "nixos-24.05", "nixos-24.11"];

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

impl SearchArgs {
  #[cfg_attr(feature = "hotpath", hotpath::measure)]
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

    let flake_ref = if channel == "nixos-unstable" {
      "github:NixOS/nixpkgs/nixos-unstable".to_string()
    } else if channel.starts_with("nixos-") {
      format!("github:NixOS/nixpkgs/{channel}")
    } else {
      "nixpkgs".to_string() // fallback to registry default
    };

    let nixpkgs_path = std::thread::spawn(move || {
      if let Ok(output) = std::process::Command::new("nix")
        .stderr(Stdio::null())
        .args(["eval", "-f", "<nixpkgs>", "path"])
        .output()
      {
        if output.status.success() {
          return Ok(output);
        }
      }

      std::process::Command::new("nix")
        .stderr(Stdio::null())
        .args(["eval", "--raw", &format!("{flake_ref}#path")])
        .output()
    });

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
      println!(
        "Querying search.nixos.org, with channel {}...",
        self.channel
      );
    }
    let then = Instant::now();

    let client = reqwest::blocking::Client::new();
    let req = client
            // NOTE: when the version of the backend API changes,
            // this file and the corresponding workflow called
            // nixos-search.yaml have to be updated accordingly.
            .post(format!(
                "https://search.nixos.org/backend/latest-44-{channel}/_search"
            ))
            .json(&query)
            .header("User-Agent", format!("nh/{}", crate::NH_VERSION))
            // Hardcoded upstream
            // https://github.com/NixOS/nixos-search/blob/744ec58e082a3fcdd741b2c9b0654a0f7fda4603/frontend/src/index.js
            .basic_auth("aWVSALXpZv", Some("X8gPHnzL52wFEekuxsfQ9cSh"))
            .build()
            .context("building search query")?;

    debug!(?req);

    #[cfg(feature = "hotpath")]
    let response = hotpath::measure_block!("http_request", {
      client
        .execute(req)
        .context("querying the elasticsearch API")?
    });
    #[cfg(not(feature = "hotpath"))]
    let response = client
      .execute(req)
      .context("querying the elasticsearch API")?;
    let elapsed = then.elapsed();
    debug!(?elapsed);
    trace!(?response);

    if !response.status().is_success() {
      eprintln!(
        "Error: search.nixos.org returned HTTP {} for channel '{}'. This \
         usually means the channel does not exist, is not indexed, or the \
         request was malformed.",
        response.status(),
        self.channel
      );
      return Err(color_eyre::eyre::eyre!(
        "search.nixos.org returned HTTP {} for channel '{}'",
        response.status(),
        self.channel
      ));
    }

    if !self.json {
      println!("Took {}ms", elapsed.as_millis());
      println!("Most relevant results at the end");
      println!();
    }

    #[cfg(feature = "hotpath")]
    let parsed_response: SearchResponse =
      hotpath::measure_block!("json_parse", {
        response
          .json()
          .context("parsing response into the elasticsearch format")?
      });
    #[cfg(not(feature = "hotpath"))]
    let parsed_response: SearchResponse = response
      .json()
      .context("parsing response into the elasticsearch format")?;
    trace!(?parsed_response);

    let documents = parsed_response
      .documents::<SearchResult>()
      .context("parsing search document")?;

    if self.json {
      // Output as JSON
      let json_output = JSONOutput {
        query: query_s,
        channel,
        elapsed_ms: elapsed.as_millis(),
        results: documents,
      };

      println!("{}", serde_json::to_string_pretty(&json_output)?);
      return Ok(());
    }

    let nixpkgs_path_output = nixpkgs_path.join().map_err(|e| {
      color_eyre::eyre::eyre!("nixpkgs_path thread panicked: {e:?}")
    })?;
    debug!("{nixpkgs_path_output:?}");

    let nixpkgs_path = nixpkgs_path_output
      .ok()
      .and_then(|output| {
        if output.status.success() {
          String::from_utf8(output.stdout).ok()
        } else {
          None
        }
      })
      .map(|s| s.trim().to_string())
      .unwrap_or_default();

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
  assert!(supported_branch("nixos-25.05"));
  assert!(!supported_branch("nixos-unstable-small"));
  assert!(!supported_branch("nixos-24.05"));
  assert!(!supported_branch("nixos-24.11"));
  assert!(!supported_branch("24.05"));
  assert!(!supported_branch("nixpkgs-darwin"));
  assert!(!supported_branch("nixpks-21.11-darwin"));
}

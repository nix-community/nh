use std::{
  collections::HashMap,
  time::{Duration, Instant},
};

use color_eyre::{
  Result,
  eyre::{Context, bail},
};
use elasticsearch_dsl::{Search, SearchResponse};
use reqwest::{
  StatusCode,
  blocking::{Client, Response},
};
use serde::{Deserialize, de::DeserializeOwned};
use tracing::{debug, trace, warn};

const NH_VERSION: &str = env!("CARGO_PKG_VERSION");

// Hardcoded upstream
// https://github.com/NixOS/nixos-search/blob/744ec58e082a3fcdd741b2c9b0654a0f7fda4603/frontend/src/index.js
const BACKEND_USER: &str = "aWVSALXpZv";
const BACKEND_PASSWORD: &str = "X8gPHnzL52wFEekuxsfQ9cSh";

/// Backend index version bundled with nh, used when the user does not override
/// it via [`BackendConfig::version`] and the newest version cannot be
/// discovered.
pub const BUNDLED_BACKEND_VERSION: &str = include_str!("../BACKEND_VERSION");

#[derive(Clone, Copy)]
pub struct SearchContexts {
  pub build:   &'static str,
  pub execute: &'static str,
  pub parse:   &'static str,
}

/// Backend index version selection for a search request.
#[derive(Clone, Copy)]
pub struct BackendConfig {
  /// Index version to try first. `None` discovers the newest version, falling
  /// back to [`BUNDLED_BACKEND_VERSION`].
  pub version:   Option<u32>,
  /// Number of newer versions to try when the requested one is outdated.
  pub fallbacks: u32,
}

/// Outcome of a single request to a specific backend index version.
enum BackendResponse {
  Found(Response),
  /// The index does not exist, so the requested version is outdated.
  Outdated,
}

pub fn search_documents<T>(
  query: &Search,
  channel: &str,
  contexts: SearchContexts,
  config: BackendConfig,
) -> Result<(Vec<T>, Duration)>
where
  T: DeserializeOwned,
{
  let client = reqwest::blocking::Client::new();

  let start = match config.version {
    Some(version) => version,
    None => {
      match discover_latest_version(&client, channel) {
        Ok(Some(version)) => version,
        Ok(None) => bundled_backend_version()?,
        Err(err) => {
          debug!(
            ?err,
            "backend index discovery failed, using the bundled version"
          );
          bundled_backend_version()?
        },
      }
    },
  };
  let last = start.saturating_add(config.fallbacks);
  let then = Instant::now();

  // The requested index version tracks search.nixos.org but can fall behind
  // between releases. A missing index answers with 404, so when a version is
  // outdated we retry against successively newer versions, up to `fallbacks`
  // times, before giving up.
  let mut version = start;
  let response = loop {
    match query_backend(&client, query, channel, version, contexts)? {
      BackendResponse::Found(response) => break response,
      BackendResponse::Outdated => {
        if version >= last {
          if start == last {
            bail!(
              "search.nixos.org has no index for channel '{channel}' at \
               backend version {start}. The channel may not exist, or the \
               version may be wrong."
            );
          }
          bail!(
            "search.nixos.org has no index for channel '{channel}' at backend \
             versions {start} through {last}. The channel may not exist, or \
             nh may be too old to query it."
          );
        }
        let next = version + 1;
        warn!(
          "Backend index version {version} is outdated, retrying with {next}. \
           Consider updating nh."
        );
        version = next;
      },
    }
  };

  let elapsed = then.elapsed();
  debug!(?elapsed);
  trace!(?response);

  let parsed_response: SearchResponse = response
    .json()
    .context("parsing response into the elasticsearch format")?;
  trace!(?parsed_response);

  let documents = parsed_response.documents::<T>().context(contexts.parse)?;
  Ok((documents, elapsed))
}

fn bundled_backend_version() -> Result<u32> {
  BUNDLED_BACKEND_VERSION
    .trim()
    .parse()
    .context("parsing the bundled backend index version")
}

/// Finds the newest backend index version available for `channel`.
///
/// Every index is exposed through a `latest-{version}-{channel}` alias, and
/// outdated indices stay online after search.nixos.org moves on. We therefore
/// need to get the newest channel using a `*`-wildcard.
///
/// # Returns
///
/// `None` when no alias matches the channel.
fn discover_latest_version(
  client: &Client,
  channel: &str,
) -> Result<Option<u32>> {
  let response = client
    .get(format!(
      "https://search.nixos.org/backend/_alias/latest-*-{channel}"
    ))
    .header("User-Agent", format!("nh/{NH_VERSION}"))
    .basic_auth(BACKEND_USER, Some(BACKEND_PASSWORD))
    .send()
    .context("querying search.nixos.org for backend index versions")?
    .error_for_status()
    .context("querying search.nixos.org for backend index versions")?;

  let indices: HashMap<String, IndexAliases> =
    response.json().context("parsing backend index aliases")?;

  let suffix = format!("-{channel}");
  let latest = indices
    .into_values()
    .flat_map(|index| index.aliases.into_iter().map(|(alias, _)| alias))
    .filter_map(|alias| {
      alias
        .strip_prefix("latest-")?
        .strip_suffix(suffix.as_str())?
        .parse::<u32>()
        .ok()
    })
    .max();

  debug!(?latest, "discovered backend index version");
  Ok(latest)
}

#[derive(Deserialize)]
struct IndexAliases {
  aliases: serde_json::Map<String, serde_json::Value>,
}

/// Queries a single backend index version.
///
/// Returns [`BackendResponse::Outdated`] on a 404 (missing index) so the caller
/// can retry a newer version. Any other non-success status is a hard error.
fn query_backend(
  client: &Client,
  query: &Search,
  channel: &str,
  version: u32,
  contexts: SearchContexts,
) -> Result<BackendResponse> {
  let req = client
    .post(format!(
      "https://search.nixos.org/backend/latest-{version}-{channel}/_search"
    ))
    .json(query)
    .header("User-Agent", format!("nh/{NH_VERSION}"))
    .basic_auth(BACKEND_USER, Some(BACKEND_PASSWORD))
    .build()
    .context(contexts.build)?;

  debug!(?req);

  let response = client.execute(req).context(contexts.execute)?;
  trace!(?response);

  if response.status() == StatusCode::NOT_FOUND {
    return Ok(BackendResponse::Outdated);
  }

  if !response.status().is_success() {
    eprintln!(
      "Error: search.nixos.org returned HTTP {} for channel '{channel}'. This \
       usually means the channel does not exist, is not indexed, or the \
       request was malformed.",
      response.status(),
    );
    bail!(
      "search.nixos.org returned HTTP {} for channel '{channel}'",
      response.status(),
    );
  }

  Ok(BackendResponse::Found(response))
}

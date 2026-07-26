use std::time::{Duration, Instant};

use color_eyre::{
  Result,
  eyre::{Context, bail},
};
use elasticsearch_dsl::{Search, SearchResponse};
use reqwest::{
  StatusCode,
  blocking::{Client, Response},
};
use serde::de::DeserializeOwned;
use tracing::{debug, trace, warn};

const NH_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Backend index version bundled with nh, used when the user does not override
/// it via [`BackendConfig::version`].
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
  /// Index version to try first. `None` uses [`BUNDLED_BACKEND_VERSION`].
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
  let start = match config.version {
    Some(version) => version,
    None => {
      BUNDLED_BACKEND_VERSION
        .trim()
        .parse()
        .context("parsing the bundled backend index version")?
    },
  };
  let last = start.saturating_add(config.fallbacks);

  let client = reqwest::blocking::Client::new();
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
    // Hardcoded upstream
    // https://github.com/NixOS/nixos-search/blob/744ec58e082a3fcdd741b2c9b0654a0f7fda4603/frontend/src/index.js
    .basic_auth("aWVSALXpZv", Some("X8gPHnzL52wFEekuxsfQ9cSh"))
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

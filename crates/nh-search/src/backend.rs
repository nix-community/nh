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
const BACKEND_VERSION: &str = include_str!("../BACKEND_VERSION");

#[derive(Clone, Copy)]
pub struct SearchContexts {
  pub build:   &'static str,
  pub execute: &'static str,
  pub parse:   &'static str,
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
) -> Result<(Vec<T>, Duration)>
where
  T: DeserializeOwned,
{
  let pinned: u32 = BACKEND_VERSION
    .trim()
    .parse()
    .context("parsing the bundled backend index version")?;

  let client = reqwest::blocking::Client::new();
  let then = Instant::now();

  // The bundled index version tracks search.nixos.org but can fall behind
  // between releases. A missing index answers with 404, so when the pinned
  // version is outdated we retry once against the next version before failing.
  let response = match query_backend(&client, query, channel, pinned, contexts)?
  {
    BackendResponse::Found(response) => response,
    BackendResponse::Outdated => {
      let next = pinned + 1;
      warn!(
        "Backend index version {pinned} is outdated, retrying with {next}. \
         Consider updating nh."
      );
      match query_backend(&client, query, channel, next, contexts)? {
        BackendResponse::Found(response) => response,
        BackendResponse::Outdated => {
          bail!(
            "search.nixos.org has no index for channel '{channel}' at backend \
             version {pinned} or {next}. The channel may not exist, or nh may \
             be too old to query it."
          );
        },
      }
    },
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

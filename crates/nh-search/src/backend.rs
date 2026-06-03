use std::time::{Duration, Instant};

use color_eyre::{
  Result,
  eyre::{Context, bail},
};
use elasticsearch_dsl::{Search, SearchResponse};
use serde::de::DeserializeOwned;
use tracing::{debug, trace};

const NH_VERSION: &str = env!("CARGO_PKG_VERSION");
const BACKEND_VERSION: &str = include_str!("../BACKEND_VERSION");

#[derive(Clone, Copy)]
pub struct SearchContexts {
  pub build:   &'static str,
  pub execute: &'static str,
  pub parse:   &'static str,
}

pub fn search_documents<T>(
  query: &Search,
  channel: &str,
  contexts: SearchContexts,
) -> Result<(Vec<T>, Duration)>
where
  T: DeserializeOwned,
{
  let backend_version = BACKEND_VERSION.trim();
  let then = Instant::now();
  let client = reqwest::blocking::Client::new();
  let req = client
    .post(format!(
      "https://search.nixos.org/backend/latest-{backend_version}-{channel}/\
       _search"
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
    bail!(
      "search.nixos.org returned HTTP {} for channel '{channel}'",
      response.status(),
    );
  }

  let parsed_response: SearchResponse = response
    .json()
    .context("parsing response into the elasticsearch format")?;
  trace!(?parsed_response);

  let documents = parsed_response.documents::<T>().context(contexts.parse)?;
  Ok((documents, elapsed))
}

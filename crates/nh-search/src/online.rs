use color_eyre::Result;
use elasticsearch_dsl::Search;
use serde::de::DeserializeOwned;
use tracing::debug;

use crate::{
  args,
  backend::{self, SearchContexts},
  channel,
  query,
  render,
  types::{
    OptionJsonOutput,
    OptionSearchResult,
    PackageJsonOutput,
    PackageSearchResult,
  },
};

pub fn run_packages(
  channel: &str,
  limit: u64,
  platforms: bool,
  json: bool,
  query: &[String],
) -> Result<()> {
  run_online(&Packages { platforms }, channel, limit, json, query)
}

pub fn run_options(
  channel: &str,
  limit: u64,
  json: bool,
  scope: &args::OptionScope,
  query: &[String],
) -> Result<()> {
  run_online(&Options { scope }, channel, limit, json, query)
}

fn run_online<M>(
  mode: &M,
  requested_channel: &str,
  limit: u64,
  json: bool,
  query: &[String],
) -> Result<()>
where
  M: OnlineMode + ?Sized,
{
  let channel = channel::validate(requested_channel)?;
  let query_s = query.join(" ");
  mode.log_query(&query_s);

  let search = mode.search_query(&query_s, limit);

  if !json {
    mode.print_querying(&channel);
  }

  let (documents, elapsed) = backend::search_documents::<M::Document>(
    &search,
    &channel,
    mode.contexts(),
  )?;

  if json {
    return mode.print_json(query_s, channel, elapsed.as_millis(), documents);
  }

  println!("Took {}ms", elapsed.as_millis());
  println!("Most relevant results at the end");
  println!();
  mode.print_results(&channel, &documents);

  Ok(())
}

trait OnlineMode {
  type Document: DeserializeOwned;

  fn log_query(&self, query: &str);
  fn search_query(&self, query: &str, limit: u64) -> Search;
  fn contexts(&self) -> SearchContexts;
  fn print_querying(&self, channel: &str);
  fn print_json(
    &self,
    query: String,
    channel: String,
    elapsed_ms: u128,
    documents: Vec<Self::Document>,
  ) -> Result<()>;
  fn print_results(&self, channel: &str, documents: &[Self::Document]);
}

struct Packages {
  platforms: bool,
}

impl OnlineMode for Packages {
  type Document = PackageSearchResult;

  fn log_query(&self, query_s: &str) {
    debug!(?query_s);
  }

  fn search_query(&self, query: &str, limit: u64) -> Search {
    query::packages(query, limit)
  }

  fn contexts(&self) -> SearchContexts {
    SearchContexts {
      build:   "building search query",
      execute: "querying the elasticsearch API",
      parse:   "parsing search document",
    }
  }

  fn print_querying(&self, channel: &str) {
    println!("Querying search.nixos.org, with channel {channel}...");
  }

  fn print_json(
    &self,
    query: String,
    channel: String,
    elapsed_ms: u128,
    documents: Vec<Self::Document>,
  ) -> Result<()> {
    let json_output = PackageJsonOutput {
      query,
      channel,
      elapsed_ms,
      results: documents,
    };

    println!("{}", serde_json::to_string_pretty(&json_output)?);
    Ok(())
  }

  fn print_results(&self, channel: &str, documents: &[Self::Document]) {
    render::print_package_results(channel, self.platforms, documents);
  }
}

struct Options<'a> {
  scope: &'a args::OptionScope,
}

impl OnlineMode for Options<'_> {
  type Document = OptionSearchResult;

  fn log_query(&self, query_s: &str) {
    let scope = self.scope;
    debug!(?query_s, ?scope);
  }

  fn search_query(&self, query: &str, limit: u64) -> Search {
    query::options(self.scope, query, limit)
  }

  fn contexts(&self) -> SearchContexts {
    SearchContexts {
      build:   "building option search query",
      execute: "querying the elasticsearch API for options",
      parse:   "parsing option search document",
    }
  }

  fn print_querying(&self, channel: &str) {
    println!("Querying options on search.nixos.org, with channel {channel}...");
  }

  fn print_json(
    &self,
    query: String,
    channel: String,
    elapsed_ms: u128,
    documents: Vec<Self::Document>,
  ) -> Result<()> {
    let json_output = OptionJsonOutput {
      query,
      channel,
      scope: query::option_scope_label(self.scope).to_string(),
      elapsed_ms,
      results: documents,
    };

    println!("{}", serde_json::to_string_pretty(&json_output)?);
    Ok(())
  }

  fn print_results(&self, channel: &str, documents: &[Self::Document]) {
    render::print_option_results(channel, documents);
  }
}

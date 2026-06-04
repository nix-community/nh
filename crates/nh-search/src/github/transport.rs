use std::time::Duration;

use color_eyre::{
  Result,
  eyre::{Context, ContextCompat, bail},
};
use reqwest::blocking::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

const API_URL: &str = "https://api.github.com/graphql";
const SEND_ATTEMPTS: u32 = 3;
const SEND_RETRY_DELAY: Duration = Duration::from_millis(200);

pub struct GraphqlClient {
  client: Client,
  token:  SecretString,
}

#[derive(Debug, Deserialize)]
struct GraphqlResponse<T> {
  data:   Option<T>,
  errors: Option<Vec<GraphqlError>>,
}

#[derive(Debug, Deserialize)]
struct GraphqlError {
  message: String,
}

impl GraphqlClient {
  pub(super) fn new(token: SecretString) -> Result<Self> {
    let client = Client::builder()
      .timeout(Duration::from_secs(10))
      .user_agent(format!("nh-search/{}", env!("CARGO_PKG_VERSION")))
      .build()
      .context("failed to create GitHub HTTP client")?;

    Ok(Self { client, token })
  }

  pub fn query<T>(&self, query: &str, variables: &Value) -> Result<T>
  where
    T: DeserializeOwned,
  {
    let response = self.send(query, variables)?;

    let status = response.status();
    let body = response
      .text()
      .context("failed to read GitHub GraphQL response")?;

    if !status.is_success() {
      bail!(
        "GitHub GraphQL request failed ({status}): {}",
        truncate_body(&body)
      );
    }

    let payload = serde_json::from_str::<GraphqlResponse<T>>(&body)
      .context("failed to parse GitHub GraphQL response")?;

    if let Some(errors) = payload.errors
      && !errors.is_empty()
    {
      let messages = errors
        .into_iter()
        .map(|error| error.message)
        .collect::<Vec<_>>();
      if messages.is_empty() {
        bail!("GitHub GraphQL request failed");
      }

      bail!("GitHub GraphQL request failed: {}", messages.join("; "));
    }

    payload.data.context("GitHub GraphQL response missing data")
  }

  fn send(
    &self,
    query: &str,
    variables: &Value,
  ) -> Result<reqwest::blocking::Response> {
    let mut attempts = 0;

    loop {
      attempts += 1;
      let response = self
        .client
        .post(API_URL)
        .bearer_auth(self.token.expose_secret())
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&json!({
          "query": query,
          "variables": variables,
        }))
        .send();

      match response {
        Ok(response) => return Ok(response),
        Err(_) if attempts < SEND_ATTEMPTS => {
          std::thread::sleep(SEND_RETRY_DELAY * attempts);
        },
        Err(err) => {
          return Err(err).with_context(|| {
            format!(
              "failed to send GitHub GraphQL request after {SEND_ATTEMPTS} \
               attempts"
            )
          });
        },
      }
    }
  }
}

fn truncate_body(body: &str) -> String {
  const LIMIT: usize = 512;
  let body = body.trim();
  let truncated = body.chars().take(LIMIT).collect::<String>();
  if body.chars().count() > LIMIT {
    format!("{truncated}...")
  } else {
    truncated
  }
}

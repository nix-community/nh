use std::{
  env,
  io::{self, IsTerminal},
};

use color_eyre::{
  Result,
  eyre::{Context, bail},
};
use nh_config::ConfigStore;
use secrecy::SecretString;

const TOKEN_CREATION_URL: &str =
  "https://github.com/settings/personal-access-tokens/new";

pub fn token() -> Result<SecretString> {
  if let Some(token) = env_token() {
    return Ok(token);
  }

  let mut store = ConfigStore::load_default()?;
  if let Some(token) = store.config()?.auth.github_token {
    return Ok(token);
  }

  if !io::stdin().is_terminal() {
    bail!(
      "GitHub token not found; set GH_TOKEN or add auth.github_token to {}",
      store.path().display()
    );
  }

  eprintln!(
    "Create a GitHub token at {TOKEN_CREATION_URL} if you do not already have \
     one."
  );
  eprintln!(
    "No token scopes are needed; NH only uses it so GitHub treats requests as \
     authenticated."
  );
  eprintln!(
    "The token will be saved at {} with user-only permissions.",
    store.path().display()
  );

  let token = inquire::Password::new("GitHub token:")
    .without_confirmation()
    .prompt()
    .context("failed to read GitHub token")?;
  let token = token.trim();
  if token.is_empty() {
    bail!("empty GitHub token");
  }

  let token = SecretString::new(token.to_string().into());
  store.set_github_token(&token);
  store.save()?;
  eprintln!("Saved GitHub token to {}.", store.path().display());
  Ok(token)
}

fn env_token() -> Option<SecretString> {
  let token = env::var("GH_TOKEN").ok()?;
  let token = token.trim();
  (!token.is_empty()).then(|| SecretString::new(token.to_string().into()))
}

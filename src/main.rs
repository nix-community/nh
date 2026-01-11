mod checks;
mod clean;
mod commands;
mod darwin;
mod generations;
mod home;
mod installable;
mod interface;
mod json;
mod logging;
mod nixos;
mod remote;
mod search;
mod update;
mod util;

use std::str::FromStr;

use color_eyre::Result;
#[cfg(feature = "hotpath")] use hotpath;

use crate::commands::{ElevationStrategy, ElevationStrategyArg};

pub const NH_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const NH_REV: Option<&str> = option_env!("NH_REV");

fn main() -> Result<()> {
  #[cfg(feature = "hotpath")]
  let _guard = hotpath::GuardBuilder::new("main").build();

  let mut args = <crate::interface::Main as clap::Parser>::parse();

  // Backward compatibility: support NH_ELEVATION_PROGRAM env var if
  // NH_ELEVATION_STRATEGY is not set.
  // TODO: Remove this fallback in a future version
  if args.elevation_strategy.is_none() {
    if let Some(old_value) = std::env::var("NH_ELEVATION_PROGRAM")
      .ok()
      .filter(|v| !v.is_empty())
    {
      tracing::warn!(
        "NH_ELEVATION_PROGRAM is deprecated, use NH_ELEVATION_STRATEGY \
         instead. Falling back to NH_ELEVATION_PROGRAM for backward \
         compatibility. Accepted values: none, passwordless, program:<path>"
      );
      match ElevationStrategyArg::from_str(&old_value) {
        Ok(strategy) => args.elevation_strategy = Some(strategy),
        Err(e) => {
          tracing::warn!(
            "Failed to parse NH_ELEVATION_PROGRAM value '{}': {}. Falling \
             back to none.",
            old_value,
            e
          );
        },
      }
    }
  }

  // Set up logging
  crate::logging::setup_logging(args.verbosity)?;
  tracing::debug!("{args:#?}");
  tracing::debug!(%NH_VERSION, ?NH_REV);

  // Check Nix version upfront
  checks::verify_nix_environment()?;

  // Once we assert required Nix features, validate NH environment checks
  // For now, this is just NH_* variables being set. More checks may be
  // added to setup_environment in the future.
  checks::verify_variables()?;

  let elevation =
    args
      .elevation_strategy
      .as_ref()
      .map_or(ElevationStrategy::Auto, |arg| {
        match arg {
          ElevationStrategyArg::Auto => ElevationStrategy::Auto,
          ElevationStrategyArg::None => ElevationStrategy::None,
          ElevationStrategyArg::Passwordless => ElevationStrategy::Passwordless,
          ElevationStrategyArg::Program(path) => {
            ElevationStrategy::Prefer(path.clone())
          },
        }
      });

  args.command.run(elevation)
}

use std::str::FromStr;

use color_eyre::Result;
use nh_core::command::{Elevation, ElevationStrategyArg};

pub mod interface;
pub mod logging;

pub const NH_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const NH_REV: Option<&str> = option_env!("NH_REV");

pub fn main() -> Result<()> {
  let mut args = <crate::interface::Main as clap::Parser>::parse();

  // Backward compatibility: support NH_ELEVATION_PROGRAM env var if
  // NH_ELEVATION_STRATEGY is not set.
  // TODO: Remove this fallback in a future version
  if args.elevation_strategy.is_none()
    && let Some(old_value) = std::env::var("NH_ELEVATION_PROGRAM")
      .ok()
      .filter(|v| !v.is_empty())
  {
    tracing::warn!(
      "NH_ELEVATION_PROGRAM is deprecated, use NH_ELEVATION_STRATEGY instead. \
       Falling back to NH_ELEVATION_PROGRAM for backward compatibility. \
       Accepted values: none, passwordless, empty-password, program:<path>"
    );
    match ElevationStrategyArg::from_str(&old_value) {
      Ok(strategy) => args.elevation_strategy = Some(strategy),
      Err(e) => {
        tracing::warn!(
          "Failed to parse NH_ELEVATION_PROGRAM value '{}': {}. Falling back \
           to none.",
          old_value,
          e
        );
      },
    }
  }

  // Set up logging
  crate::logging::setup_logging(args.verbosity)?;
  tracing::debug!("{args:#?}");
  tracing::debug!(%NH_VERSION, ?NH_REV);

  // Check Nix version upfront
  nh_core::checks::verify_nix_environment()?;

  // Once we assert required Nix features, validate NH environment checks
  // For now, this is just NH_* variables being set. More checks may be
  // added to setup_environment in the future.
  nh_core::checks::verify_variables()?;

  let elevation = args
    .elevation_strategy
    .clone()
    .map_or_else(Elevation::default, Elevation::from);

  args.command.run(elevation)
}

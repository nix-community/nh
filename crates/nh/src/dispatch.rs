mod clean;
mod darwin;
mod home;
mod os;

use nh_cli::NHCommand;
use nh_core::{
  checks::{FeatureRequirements, NoFeatures},
  command::ElevationStrategy,
};

use crate::Result;

#[must_use]
pub fn get_feature_requirements(
  command: &NHCommand,
) -> Box<dyn FeatureRequirements> {
  match command {
    NHCommand::Os { args } => os::feature_requirements(args),
    NHCommand::Home { args } => home::feature_requirements(args),
    NHCommand::Darwin { args } => darwin::feature_requirements(args),
    NHCommand::Search { .. } | NHCommand::Clean { .. } => Box::new(NoFeatures),
  }
}

/// Run the selected subcommand.
///
/// # Errors
///
/// Returns an error if required Nix features are unavailable or if the
/// selected subcommand fails.
pub fn run(command: NHCommand, elevation: ElevationStrategy) -> Result<()> {
  // Check features specific to this command
  let requirements = get_feature_requirements(&command);
  requirements.check_features()?;

  match command {
    NHCommand::Os { args } => os::run(args, elevation),
    NHCommand::Search { args } => args.run(),
    NHCommand::Clean { args } => clean::run(args, elevation),
    NHCommand::Home { args } => home::run(args),
    NHCommand::Darwin { args } => darwin::run(args, elevation),
  }
}

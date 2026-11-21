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
    NHCommand::Os { args } => args.get_feature_requirements(),
    NHCommand::Home { args } => args.get_feature_requirements(),
    NHCommand::Darwin { args } => args.get_feature_requirements(),
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
    NHCommand::Os { args } => args.run(elevation),
    NHCommand::Search { args } => args.run(),
    NHCommand::Clean { args: proxy } => proxy.command.run(elevation),
    NHCommand::Home { args } => args.run(),
    NHCommand::Darwin { args } => args.run(elevation),
  }
}

use nh_cli::darwin::{
  DarwinArgs,
  DarwinRebuildArgs,
  DarwinReplArgs,
  DarwinSubcommand,
};
use nh_core::{
  checks::{
    DarwinReplFeatures,
    FeatureRequirements,
    FlakeFeatures,
    LegacyFeatures,
  },
  command::ElevationStrategy,
};
use nh_darwin::{DarwinCommand, DarwinRebuildOptions, DarwinReplOptions};
use nh_installable::CommandContext;

use super::common;
use crate::Result;

pub(super) fn feature_requirements(
  args: &DarwinArgs,
) -> Box<dyn FeatureRequirements> {
  match &args.subcommand {
    DarwinSubcommand::Repl { args } => {
      let is_flake = args.installable.uses_flakes(CommandContext::Darwin);
      Box::new(DarwinReplFeatures { is_flake })
    },
    DarwinSubcommand::Switch { args } | DarwinSubcommand::Build { args } => {
      if args.common.installable.uses_flakes(CommandContext::Darwin) {
        Box::new(FlakeFeatures)
      } else {
        Box::new(LegacyFeatures)
      }
    },
  }
}

pub(super) fn run(
  args: DarwinArgs,
  elevation: ElevationStrategy,
) -> Result<()> {
  command(args.subcommand).run(elevation)
}

fn command(subcommand: DarwinSubcommand) -> DarwinCommand {
  match subcommand {
    DarwinSubcommand::Switch { args } => DarwinCommand::Switch(rebuild(args)),
    DarwinSubcommand::Build { args } => DarwinCommand::Build(rebuild(args)),
    DarwinSubcommand::Repl { args } => DarwinCommand::Repl(repl(args)),
  }
}

#[expect(
  deprecated,
  reason = "show_activation_logs is still parsed but no longer has an effect"
)]
fn rebuild(args: DarwinRebuildArgs) -> DarwinRebuildOptions {
  let DarwinRebuildArgs {
    common,
    update_args,
    hostname,
    extra_args,
    bypass_root_check,
    show_activation_logs: _,
    build_host,
  } = args;
  DarwinRebuildOptions {
    common: common::common_rebuild(common),
    update_args: common::update(update_args),
    hostname,
    extra_args,
    bypass_root_check,
    build_host,
  }
}

fn repl(args: DarwinReplArgs) -> DarwinReplOptions {
  let DarwinReplArgs {
    installable,
    hostname,
  } = args;
  DarwinReplOptions {
    installable,
    hostname,
  }
}

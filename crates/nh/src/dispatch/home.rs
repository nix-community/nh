use nh_cli::home::{HomeArgs, HomeRebuildArgs, HomeReplArgs, HomeSubcommand};
use nh_core::checks::{
  FeatureRequirements,
  FlakeFeatures,
  HomeReplFeatures,
  LegacyFeatures,
};
use nh_home::{HomeCommand, HomeRebuildOptions, HomeReplOptions};
use nh_installable::CommandContext;

use super::common;
use crate::Result;

pub(super) fn feature_requirements(
  args: &HomeArgs,
) -> Box<dyn FeatureRequirements> {
  match &args.subcommand {
    HomeSubcommand::Repl { args } => {
      let is_flake = args.installable.0.uses_flakes(CommandContext::Home);
      Box::new(HomeReplFeatures { is_flake })
    },
    HomeSubcommand::Switch { args } | HomeSubcommand::Build { args } => {
      if args.common.installable.0.uses_flakes(CommandContext::Home) {
        Box::new(FlakeFeatures)
      } else {
        Box::new(LegacyFeatures)
      }
    },
  }
}

pub(super) fn run(args: HomeArgs) -> Result<()> {
  command(args.subcommand).run()
}

fn command(subcommand: HomeSubcommand) -> HomeCommand {
  match subcommand {
    HomeSubcommand::Switch { args } => HomeCommand::Switch(rebuild(args)),
    HomeSubcommand::Build { args } => HomeCommand::Build(rebuild(args)),
    HomeSubcommand::Repl { args } => HomeCommand::Repl(repl(args)),
  }
}

#[expect(
  deprecated,
  reason = "show_activation_logs is still parsed but no longer has an effect"
)]
fn rebuild(args: HomeRebuildArgs) -> HomeRebuildOptions {
  let HomeRebuildArgs {
    common,
    update_args,
    configuration,
    specialisation,
    no_specialisation,
    extra_args,
    backup_extension,
    show_activation_logs: _,
    build_host,
  } = args;
  HomeRebuildOptions {
    common: common::common_rebuild(common),
    update_args: common::update(update_args),
    configuration,
    specialisation,
    no_specialisation,
    extra_args,
    backup_extension,
    build_host,
  }
}

fn repl(args: HomeReplArgs) -> HomeReplOptions {
  let HomeReplArgs {
    installable,
    configuration,
    extra_args,
  } = args;
  HomeReplOptions {
    installable: common::installable(installable),
    configuration,
    extra_args,
  }
}

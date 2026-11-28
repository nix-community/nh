use nh_cli::os::{
  Field,
  OsArgs,
  OsBuildArgs,
  OsBuildImageArgs,
  OsBuildVmArgs,
  OsDryActivateArgs,
  OsGenerationsArgs,
  OsRebuildActivateArgs,
  OsRebuildArgs,
  OsReplArgs,
  OsRollbackArgs,
  OsSubcommand,
};
use nh_core::{
  checks::{
    FeatureRequirements,
    FlakeFeatures,
    LegacyFeatures,
    OsReplFeatures,
  },
  command::ElevationStrategy,
};
use nh_installable::CommandContext;
use nh_nixos::{
  generations,
  nixos::{
    OsBuildImageOptions,
    OsBuildOptions,
    OsBuildVmOptions,
    OsCommand,
    OsDryActivateOptions,
    OsGenerationsOptions,
    OsRebuildActivateOptions,
    OsRebuildOptions,
    OsReplOptions,
    OsRollbackOptions,
  },
};

use super::common;
use crate::Result;

pub(super) fn feature_requirements(
  args: &OsArgs,
) -> Box<dyn FeatureRequirements> {
  match &args.subcommand {
    OsSubcommand::Repl { args } => {
      let is_flake = args.installable.0.uses_flakes(CommandContext::Os);
      Box::new(OsReplFeatures { is_flake })
    },
    OsSubcommand::Switch { args }
    | OsSubcommand::Boot { args }
    | OsSubcommand::Test { args } => {
      if args
        .rebuild
        .common
        .installable
        .0
        .uses_flakes(CommandContext::Os)
      {
        Box::new(FlakeFeatures)
      } else {
        Box::new(LegacyFeatures)
      }
    },
    OsSubcommand::Build {
      args: OsBuildArgs { rebuild: args, .. },
    }
    | OsSubcommand::DryActivate {
      args: OsDryActivateArgs { rebuild: args, .. },
    } => {
      if args.common.installable.0.uses_flakes(CommandContext::Os) {
        Box::new(FlakeFeatures)
      } else {
        Box::new(LegacyFeatures)
      }
    },
    OsSubcommand::BuildVm { args } => {
      if args
        .common
        .common
        .installable
        .0
        .uses_flakes(CommandContext::Os)
      {
        Box::new(FlakeFeatures)
      } else {
        Box::new(LegacyFeatures)
      }
    },
    OsSubcommand::Info { .. } | OsSubcommand::Rollback { .. } => {
      Box::new(LegacyFeatures)
    },

    OsSubcommand::BuildImage { args } => {
      if args
        .common
        .common
        .installable
        .0
        .uses_flakes(CommandContext::Os)
      {
        Box::new(FlakeFeatures)
      } else {
        Box::new(LegacyFeatures)
      }
    },
  }
}

pub(super) fn run(args: OsArgs, elevation: ElevationStrategy) -> Result<()> {
  command(args.subcommand).run(elevation)
}

fn command(subcommand: OsSubcommand) -> OsCommand {
  match subcommand {
    OsSubcommand::Switch { args } => OsCommand::Switch(rebuild_activate(args)),
    OsSubcommand::Boot { args } => OsCommand::Boot(rebuild_activate(args)),
    OsSubcommand::Test { args } => OsCommand::Test(rebuild_activate(args)),
    OsSubcommand::DryActivate { args } => {
      OsCommand::DryActivate(dry_activate(args))
    },
    OsSubcommand::Build { args } => OsCommand::Build(build(args)),
    OsSubcommand::Repl { args } => OsCommand::Repl(repl(args)),
    OsSubcommand::Info { args } => OsCommand::Info(generations_options(args)),
    OsSubcommand::Rollback { args } => OsCommand::Rollback(rollback(args)),
    OsSubcommand::BuildVm { args } => OsCommand::BuildVm(build_vm(args)),
    OsSubcommand::BuildImage { args } => {
      OsCommand::BuildImage(build_image(args))
    },
  }
}

fn rebuild(args: OsRebuildArgs) -> OsRebuildOptions {
  let OsRebuildArgs {
    common,
    update_args,
    hostname,
    specialisation,
    no_specialisation,
    extra_args,
    bypass_root_check,
    target_host,
    build_host,
    no_validate,
  } = args;
  OsRebuildOptions {
    common: common::common_rebuild(common),
    update_args: common::update(update_args),
    hostname,
    specialisation,
    no_specialisation,
    extra_args,
    bypass_root_check,
    target_host,
    build_host,
    no_validate,
  }
}

#[expect(
  deprecated,
  reason = "show_activation_logs is still parsed but no longer has an effect"
)]
fn rebuild_activate(args: OsRebuildActivateArgs) -> OsRebuildActivateOptions {
  let OsRebuildActivateArgs {
    rebuild: rebuild_args,
    install_bootloader,
    show_activation_logs: _,
    continue_on_activation_failure,
    no_systemd_run,
  } = args;
  OsRebuildActivateOptions {
    rebuild: rebuild(rebuild_args),
    install_bootloader,
    continue_on_activation_failure,
    no_systemd_run,
  }
}

fn dry_activate(args: OsDryActivateArgs) -> OsDryActivateOptions {
  let OsDryActivateArgs {
    rebuild: rebuild_args,
    no_systemd_run,
  } = args;
  OsDryActivateOptions {
    rebuild: rebuild(rebuild_args),
    no_systemd_run,
  }
}

fn build(args: OsBuildArgs) -> OsBuildOptions {
  let OsBuildArgs {
    rebuild: rebuild_args,
    no_link,
  } = args;
  OsBuildOptions {
    rebuild: rebuild(rebuild_args),
    no_link,
  }
}

fn build_vm(args: OsBuildVmArgs) -> OsBuildVmOptions {
  let OsBuildVmArgs {
    common,
    with_bootloader,
    run,
  } = args;
  OsBuildVmOptions {
    common: rebuild(common),
    with_bootloader,
    run,
  }
}

fn build_image(args: OsBuildImageArgs) -> OsBuildImageOptions {
  let OsBuildImageArgs {
    common,
    image_variant,
  } = args;
  OsBuildImageOptions {
    common: rebuild(common),
    image_variant,
  }
}

fn repl(args: OsReplArgs) -> OsReplOptions {
  let OsReplArgs {
    installable,
    hostname,
  } = args;
  OsReplOptions {
    installable: common::installable(installable),
    hostname,
  }
}

fn generations_options(args: OsGenerationsArgs) -> OsGenerationsOptions {
  let OsGenerationsArgs {
    profile,
    fields,
    json,
  } = args;
  OsGenerationsOptions {
    profile,
    fields: fields.map(|fields| fields.iter().map(field).collect()),
    json,
  }
}

fn rollback(args: OsRollbackArgs) -> OsRollbackOptions {
  let OsRollbackArgs {
    dry,
    ask,
    specialisation,
    no_specialisation,
    to,
    bypass_root_check,
    diff,
    no_systemd_run,
  } = args;
  OsRollbackOptions {
    dry,
    ask,
    specialisation,
    no_specialisation,
    to,
    bypass_root_check,
    diff: common::diff(diff),
    no_systemd_run,
  }
}

const fn field(value: &Field) -> generations::Field {
  match value {
    Field::Id => generations::Field::Id,
    Field::Date => generations::Field::Date,
    Field::Nver => generations::Field::Nver,
    Field::Kernel => generations::Field::Kernel,
    Field::Confrev => generations::Field::Confrev,
    Field::Spec => generations::Field::Spec,
    Field::Size => generations::Field::Size,
  }
}

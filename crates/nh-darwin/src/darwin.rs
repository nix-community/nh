pub mod args;
mod rollback;

use std::{
  convert::Into,
  fs,
  io,
  path::{Path, PathBuf},
};

use args::{DarwinArgs, DarwinRebuildArgs, DarwinReplArgs, DarwinSubcommand};
use misstep::{Result, ResultExt, bail};
use nh_core::{
  command::{Command, CommandKind, ElevationStrategy, NixCommand},
  privileged::{DarwinActivation, PrivilegedOp},
  update::update_with_args,
  util::{get_hostname, use_rom},
};
use nh_diff::print_diff;
use nh_installable::{
  CommandContext,
  ConfigurationInstallable,
  ConfigurationLayout,
  Installable,
};
use tracing::{debug, warn};

const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
const CURRENT_PROFILE: &str = "/run/current-system";

impl DarwinArgs {
  /// Run the `darwin` subcommand.
  ///
  /// # Parameters
  ///
  /// * `self` - The Darwin operation arguments
  /// * `elevation` - The privilege elevation strategy (sudo/doas/none)
  ///
  /// # Returns
  ///
  /// Returns `Ok(())` if the operation succeeds.
  ///
  /// # Errors
  ///
  /// Returns an error if:
  ///
  /// - Build or activation operations fail
  /// - Remote operations encounter network or SSH issues
  /// - Nix evaluation or building fails
  /// - File system operations fail
  pub fn run(self, elevation: ElevationStrategy) -> Result<()> {
    use DarwinRebuildVariant::{Build, Switch};
    match self.subcommand {
      DarwinSubcommand::Switch { args } => args.rebuild(&Switch, elevation),
      DarwinSubcommand::Build { args } => {
        if args.common.ask || args.common.dry {
          warn!("`--ask` and `--dry` have no effect for `nh darwin build`");
        }
        args.rebuild(&Build, elevation)
      },
      DarwinSubcommand::Repl { args } => args.run(),
      DarwinSubcommand::Rollback { args } => args.rollback(elevation),
    }
  }
}

enum DarwinRebuildVariant {
  Switch,
  Build,
}

impl DarwinRebuildArgs {
  fn rebuild(
    self,
    variant: &DarwinRebuildVariant,
    elevation: ElevationStrategy,
  ) -> Result<()> {
    use DarwinRebuildVariant::{Build, Switch};

    if nix::unistd::Uid::effective().is_root() && !self.bypass_root_check {
      bail!(
        "Don't run nh darwin as root. I will call sudo internally as needed"
      );
    }

    let hostname = get_hostname(self.hostname)?;

    let (out_path, _tempdir_guard): (PathBuf, Option<tempfile::TempDir>) =
      if let Some(ref p) = self.common.out_link {
        (p.clone(), None)
      } else {
        let dir = tempfile::Builder::new().prefix("nh-darwin").tempdir()?;
        (dir.as_ref().join("result"), Some(dir))
      };

    debug!("Output path: {out_path:?}");

    let installable = self
      .common
      .installable
      .clone()
      .resolve_or_default(CommandContext::Darwin)?;

    if self.update_args.update_all || self.update_args.update_input.is_some() {
      update_with_args(
        &installable,
        self.update_args.update_input,
        &self.common.passthrough,
      )?;
    }

    let toplevel = toplevel_for(hostname, installable, "toplevel")?;

    let build = nh_core::command::Build::new(toplevel)
      .extra_args(&self.extra_args)
      .passthrough(&self.common.passthrough)
      .message("Building Darwin configuration")
      .rom(use_rom(self.common.no_rom, self.common.no_nom));

    // Initialize SSH control - guard will cleanup connections on drop
    let _ssh_guard =
      self.build_host.is_some().then(nh_remote::init_ssh_control);

    nh_remote::build(
      build,
      self.build_host.as_ref(),
      None,
      &out_path,
      &self.common.passthrough,
    )
    .context("Failed to build Darwin configuration")?;

    let target_profile = out_path.clone();

    if !target_profile
      .try_exists()
      .context("Failed to check build output")?
    {
      bail!(
        "Darwin build output does not exist: {}",
        target_profile.display()
      );
    }

    print_diff(
      &self.common.diff,
      None,
      Path::new(CURRENT_PROFILE),
      &target_profile,
    )?;

    if self.common.ask && !self.common.dry && !matches!(variant, Build) {
      let confirmation = inquire::Confirm::new("Apply the config?")
        .with_default(false)
        .prompt()?;

      if !confirmation {
        bail!("User rejected the new config");
      }
    }

    if matches!(variant, Switch) {
      let activate_user = out_path.join("activate-user");

      // Determine if we need to elevate privileges
      let needs_elevation = !activate_user
        .try_exists()
        .context("Failed to check if activate-user file exists")?
        || std::fs::read_to_string(&activate_user)
          .context("Failed to read activate-user file")?
          .contains("# nix-darwin: deprecated");

      let activation = DarwinActivation {
        system:   out_path.clone(),
        activate: needs_elevation,
        nix_args: self.common.passthrough.generate_passthrough_args(),
      };
      if self.common.dry
        || nix::unistd::Uid::effective().is_root()
        || matches!(elevation, ElevationStrategy::None)
      {
        run_darwin_commands(&activation, self.common.dry)?;
      } else {
        PrivilegedOp::DarwinActivate(activation).run_elevated(elevation)?;
      }

      if !needs_elevation {
        run_activate(&out_path, self.common.dry)?;
      }
    }

    debug!("Completed operation with output path: {out_path:?}");

    Ok(())
  }
}

/// Installs the system profile and, when requested, activates it.
///
/// # Errors
///
/// Returns an error if profile installation or activation fails.
pub fn run_darwin_commands(
  activation: &DarwinActivation,
  dry: bool,
) -> Result<()> {
  let (binary, args, _) = NixCommand::new(CommandKind::Build)
    .print_build_logs(false)
    .args(["--no-link", "--profile", SYSTEM_PROFILE])
    .arg(&activation.system)
    .args(&activation.nix_args)
    .into_parts();
  Command::new(binary)
    .args(args)
    .dry(dry)
    .with_required_env()
    .run()
    .context("Failed to set Darwin system profile")?;

  if activation.activate {
    run_activate(&activation.system, dry)?;
  }
  Ok(())
}

fn run_activate(system: &Path, dry: bool) -> Result<()> {
  Command::new(system.join("activate"))
    .message("Activating configuration")
    .dry(dry)
    .show_output(true)
    .with_required_env()
    .run()
    .context("Darwin activation failed")
}

/// Prepare activation before changing the system profile.
fn activation_command(
  system: &Path,
  elevation: Option<ElevationStrategy>,
  show_logs: bool,
) -> Result<Command> {
  let activate = system.join("activate");
  fs::metadata(&activate).with_context(|| {
    format!(
      "Missing Darwin activation executable: {}",
      activate.display()
    )
  })?;

  let activate_user = system.join("activate-user");
  let needs_elevation = match fs::read_to_string(&activate_user) {
    Ok(script) => {
      script
        .lines()
        .any(|line| line == "# nix-darwin: deprecated")
    },
    Err(error) if error.kind() == io::ErrorKind::NotFound => true,
    Err(error) => {
      return Err(error)
        .with_context(|| format!("Failed to read {}", activate_user.display()))
        .erase();
    },
  };

  Ok(
    Command::new(activate)
      .message("Activating configuration")
      .elevate(if needs_elevation { elevation } else { None })
      .show_output(show_logs)
      .with_required_env(),
  )
}

fn elevation_if_needed(
  elevation: ElevationStrategy,
) -> Option<ElevationStrategy> {
  if nix::unistd::Uid::effective().is_root()
    || matches!(elevation, ElevationStrategy::None)
  {
    None
  } else {
    Some(elevation)
  }
}

impl DarwinReplArgs {
  fn run(self) -> Result<()> {
    let mut target_installable = self
      .installable
      .resolve_or_default(CommandContext::Darwin)?;

    if matches!(target_installable, Installable::Store { .. }) {
      bail!("Nix doesn't support nix store installables.");
    }

    let hostname = get_hostname(self.hostname)?;

    // Enter the repl at the configuration itself, without a build attribute.
    target_installable.resolve_configuration(
      ConfigurationLayout {
        set:        "darwinConfigurations",
        build_attr: &[],
      },
      Some(&hostname),
    )?;

    let status = NixCommand::new(CommandKind::Repl)
      .args(target_installable.to_args()?)
      .with_required_env()
      .run_with_logs()?;
    if !status.success() {
      bail!("nix repl failed (exit status {status:?})");
    }

    Ok(())
  }
}

/// Resolve a nix-darwin installable to the requested system build attribute.
///
/// # Errors
///
/// Returns an error if the installable is a store path or if the flake
/// attribute path is too specific to infer the requested build attribute.
pub fn toplevel_for<S: AsRef<str>>(
  hostname: S,
  installable: Installable,
  final_attr: &str,
) -> Result<Installable> {
  if matches!(installable, Installable::Store { .. }) {
    bail!("Nix doesn't support nix store installables.");
  }

  let mut res = installable;
  res.resolve_configuration(
    ConfigurationLayout {
      set:        "darwinConfigurations",
      build_attr: &["config", "system", "build", final_attr],
    },
    Some(hostname.as_ref()),
  )?;

  Ok(res)
}

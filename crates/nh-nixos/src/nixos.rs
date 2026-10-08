use std::{
  fs,
  path::{Path, PathBuf},
};

use misstep::{OptionExt, Report, Result, ResultExt, bail, report};
use nh_core::{
  command::{self, Command, CommandKind, ElevationStrategy, NixCommand},
  privileged::{
    NixosActivation,
    NixosActivationAction,
    NixosRollback,
    PrivilegedOp,
  },
  update::update_with_args,
  util::{
    ensure_ssh_key_login,
    get_build_image_variants_flake_with_args,
    get_build_image_variants_with_args,
    get_hostname,
    use_rom,
  },
};
use nh_diff::print_diff;
use nh_installable::{
  CommandContext,
  ConfigurationInstallable,
  ConfigurationLayout,
  Installable,
  InstallableArgs,
};
use nh_remote::{self, RemoteHost};
use tracing::{debug, info, warn};

use crate::{
  args::{
    self,
    OsBuildImageArgs,
    OsBuildVmArgs,
    OsGenerationsArgs,
    OsRebuildActivateArgs,
    OsRebuildArgs,
    OsReplArgs,
    OsRollbackArgs,
    OsSubcommand::{self},
  },
  generations,
};

const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
const CURRENT_PROFILE: &str = "/run/current-system";

const SPEC_LOCATION: &str = "/etc/specialisation";

/// Essential files that must exist in a valid NixOS system closure. Each tuple
/// contains the file path relative to the system profile and its description.
/// The descriptions are used on log messages or errors.
const ESSENTIAL_FILES: &[(&str, &str)] = &[
  ("bin/switch-to-configuration", "activation script"),
  ("nixos-version", "system version identifier"),
  ("init", "system init script"),
  ("sw/bin", "system path"),
];

fn activation_command(path: &Path, action: &str, systemd_run: bool) -> Command {
  let command = if systemd_run && Path::new("/run/systemd/system").is_dir() {
    Command::new("systemd-run")
      .args(nh_remote::SYSTEMD_RUN_ARGS)
      .arg(path)
  } else {
    Command::new(path)
  };

  command.arg(action)
}

impl args::OsArgs {
  /// Executes the NixOS subcommand.
  ///
  /// # Parameters
  ///
  /// * `self` - The NixOS operation arguments
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
    use OsRebuildVariant::{Boot, Build, DryActivate, Switch, Test};
    match self.subcommand {
      OsSubcommand::Boot { args } => {
        args.rebuild_and_activate(&Boot, None, elevation)
      },
      OsSubcommand::Test { args } => {
        args.rebuild_and_activate(&Test, None, elevation)
      },
      OsSubcommand::Switch { args } => {
        args.rebuild_and_activate(&Switch, None, elevation)
      },
      OsSubcommand::DryActivate { args } => {
        #[expect(deprecated, reason = "the struct still carries the flag")]
        let activate = OsRebuildActivateArgs {
          rebuild:                        args.rebuild,
          install_bootloader:             false,
          show_activation_logs:           false,
          continue_on_activation_failure: false,
          no_systemd_run:                 args.no_systemd_run,
        };
        activate.rebuild_and_activate(&DryActivate, None, elevation)
      },
      OsSubcommand::Build { args } => {
        if args.common.ask {
          warn!("`--ask` has no effect for `nh os build`");
        }

        if args.common.dry {
          args.dry_build()
        } else {
          args.build_only(&Build, None, &elevation)
        }
      },
      OsSubcommand::BuildVm { args } => args.build_vm(&elevation),
      OsSubcommand::Repl { args } => args.run(),
      OsSubcommand::Info { args } => args.info(),
      OsSubcommand::Rollback { args } => args.rollback(elevation),
      OsSubcommand::BuildImage { args } => args.build_image(&elevation),
    }
  }
}

#[derive(Debug)]
enum OsRebuildVariant {
  Build,
  Switch,
  Boot,
  Test,
  DryActivate,
  BuildVm,
  BuildIso,
}

/// Runs local activation steps that need root.
///
/// # Errors
///
/// Returns an error if activation, profile installation or the bootloader
/// step fails.
pub fn run_activation_commands(activation: &NixosActivation) -> Result<()> {
  use NixosActivationAction::{Boot, DryActivate, Switch, Test};

  if let Test | Switch | DryActivate = activation.action {
    let action = if matches!(activation.action, DryActivate) {
      "dry-activate"
    } else {
      "test"
    };
    let activation_result = activation_command(
      &activation.switch_to_configuration,
      action,
      activation.systemd_run,
    )
    .message(if matches!(activation.action, DryActivate) {
      "Previewing activation"
    } else {
      "Activating configuration"
    })
    .preserve_envs(["NIXOS_INSTALL_BOOTLOADER", "NIXOS_NO_CHECK"])
    .with_required_env()
    .show_output(true)
    .run()
    .context(format!("Activation ({action}) failed"));

    if let Err(e) = activation_result {
      handle_activation_failure(
        e,
        matches!(activation.action, Switch),
        activation.continue_on_activation_failure,
      )?;
    }
  }

  if let Boot | Switch = activation.action {
    let (binary, args, _) = NixCommand::new(CommandKind::Build)
      .print_build_logs(false)
      .args(["--no-link", "--profile", SYSTEM_PROFILE])
      .arg(&activation.system)
      .args(&activation.nix_args)
      .into_parts();
    Command::new(binary)
      .args(args)
      .with_required_env()
      .run()
      .context("Failed to set system profile")?;

    let mut cmd = activation_command(
      &activation.switch_to_configuration,
      "boot",
      activation.systemd_run,
    )
    .message("Adding configuration to bootloader")
    .preserve_envs(["NIXOS_INSTALL_BOOTLOADER", "NIXOS_NO_CHECK"]);

    if activation.install_bootloader {
      cmd = cmd.set_env("NIXOS_INSTALL_BOOTLOADER", "1");
    }

    cmd
      .with_required_env()
      .run()
      .context("Bootloader activation failed")?;
  }

  Ok(())
}

fn handle_activation_failure(
  e: Report,
  is_switch: bool,
  continue_on_activation_failure: bool,
) -> Result<()> {
  // On `switch`, activation runs before the bootloader step, so a failure
  // here normally skips it and leaves nothing new to boot into. `test`
  // has no bootloader step, so its failures are always fatal.
  if is_switch && continue_on_activation_failure {
    warn!("{e:?}");
    warn!(
      "Activation failed; adding the generation to the bootloader anyway \
       (NH_CONTINUE_ON_ACTIVATION_FAILURE set)."
    );
    return Ok(());
  }
  if is_switch {
    warn!(
      "Activation failed; the new generation will not be added to the \
       bootloader. Pass --continue-on-activation-failure to override."
    );
  }
  Err(e)
}

/// Points the system profile at the rollback target and activates it.
///
/// # Errors
///
/// Returns an error if the profile cannot be set or activation fails.
pub fn run_rollback_commands(rollback: &NixosRollback) -> Result<()> {
  // Set the system profile
  info!("Setting system profile...");

  Command::new("ln")
    .arg("-sfn") // force, symbolic link
    .arg(&rollback.target_profile)
    .arg(SYSTEM_PROFILE)
    .message("Setting system profile")
    .with_required_env()
    .run()
    .context("Failed to set system profile during rollback")?;

  info!("Activating...");

  if let Err(e) = activation_command(
    &rollback.switch_to_configuration,
    "switch",
    rollback.systemd_run,
  )
  .preserve_envs(["NIXOS_INSTALL_BOOTLOADER", "NIXOS_NO_CHECK"])
  .with_required_env()
  .run()
  {
    // If activation fails, rollback the profile
    if let Some(previous_profile) = &rollback.previous_profile {
      Command::new("ln")
        .arg("-sfn") // Force, symbolic link
        .arg(previous_profile)
        .arg(SYSTEM_PROFILE)
        .message("Rolling back system profile")
        .with_required_env()
        .run()
        .context(
          "NixOS: Failed to restore previous system profile after failed \
           activation",
        )?;
    }

    return Err(
      e.context("Activation (switch) failed")
        .context("Failed to activate configuration"),
    );
  }

  Ok(())
}

impl OsBuildVmArgs {
  fn build_vm(self, elevation: &ElevationStrategy) -> Result<()> {
    let attr = if self.with_bootloader {
      "vmWithBootLoader"
    } else {
      "vm"
    };
    let out_path = self
      .common
      .common
      .out_link
      .clone()
      .unwrap_or_else(|| PathBuf::from("result"));

    debug!("Building VM with attribute: {}", attr);

    // Show warning if no hostname was explicitly provided for VM builds
    if self.common.hostname.is_none() {
      let (_, target_hostname) = self.common.setup_build_context(elevation)?;
      tracing::warn!(
        "Guessing system is {target_hostname} for a VM image. If this isn't \
         intended, use --hostname to change."
      );
    }

    // The VM runs locally, but the target's name still picks the config.
    let mut rebuild = self.common;
    if self.run
      && let Some(target_host) = rebuild.target_host.take()
    {
      rebuild.hostname.get_or_insert_with(|| {
        target_host.hostname_without_domain().to_owned()
      });
    }

    rebuild.build_only(&OsRebuildVariant::BuildVm, Some(&[attr]), elevation)?;

    // If --run flag is set, execute the VM
    if self.run {
      run_vm(&out_path)?;
    }

    Ok(())
  }
}

impl OsRebuildActivateArgs {
  // final_attr is the attribute of config.system.build.X to evaluate.
  fn rebuild_and_activate(
    self,
    variant: &OsRebuildVariant,
    final_attrs: Option<&[&str]>,
    elevation: ElevationStrategy,
  ) -> Result<()> {
    use OsRebuildVariant::{Build, BuildVm};

    let (local_elevate, target_hostname) =
      self.rebuild.setup_build_context(&elevation)?;

    let (out_path, _tempdir_guard) =
      self.rebuild.determine_output_path(variant)?;

    let toplevel = self
      .rebuild
      .resolve_installable_and_toplevel(&target_hostname, final_attrs)?;

    if self.rebuild.update_args.update_all
      || self.rebuild.update_args.update_input.is_some()
    {
      update_with_args(
        &toplevel,
        self.rebuild.update_args.update_input.clone(),
        &self.rebuild.common.passthrough,
      )?;
    }

    let message = match variant {
      BuildVm => "Building NixOS VM image",
      _ => "Building NixOS configuration",
    };

    // Initialize SSH control early if we have remote hosts - guard will keep
    // connections alive for both build and activation
    let _ssh_guard = if self.rebuild.build_host.is_some()
      || self.rebuild.target_host.is_some()
    {
      let guard = nh_remote::init_ssh_control();

      // Pre-establish ControlMaster connections so that delegated SSH
      // invocations (e.g. `nix copy --to ssh://...`) reuse the already-
      // authenticated socket rather than opening a fresh connection where
      // SSH option ordering may differ.
      if let Some(build_host) = &self.rebuild.build_host {
        nh_remote::open_ssh_control_master(build_host)
          .context("Failed to establish SSH connection to build host")?;
      }

      if let Some(target_host) = &self.rebuild.target_host {
        nh_remote::open_ssh_control_master(target_host)
          .context("Failed to establish SSH connection to target host")?;
      }

      Some(guard)
    } else {
      None
    };

    // Now that the ControlMaster is up, probe the remote uid for elevation.
    let elevate = if self.rebuild.target_host.is_some() {
      self.rebuild.determine_remote_elevation(&elevation)?
    } else {
      local_elevate
    };

    let store_path =
      self.rebuild.execute_build(toplevel, &out_path, message)?;

    let target_profile = self
      .rebuild
      .resolve_specialisation_and_profile(&store_path)?;

    print_diff(
      &self.rebuild.common.diff,
      self.rebuild.target_host.as_ref(),
      Path::new(CURRENT_PROFILE),
      &target_profile,
    )?;

    if self.rebuild.common.dry || matches!(variant, Build | BuildVm) {
      if self.rebuild.common.ask {
        warn!("--ask has no effect as dry run was requested");
      }

      // For VM builds, print instructions on how to run the VM
      if matches!(variant, BuildVm) && !self.rebuild.common.dry {
        print_vm_instructions(&out_path);
      }

      return Ok(());
    }

    self.activate_rebuilt_config(
      variant,
      &store_path,
      &target_profile,
      elevate,
      elevation,
    )?;

    Ok(())
  }

  /// Activate `target_profile`, the built `store_path` or one of its
  /// specialisations. Both are in the store of the host being activated.
  fn activate_rebuilt_config(
    &self,
    variant: &OsRebuildVariant,
    store_path: &Path,
    target_profile: &Path,
    elevate: bool,
    elevation: ElevationStrategy,
  ) -> Result<()> {
    use OsRebuildVariant::{
      Boot,
      Build,
      BuildIso,
      BuildVm,
      DryActivate,
      Switch,
      Test,
    };

    if self.rebuild.common.ask && !matches!(variant, DryActivate) {
      let confirmation = inquire::Confirm::new("Apply the config?")
        .with_default(false)
        .prompt()?;

      if !confirmation {
        bail!("User rejected the new config");
      }
    }

    if self.rebuild.no_validate {
      warn!(
        "Skipping pre-activation validation (--no-validate or NH_NO_VALIDATE \
         set)"
      );
      warn!(
        "This may result in activation failures if the system closure is \
         incomplete"
      );
    } else if let Some(target_host) = &self.rebuild.target_host {
      validate_system_closure_remote(
        target_profile,
        target_host,
        self.rebuild.build_host.as_ref(),
      )?;
    } else {
      validate_system_closure(target_profile)?;
    }

    let switch_to_configuration =
      target_profile.join("bin").join("switch-to-configuration");

    let Some(target_host) = &self.rebuild.target_host else {
      let action = match variant {
        Switch => NixosActivationAction::Switch,
        Boot => NixosActivationAction::Boot,
        Test => NixosActivationAction::Test,
        DryActivate => NixosActivationAction::DryActivate,
        #[expect(
          clippy::unreachable,
          reason = "build variants return before activation"
        )]
        Build | BuildVm | BuildIso => unreachable!(),
      };
      let activation = NixosActivation {
        action,
        switch_to_configuration,
        system: store_path.to_owned(),
        continue_on_activation_failure: self.continue_on_activation_failure,
        install_bootloader: self.install_bootloader,
        systemd_run: !self.no_systemd_run,
        nix_args: self.rebuild.common.passthrough.generate_passthrough_args(),
      };
      if elevate {
        PrivilegedOp::NixosActivate(activation).run_elevated(elevation)?;
      } else {
        run_activation_commands(&activation)?;
      }
      return Ok(());
    };

    if let Test | Switch | DryActivate = variant {
      let activation_type = match variant {
        Test => nh_remote::ActivationType::Test,
        Switch => nh_remote::ActivationType::Switch,
        DryActivate => nh_remote::ActivationType::DryActivate,
        #[expect(clippy::unreachable, reason = "guarded by the enclosing if")]
        Boot | Build | BuildVm | BuildIso => unreachable!(),
      };

      let activation_result = nh_remote::activate_remote(
        target_host,
        target_profile,
        &nh_remote::ActivateRemoteConfig {
          platform: nh_remote::Platform::NixOS,
          activation_type,
          install_bootloader: false,
          systemd_run: !self.no_systemd_run,
          elevation: elevate.then_some(elevation.clone()),
        },
      )
      .context(format!("Activation ({}) failed", activation_type.as_str()));

      if let Err(e) = activation_result {
        handle_activation_failure(
          e,
          matches!(variant, Switch),
          self.continue_on_activation_failure,
        )?;
      }
    }

    // The bootloader gets the base system closure instead of the
    // specialisation one. This is what makes all specialisations visible in
    // the bootloader instead of only the generation with the specialisation.
    if let Boot | Switch = variant {
      nh_remote::activate_remote(
        target_host,
        store_path,
        &nh_remote::ActivateRemoteConfig {
          platform:           nh_remote::Platform::NixOS,
          activation_type:    nh_remote::ActivationType::Boot,
          install_bootloader: self.install_bootloader,
          systemd_run:        !self.no_systemd_run,
          elevation:          elevate.then_some(elevation),
        },
      )
      .context("Bootloader activation failed")?;
    }

    debug!(
      "Completed {variant:?} operation with {}",
      target_profile.display()
    );
    Ok(())
  }
}

impl OsRebuildArgs {
  /// Performs initial setup and gathers context for an OS rebuild operation.
  ///
  /// This includes:
  /// - Ensuring SSH key login if a remote build/target host is involved.
  /// - Determining elevation status for local activation; the remote case is
  ///   handled by [`Self::determine_remote_elevation`] after the SSH
  ///   `ControlMaster` is up.
  /// - Performing updates to Nix inputs if specified.
  /// - Resolving the target hostname for the build.
  ///
  /// # Returns
  ///
  /// `Result` containing a tuple:
  ///
  /// - `bool`: `true` if local elevation is required. When `target_host` is set
  ///   this value is meaningless (returned as `false`), and the real answer is
  ///   produced by [`Self::determine_remote_elevation`] once the SSH
  ///   `ControlMaster` is up.
  /// - `String`: The resolved target hostname.
  fn setup_build_context(
    &self,
    elevation: &ElevationStrategy,
  ) -> Result<(bool, String)> {
    // Only check SSH key login if remote hosts are involved
    if self.build_host.is_some() || self.target_host.is_some() {
      ensure_ssh_key_login()?;
    }

    // We still call this for the local-root guard it performs, even though
    // remote-target flows take their elevate answer from
    // `determine_remote_elevation` later.
    let local_elevate =
      has_elevation_status(self.bypass_root_check, elevation)?;
    let elevate = self.target_host.is_none() && local_elevate;

    let target_hostname = get_hostname(
      self
        .hostname
        .as_deref()
        .or_else(|| {
          self
            .target_host
            .as_ref()
            .map(RemoteHost::hostname_without_domain)
        })
        .map(ToOwned::to_owned),
    )?;
    Ok((elevate, target_hostname))
  }

  /// Probe the remote uid to decide whether activation needs elevation.
  ///
  /// This must be called after [`nh_remote::open_ssh_control_master`]
  /// so the probe reuses the established connection.
  ///
  /// # Returns
  ///
  /// `false` when `target_host` is unset (caller should use
  /// [`Self::setup_build_context`] or [`has_elevation_status`] for the
  /// local case) or when the elevation strategy is [`None`].
  fn determine_remote_elevation(
    &self,
    elevation: &ElevationStrategy,
  ) -> Result<bool> {
    let Some(target_host) = &self.target_host else {
      return Ok(false);
    };
    if matches!(elevation, ElevationStrategy::None) {
      return Ok(false);
    }
    let uid = nh_remote::probe_remote_uid(target_host)?;
    Ok(uid != 0)
  }

  fn determine_output_path(
    &self,
    variant: &OsRebuildVariant,
  ) -> Result<(PathBuf, Option<tempfile::TempDir>)> {
    use OsRebuildVariant::{Build, BuildIso, BuildVm};
    if let Some(p) = self.common.out_link.clone() {
      Ok((p, None))
    } else {
      let (path, guard) = if matches!(variant, BuildVm | BuildIso | Build) {
        (PathBuf::from("result"), None)
      } else {
        let dir = tempfile::Builder::new().prefix("nh-os").tempdir()?;
        (dir.as_ref().join("result"), Some(dir))
      };
      Ok((path, guard))
    }
  }

  fn resolve_installable_and_toplevel(
    &self,
    target_hostname: &str,
    final_attrs: Option<&[&str]>,
  ) -> Result<Installable> {
    let installable = self
      .common
      .installable
      .clone()
      .resolve_or_default(CommandContext::Os)?;

    let mut build_attr = vec!["config", "system", "build"];
    build_attr.extend_from_slice(final_attrs.unwrap_or(&["toplevel"]));

    toplevel_for(target_hostname, installable, &build_attr)
  }

  /// Build the configuration onto the host that will activate it, returning
  /// its store path.
  fn execute_build(
    &self,
    toplevel: Installable,
    out_path: &Path,
    message: &str,
  ) -> Result<PathBuf> {
    let build = command::Build::new(toplevel)
      .extra_args(&self.extra_args)
      .passthrough(&self.common.passthrough)
      .message(message)
      .rom(use_rom(self.common.no_rom, self.common.no_nom));

    nh_remote::build(
      build,
      self.build_host.as_ref(),
      self.target_host.as_ref(),
      out_path,
      &self.common.passthrough,
    )
    .context("Failed to build configuration")
  }

  /// Resolve the profile to activate: `store_path`, or one of its
  /// specialisations.
  fn resolve_specialisation_and_profile(
    &self,
    store_path: &Path,
  ) -> Result<PathBuf> {
    let specialisation_path =
      |spec: &str| store_path.join("specialisation").join(spec);

    if self.no_specialisation {
      return Ok(store_path.to_path_buf());
    }

    // A remote target's booted specialisation can't be read from here, and
    // its closure may only exist remotely, so a named specialisation is
    // checked by the remote closure validation instead.
    if self.target_host.is_some() {
      return Ok(
        self
          .specialisation
          .as_deref()
          .map_or_else(|| store_path.to_path_buf(), specialisation_path),
      );
    }

    // Did the user name a specialisation with `--specialisation`, or did we
    // infer it from the running system? A named one that we can't build is a
    // user error. An inferred one that we can't build should fall back instead.
    let booted_specialisation = std::fs::read_to_string(SPEC_LOCATION)
      .ok()
      .map(|s| s.trim().to_owned());
    let Some(spec) = self.specialisation.clone().or(booted_specialisation)
    else {
      return Ok(store_path.to_path_buf());
    };
    debug!("Target specialisation: {spec}");

    let spec_path = specialisation_path(&spec);
    if spec_path.exists() {
      return Ok(spec_path);
    }
    if self.specialisation.is_some() {
      bail!(
        "Specialisation '{spec}' does not exist in the built configuration"
      );
    }

    // Inferred from the running system, but absent from the config being
    // built. This happens with `nh os build --hostname <other>` while booted
    // into a specialisation. Fall back to the base config instead of failing.
    warn!(
      "Booted specialisation '{spec}' does not exist in the built \
       configuration. Falling back to the base configuration. Pass \
       --specialisation to pick one, or --no-specialisation to silence this \
       warning."
    );
    Ok(store_path.to_path_buf())
  }

  fn dry_build(&self) -> Result<()> {
    if self.build_host.is_some() || self.target_host.is_some() {
      bail!(
        "`nh os build --dry` does not support remote build or target hosts"
      );
    }

    let hostname = get_hostname(self.hostname.clone())?;
    let installable = self.resolve_installable_and_toplevel(&hostname, None)?;

    if self.update_args.update_all || self.update_args.update_input.is_some() {
      update_with_args(
        &installable,
        self.update_args.update_input.clone(),
        &self.common.passthrough,
      )?;
    }

    command::Build::new(installable)
      .extra_args(&self.extra_args)
      .passthrough(&self.common.passthrough)
      .dry_run()
      .context("Failed to calculate build plan")
  }

  // final_attr is the attribute of config.system.build.X to evaluate.
  // Used by Build and BuildVm subcommands which don't activate
  fn build_only(
    self,
    variant: &OsRebuildVariant,
    final_attrs: Option<&[&str]>,
    elevation: &ElevationStrategy,
  ) -> Result<()> {
    use OsRebuildVariant::{Build, BuildIso, BuildVm};

    let (_, target_hostname) = self.setup_build_context(elevation)?;

    let (out_path, _tempdir_guard) = self.determine_output_path(variant)?;

    let toplevel = if matches!(variant, BuildVm)
      && !self.no_specialisation
      && let Some(spec) = self.specialisation.as_deref()
    {
      let installable = self
        .common
        .installable
        .clone()
        .resolve_or_default(CommandContext::Os)?;
      let attr = final_attrs
        .and_then(|attrs| attrs.first())
        .context("Missing VM build attribute")?;

      toplevel_for(&target_hostname, installable, &[
        "config",
        "specialisation",
        spec,
        "configuration",
        "system",
        "build",
        attr,
      ])?
    } else {
      self.resolve_installable_and_toplevel(&target_hostname, final_attrs)?
    };

    if self.update_args.update_all || self.update_args.update_input.is_some() {
      update_with_args(
        &toplevel,
        self.update_args.update_input.clone(),
        &self.common.passthrough,
      )?;
    }

    let message = match variant {
      BuildVm => "Building NixOS VM image",
      BuildIso => {
        &final_attrs.and_then(|attrs| attrs.last()).map_or_else(
          || "Building NixOS image".to_string(),
          |variant| format!("Building NixOS image ({variant})"),
        )
      },
      _ => "Building NixOS configuration",
    };

    let store_path = self.execute_build(toplevel, &out_path, message)?;

    let target_profile = if matches!(variant, BuildVm) {
      store_path
    } else {
      self.resolve_specialisation_and_profile(&store_path)?
    };

    print_diff(
      &self.common.diff,
      self.target_host.as_ref(),
      Path::new(CURRENT_PROFILE),
      &target_profile,
    )?;

    // Build, BuildVm and BuildIso subcommands never activate
    debug_assert!(matches!(variant, Build | BuildVm | BuildIso));

    Ok(())
  }
}

impl OsRollbackArgs {
  #[expect(clippy::too_many_lines)]
  fn rollback(&self, elevation: ElevationStrategy) -> Result<()> {
    let elevate = has_elevation_status(self.bypass_root_check, &elevation)?;

    let generations = list_generations()?;

    let selected_generation = fs::read_link(SYSTEM_PROFILE)
      .ok()
      .and_then(|link| generations::from_profile_dir(&link, "system"))
      .and_then(|number| {
        generations
          .iter()
          .find(|generation| generation.number == number)
      });

    // An explicit generation can repair a missing selected profile; only an
    // automatic rollback needs a valid selected generation for comparison.
    let target_generation = if let Some(gen_number) = self.to {
      get_generation_by_number(gen_number, &generations)?
    } else {
      let current_generation = selected_generation
        .context("Selected system profile is not a generation")?;
      &find_previous_generation(current_generation.number, &generations)?
    };

    info!("Rolling back to generation {}", target_generation.number);

    // Construct path to the generation
    let profile_dir = Path::new(SYSTEM_PROFILE).parent().unwrap_or_else(|| {
      tracing::warn!(
        "SYSTEM_PROFILE has no parent, defaulting to /nix/var/nix/profiles"
      );
      Path::new("/nix/var/nix/profiles")
    });
    let generation_link =
      profile_dir.join(format!("system-{}-link", target_generation.number));

    // Handle specialisations
    let current_specialisation = fs::read_to_string(SPEC_LOCATION)
      .ok()
      .map(|s| s.trim().to_owned());

    let target_specialisation = if self.no_specialisation {
      None
    } else {
      self.specialisation.clone().or(current_specialisation)
    };

    debug!("target_specialisation: {target_specialisation:?}");

    // Resolve the target before changing the selected system profile.
    let switch_to_configuration = match &target_specialisation {
      None => generation_link.join("bin/switch-to-configuration"),
      Some(spec) => {
        let spec_path = generation_link.join("specialisation").join(spec);
        if spec_path.exists() {
          spec_path.join("bin/switch-to-configuration")
        } else if self.specialisation.is_some() {
          bail!(
            "Specialisation '{}' does not exist in generation {}",
            spec,
            target_generation.number
          );
        } else {
          warn!(
            "Specialisation '{}' does not exist in generation {}",
            spec, target_generation.number
          );
          warn!("Using base configuration without specialisations");
          generation_link.join("bin/switch-to-configuration")
        }
      },
    };

    if !switch_to_configuration.exists() {
      return Err(missing_switch_to_configuration_error());
    }

    print_diff(
      &self.diff,
      None,
      Path::new(CURRENT_PROFILE),
      &generation_link,
    )?;

    if self.dry {
      info!(
        "Dry run: would roll back to generation {}",
        target_generation.number
      );
      return Ok(());
    }

    if self.ask {
      let confirmation = inquire::Confirm::new(&format!(
        "Roll back to generation {}?",
        target_generation.number
      ))
      .with_default(false)
      .prompt()?;

      if !confirmation {
        bail!("User rejected the rollback");
      }
    }

    let rollback = NixosRollback {
      target_profile: generation_link,
      previous_profile: selected_generation
        .filter(|generation| generation.number > 0)
        .map(|generation| {
          profile_dir.join(format!("system-{}-link", generation.number))
        }),
      switch_to_configuration,
      systemd_run: !self.no_systemd_run,
    };
    if elevate {
      PrivilegedOp::NixosRollback(rollback).run_elevated(elevation)?;
    } else {
      run_rollback_commands(&rollback)?;
    }

    info!(
      "Successfully rolled back to generation {}",
      target_generation.number
    );
    Ok(())
  }
}

impl OsBuildImageArgs {
  fn build_image(self, elevation: &ElevationStrategy) -> Result<()> {
    let (_, target_hostname) = self.common.setup_build_context(elevation)?;
    let display_variant = self.image_variant.as_deref().unwrap_or("isoImage");

    // Show warning if no hostname was explicitly provided for image builds
    if self.common.hostname.is_none() {
      tracing::warn!(
        "Guessing system is {target_hostname} for an image ({}). If this \
         isn't intended, use --hostname to change.",
        display_variant
      );
    }

    let mut installable = self
      .common
      .common
      .installable
      .clone()
      .resolve_or_default(CommandContext::Os)?;

    if self.common.update_args.update_all
      || self.common.update_args.update_input.is_some()
    {
      update_with_args(
        &installable,
        self.common.update_args.update_input.clone(),
        &self.common.common.passthrough,
      )?;
    }

    if matches!(installable, Installable::Store { .. }) {
      bail!("Unsupported installable type for image building")
    }

    let legacy_attribute = if let Some(ref variant) = self.image_variant {
      let (valid_variants, attribute) = match &installable {
        Installable::Flake { .. } => {
          let images_installable =
            toplevel_for(&target_hostname, installable.clone(), &[
              "config", "system", "build", "images",
            ])?;
          let variants = get_build_image_variants_flake_with_args(
            &images_installable,
            self.common.common.passthrough.generate_evaluation_args(),
          )?;
          (variants, None)
        },
        Installable::File { .. } | Installable::Expression { .. } => {
          let images = get_build_image_variants_with_args(
            &installable,
            &target_hostname,
            self
              .common
              .common
              .passthrough
              .generate_legacy_evaluation_args(),
          )?;
          (images.variants, Some(images.attribute))
        },
        #[expect(clippy::unreachable)]
        Installable::Store { .. } => unreachable!("already checked above"),
      };

      // Validate that the requested variant exists
      if !valid_variants.contains(variant) {
        bail!(
          "Invalid image variant '{}'. Available variants:\n- {}",
          variant,
          valid_variants.join("\n- ")
        );
      }

      attribute
    } else {
      None
    };

    if let Some(selected) = legacy_attribute
      && let Installable::File { attribute, .. }
      | Installable::Expression { attribute, .. } = &mut installable
    {
      *attribute = selected;
    }

    if self.common.common.dry {
      info!(
        "Dry run: would build NixOS image '{}' ({:?})",
        display_variant, installable
      );
      return Ok(());
    }

    let mut rebuild = self.common;
    rebuild.update_args.update_all = false;
    rebuild.update_args.update_input = None;
    rebuild.common.installable = InstallableArgs::Specified(installable);

    let attrs = self
      .image_variant
      .as_deref()
      .map_or_else(|| vec!["isoImage"], |variant| vec!["images", variant]);
    rebuild.build_only(&OsRebuildVariant::BuildIso, Some(&attrs), elevation)?;

    Ok(())
  }
}

/// Finds the VM runner script in the given build output directory.
///
/// Searches for a file matching `run-*-vm` in the `bin` subdirectory of
/// `out_path`.
///
/// # Arguments
///
/// * `out_path` - The path to the build output directory (usually `result`).
///
/// # Returns
///
/// * `Ok(PathBuf)` with the path to the VM runner script if found.
/// * `Err` if the script cannot be found or the bin directory is missing.
///
/// # Errors
///
/// Returns an error if the bin directory does not exist or if no matching
/// script is found.
fn find_vm_script(out_path: &Path) -> Result<PathBuf> {
  let bin_dir = out_path.join("bin");

  if !bin_dir.is_dir() {
    bail!(
      "VM build output missing bin directory at {}",
      bin_dir.display()
    );
  }

  let vm_script = fs::read_dir(&bin_dir)
    .with_context(|| format!("Failed to read directory {}", bin_dir.display()))?
    .filter_map(|entry_result| {
      match entry_result {
        Ok(entry) => Some(entry),
        Err(e) => {
          warn!("Error reading entry in {}: {}", bin_dir.display(), e);
          None
        },
      }
    })
    .find_map(|entry| {
      let fname = entry.file_name();
      if fname
        .to_str()
        .is_some_and(|name| name.starts_with("run-") && name.ends_with("-vm"))
      {
        Some(entry.path())
      } else {
        None
      }
    })
    .with_context(|| {
      format!("Could not find VM runner script in {}", bin_dir.display())
    })?;

  Ok(vm_script)
}

/// Prints instructions for running the built VM to the user.
///
/// Attempts to locate the VM runner script in the build output directory and
/// prints a message with the path to the script. If the script cannot be found,
/// prints a warning and a generic path pattern.
///
/// # Arguments
///
/// * `out_path` - The path to the build output directory (usually `result`).
///
/// # Returns
///
/// * `Ok(())` on success.
/// * `Err` if there is an error searching for the VM script.
fn print_vm_instructions(out_path: &Path) {
  match find_vm_script(out_path) {
    Ok(script) => {
      info!(
        "Done. The virtual machine can be started by running {}",
        script.display()
      );
    },
    Err(e) => {
      warn!("VM build completed, but could not find run script: {}", e);
      info!(
        "Done. The virtual machine script should be at {}/bin/run-*-vm",
        out_path.display()
      );
    },
  }
}

/// Runs the built NixOS VM by executing the VM runner script.
///
/// Locates the VM runner script in the build output directory and executes it,
/// streaming its output to the user. Returns an error if the script cannot be
/// found or if execution fails.
///
/// # Arguments
///
/// * `out_path` - The path to the build output directory (usually `result`).
///
/// # Returns
///
/// * `Ok(())` if the VM was started successfully.
/// * `Err` if the script cannot be found or execution fails.
fn run_vm(out_path: &Path) -> Result<()> {
  let vm_script = find_vm_script(out_path)?;

  info!(
    "Running VM... Starting virtual machine with {}",
    vm_script.display()
  );

  Command::new(&vm_script)
    .message("Running VM")
    .show_output(true)
    .with_required_env()
    .run()
    .with_context(|| {
      format!("Failed to run VM script at {}", vm_script.display())
    })?;

  Ok(())
}

/// Validates that essential files exist in the system closure.
///
/// Checks for a few critical files that must be present in a complete NixOS
/// system. This is essentially in-line with what nixos-rebuild-ng checks for.
///
/// - bin/switch-to-configuration: activation script
/// - nixos-version: system version identifier
/// - init: system init script
/// - sw/bin: system path binaries
///
/// # Returns
///
/// `Ok(())` if all files exist, or an error listing missing files.
fn validate_system_closure(system_path: &Path) -> Result<()> {
  let mut missing = Vec::new();
  for (file, description) in ESSENTIAL_FILES {
    let path = system_path.join(file);
    if !path.exists() {
      missing.push(format!("  - {file} ({description})"));
    }
  }

  if !missing.is_empty() {
    let missing_list = missing.join("\n");
    return Err(report!(
      "System closure validation failed. Missing essential files:\n{}\n\nThis \
       typically happens when:\n1. 'system.switch.enable' is set to false in \
       your configuration\n2. The build was incomplete or corrupted\n3. \
       You're using an incomplete derivation\n\nTo fix this:\n1. Check if \
       'system.switch.enable = false' is set and remove it\n2. Rebuild your \
       system configuration\n3. If the problem persists, verify your system \
       closure is complete\n\nSystem path checked: {}",
      missing_list,
      system_path.display()
    ));
  }

  Ok(())
}

/// Validates essential files on a remote host via SSH.
///
/// Similar to [`validate_system_closure`] but executes checks on a remote host.
fn validate_system_closure_remote(
  system_path: &Path,
  target_host: &RemoteHost,
  build_host: Option<&RemoteHost>,
) -> Result<()> {
  // Build context string for error messages
  let context = build_host.map(|build| {
    if build.hostname() == target_host.hostname() {
      "also build host".to_string()
    } else {
      format!("built on '{build}'")
    }
  });

  // Delegate to the generic remote validation function
  nh_remote::validate_closure_remote(
    target_host,
    system_path,
    ESSENTIAL_FILES,
    context.as_deref(),
  )
}

/// Returns an error indicating that the 'switch-to-configuration' binary is
/// missing, along with common reasons and solutions.
#[track_caller]
fn missing_switch_to_configuration_error() -> misstep::Report {
  report!(
    "The 'switch-to-configuration' binary is missing from the built \
     configuration.\n\nThis typically happens when 'system.switch.enable' is \
     set to false in your\nNixOS configuration. To fix this, please \
     either:\n1. Remove 'system.switch.enable = false' from your \
     configuration, or\n2. Set 'system.switch.enable = true' explicitly\n\nIf \
     the problem persists, please open an issue on our issue tracker!"
  )
}

/// Checks if the current user is root and returns whether elevation is needed.
///
/// Returns `true` if elevation is required (not root and `bypass_root_check` is
/// false). Returns `false` if elevation is not required (root or
/// `bypass_root_check` is true).
///
/// # Arguments
///
/// * `bypass_root_check` - If true, bypasses the root check and assumes no
///   elevation is needed.
///
/// # Errors
///
/// Returns an error if `bypass_root_check` is false and the user is root,
/// as `nh os` subcommands should not be run directly as root.
fn has_elevation_status(
  bypass_root_check: bool,
  elevation: &command::ElevationStrategy,
) -> Result<bool> {
  // If elevation strategy is None, never elevate
  if matches!(elevation, command::ElevationStrategy::None) {
    return Ok(false);
  }

  let is_root = nix::unistd::Uid::effective().is_root();

  if is_root && !bypass_root_check {
    bail!(
      "Don't run nh os as root. It will escalate its privileges internally as \
       needed."
    );
  }

  if bypass_root_check {
    warn!(
      "Bypassing root check; running nix as {}",
      if is_root { "root" } else { "non-root" }
    );
  }

  Ok(!is_root)
}

fn find_previous_generation(
  current_number: u64,
  generations: &[generations::GenerationInfo],
) -> Result<generations::GenerationInfo> {
  let Some(current_idx) =
    generations.iter().position(|g| g.number == current_number)
  else {
    bail!("Current generation not found");
  };

  if current_idx == 0 {
    bail!("No generation older than the current one exists");
  }

  let previous_generation = generations[current_idx - 1].clone();

  Ok(previous_generation)
}

fn get_generation_by_number(
  number: u64,
  generations: &[generations::GenerationInfo],
) -> Result<&generations::GenerationInfo> {
  generations
    .iter()
    .find(|g| g.number == number)
    .with_context(|| format!("Generation {number} not found"))
}

fn list_generations() -> Result<Vec<generations::GenerationInfo>> {
  let profile_path = PathBuf::from(SYSTEM_PROFILE);
  let profiles_dir = profile_path
    .parent()
    .unwrap_or_else(|| Path::new("/nix/var/nix/profiles"));

  let mut generations = Vec::new();
  for entry in fs::read_dir(profiles_dir)? {
    let entry = match entry {
      Ok(e) => e,
      Err(e) => {
        warn!("Failed to read entry in profile directory: {}", e);
        continue;
      },
    };

    let path = entry.path();
    if generations::from_profile_dir(&path, "system").is_some()
      && let Some(gen_info) = generations::describe(&path, None)
    {
      generations.push(gen_info);
    }
  }

  if generations.is_empty() {
    bail!("No generations found");
  }

  tracing::debug!("{} generations found", generations.len());

  generations.sort_by_key(|g| g.number);

  Ok(generations)
}

/// Resolve a NixOS installable to an attribute path within the selected
/// configuration.
///
/// # Errors
///
/// Returns an error if the flake attribute path is too specific to infer the
/// requested build attributes.
pub fn toplevel_for<S: AsRef<str>>(
  hostname: S,
  installable: Installable,
  build_attr: &[&str],
) -> Result<Installable> {
  let mut res = installable;
  res.resolve_configuration(
    ConfigurationLayout {
      set: "nixosConfigurations",
      build_attr,
    },
    Some(hostname.as_ref()),
  )?;

  Ok(res)
}

impl OsReplArgs {
  fn run(self) -> Result<()> {
    let mut target_installable =
      self.installable.resolve_or_default(CommandContext::Os)?;

    if matches!(target_installable, Installable::Store { .. }) {
      bail!("Nix doesn't support nix store installables.");
    }

    let hostname = get_hostname(self.hostname)?;

    // Enter the repl at the configuration itself, without a build attribute.
    target_installable.resolve_configuration(
      ConfigurationLayout {
        set:        "nixosConfigurations",
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

impl OsGenerationsArgs {
  fn info(&self) -> Result<()> {
    let profile = match self.profile {
      Some(ref p) => PathBuf::from(p),
      None => bail!("Profile path is required"),
    };

    if !profile.is_symlink() {
      return Err(report!(
        "No profile `{:?}` found",
        profile.file_name().unwrap_or_default()
      ));
    }

    let profile_dir = profile.parent().unwrap_or_else(|| Path::new("."));
    let profile_name = profile
      .file_name()
      .and_then(|name| name.to_str())
      .context("Profile name contains invalid UTF-8")?;

    let generations: Vec<_> = fs::read_dir(profile_dir)?
      .filter_map(|entry| {
        let path = entry.ok()?.path();
        generations::from_profile_dir(&path, profile_name).map(|_| path)
      })
      .collect();

    let gen_dir_refs: Vec<&std::path::Path> =
      generations.iter().map(PathBuf::as_path).collect();
    let closure_sizes = generations::get_closure_sizes_batch(&gen_dir_refs);

    let descriptions: Vec<generations::GenerationInfo> = generations
      .iter()
      .filter_map(|gen_dir| {
        let size = closure_sizes.get(gen_dir).cloned();
        generations::describe(gen_dir, size)
      })
      .collect();

    let boot_generation = fs::read_link(&profile)
      .ok()
      .and_then(|link| generations::from_profile_dir(&link, profile_name));

    if self.json {
      return generations::print_json(&descriptions, boot_generation);
    }

    generations::print_info(
      descriptions,
      self.fields.as_deref(),
      boot_generation,
    )?;

    Ok(())
  }
}

use std::{
  convert::Into,
  env,
  fs,
  path::{Path, PathBuf},
};

use color_eyre::eyre::{Context, Result, bail, eyre};
use tracing::{debug, info, warn};

use crate::{
  commands::{self, Command, ElevationStrategy},
  generations,
  installable::Installable,
  interface::{
    self,
    DiffType,
    OsBuildVmArgs,
    OsGenerationsArgs,
    OsRebuildActivateArgs,
    OsRebuildArgs,
    OsReplArgs,
    OsRollbackArgs,
    OsSubcommand::{self},
  },
  remote::{self, RemoteBuildConfig, RemoteHost},
  update::update,
  util::{ensure_ssh_key_login, get_hostname, print_dix_diff},
};

const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
const CURRENT_PROFILE: &str = "/run/current-system";

const SPEC_LOCATION: &str = "/etc/specialisation";

impl interface::OsArgs {
  #[cfg_attr(feature = "hotpath", hotpath::measure)]
  pub fn run(self, elevation: ElevationStrategy) -> Result<()> {
    use OsRebuildVariant::{Boot, Build, Switch, Test};
    match self.subcommand {
      OsSubcommand::Boot(args) => {
        args.rebuild_and_activate(&Boot, None, elevation)
      },
      OsSubcommand::Test(args) => {
        args.rebuild_and_activate(&Test, None, elevation)
      },
      OsSubcommand::Switch(args) => {
        args.rebuild_and_activate(&Switch, None, elevation)
      },
      OsSubcommand::Build(args) => {
        if args.common.ask || args.common.dry {
          warn!("`--ask` and `--dry` have no effect for `nh os build`");
        }
        args.build_only(&Build, None, elevation)
      },
      OsSubcommand::BuildVm(args) => args.build_vm(elevation),
      OsSubcommand::Repl(args) => args.run(),
      OsSubcommand::Info(args) => args.info(),
      OsSubcommand::Rollback(args) => args.rollback(elevation),
    }
  }
}

#[derive(Debug)]
enum OsRebuildVariant {
  Build,
  Switch,
  Boot,
  Test,
  BuildVm,
}

impl OsBuildVmArgs {
  fn build_vm(self, elevation: ElevationStrategy) -> Result<()> {
    let attr = if self.with_bootloader {
      "vmWithBootLoader".to_owned()
    } else {
      "vm".to_owned()
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
      let (_, target_hostname) = self.common.setup_build_context()?;
      tracing::warn!(
        "Guessing system is {target_hostname} for a VM image. If this isn't \
         intended, use --hostname to change."
      );
    }

    self.common.build_only(
      &OsRebuildVariant::BuildVm,
      Some(&attr),
      elevation,
    )?;

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
    final_attr: Option<&String>,
    elevation: ElevationStrategy,
  ) -> Result<()> {
    use OsRebuildVariant::{Build, BuildVm};

    let (elevate, target_hostname) = self.rebuild.setup_build_context()?;

    let (out_path, _tempdir_guard) =
      self.rebuild.determine_output_path(variant)?;

    let toplevel = self
      .rebuild
      .resolve_installable_and_toplevel(&target_hostname, final_attr)?;

    let message = match variant {
      BuildVm => "Building NixOS VM image",
      _ => "Building NixOS configuration",
    };

    self.rebuild.execute_build(toplevel, &out_path, message)?;

    let target_profile =
      self.rebuild.resolve_specialisation_and_profile(&out_path)?;

    self.rebuild.handle_dix_diff(&target_profile);

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
      &out_path,
      &target_profile,
      elevate,
      elevation,
    )?;

    Ok(())
  }

  fn activate_rebuilt_config(
    &self,
    variant: &OsRebuildVariant,
    out_path: &Path,
    target_profile: &Path,
    elevate: bool,
    elevation: ElevationStrategy,
  ) -> Result<()> {
    use OsRebuildVariant::{Boot, Switch, Test};

    if self.rebuild.common.ask {
      let confirmation = inquire::Confirm::new("Apply the config?")
        .with_default(false)
        .prompt()?;

      if !confirmation {
        bail!("User rejected the new config");
      }
    }

    if let Some(target_host) = &self.rebuild.target_host {
      Command::new("nix")
        .args([
          "copy",
          "--to",
          format!("ssh://{target_host}").as_str(),
          match target_profile.to_str() {
            Some(s) => s,
            None => {
              return Err(eyre!("target_profile path is not valid UTF-8"));
            },
          },
        ])
        .message("Copying configuration to target")
        .with_required_env()
        .run()?;
    }

    let switch_to_configuration = target_profile
      .canonicalize()
      .context("Failed to resolve output path")?
      .join("bin")
      .join("switch-to-configuration")
      .canonicalize()
      .context("Failed to resolve switch-to-configuration path")?;

    if !switch_to_configuration.exists() {
      return Err(missing_switch_to_configuration_error());
    }

    let canonical_out_path =
      switch_to_configuration.to_str().ok_or_else(|| {
        eyre!("switch-to-configuration path contains invalid UTF-8")
      })?;

    if let Test | Switch = variant {
      let variant_label = match variant {
        Test => "test",
        Switch => "switch",
        _ => unreachable!(),
      };

      Command::new(canonical_out_path)
        .arg("test")
        .ssh(self.rebuild.target_host.clone())
        .message("Activating configuration")
        .elevate(elevate.then_some(elevation.clone()))
        .preserve_envs(["NIXOS_INSTALL_BOOTLOADER"])
        .with_required_env()
        .show_output(self.show_activation_logs)
        .run()
        .wrap_err(format!("Activation ({variant_label}) failed"))?;

      debug!("Completed {variant:?} operation with output path: {out_path:?}");
    }

    if let Boot | Switch = variant {
      Command::new("nix")
        .elevate(elevate.then_some(elevation.clone()))
        .args(["build", "--no-link", "--profile", SYSTEM_PROFILE])
        .arg(canonical_out_path)
        .ssh(self.rebuild.target_host.clone())
        .with_required_env()
        .run()
        .wrap_err("Failed to set system profile")?;

      let mut cmd = Command::new(switch_to_configuration)
        .arg("boot")
        .ssh(self.rebuild.target_host.clone())
        .elevate(elevate.then_some(elevation))
        .message("Adding configuration to bootloader")
        .preserve_envs(["NIXOS_INSTALL_BOOTLOADER"]);

      if self.rebuild.install_bootloader {
        cmd = cmd.set_env("NIXOS_INSTALL_BOOTLOADER", "1");
      }

      cmd
        .with_required_env()
        .run()
        .wrap_err("Bootloader activation failed")?;
    }

    debug!("Completed {variant:?} operation with output path: {out_path:?}");
    Ok(())
  }
}

impl OsRebuildArgs {
  /// Performs initial setup and gathers context for an OS rebuild operation.
  ///
  /// This includes:
  /// - Ensuring SSH key login if a remote build/target host is involved.
  /// - Checking and determining elevation status.
  /// - Performing updates to Nix inputs if specified.
  /// - Resolving the target hostname for the build.
  ///
  /// # Returns
  ///
  /// `Result` containing a tuple:
  ///
  /// - `bool`: `true` if elevation is required, `false` otherwise.
  /// - `String`: The resolved target hostname.
  fn setup_build_context(&self) -> Result<(bool, String)> {
    if self.build_host.is_some() || self.target_host.is_some() {
      // This can fail, we only care about prompting the user
      // for ssh key login beforehand.
      let _ = ensure_ssh_key_login();
    }

    let elevate = has_elevation_status(self.bypass_root_check)?;

    if self.update_args.update_all || self.update_args.update_input.is_some() {
      update(
        &self.common.installable,
        self.update_args.update_input.clone(),
      )?;
    }

    let target_hostname = get_hostname(self.hostname.clone())?;
    Ok((elevate, target_hostname))
  }

  fn determine_output_path(
    &self,
    variant: &OsRebuildVariant,
  ) -> Result<(PathBuf, Option<tempfile::TempDir>)> {
    use OsRebuildVariant::{Build, BuildVm};
    if let Some(p) = self.common.out_link.clone() {
      Ok((p, None))
    } else {
      let (path, guard) = if matches!(variant, BuildVm | Build) {
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
    final_attr: Option<&String>,
  ) -> Result<Installable> {
    let installable = (get_nh_os_flake_env()?)
      .unwrap_or_else(|| self.common.installable.clone());

    let installable = match installable {
      Installable::Unspecified => Installable::try_find_default_for_os()?,
      other => other,
    };

    toplevel_for(
      target_hostname,
      installable,
      final_attr.map_or("toplevel", |v| v),
    )
  }

  fn execute_build(
    &self,
    toplevel: Installable,
    out_path: &Path,
    message: &str,
  ) -> Result<()> {
    // If a build host is specified, use proper remote build semantics:
    //
    // 1. Evaluate derivation locally
    // 2. Copy derivation to build host (user-initiated SSH)
    // 3. Build on remote host
    // 4. Copy result back (to localhost or target_host)
    if let Some(ref build_host_str) = self.build_host {
      info!("{message}");

      let build_host = RemoteHost::parse(build_host_str)
        .wrap_err("Invalid build host specification")?;

      let target_host = self
        .target_host
        .as_ref()
        .map(|s| RemoteHost::parse(s))
        .transpose()
        .wrap_err("Invalid target host specification")?;

      let config = RemoteBuildConfig {
        build_host,
        target_host,
        use_nom: !self.common.no_nom,
        use_substitutes: self.common.passthrough.use_substitutes,
        extra_args: self
          .extra_args
          .iter()
          .map(Into::into)
          .chain(
            self
              .common
              .passthrough
              .generate_passthrough_args()
              .into_iter()
              .map(Into::into),
          )
          .collect(),
      };

      remote::check_remote_connectivity(&config)?;

      remote::build_remote(&toplevel, &config, Some(out_path))?;

      Ok(())
    } else {
      // Local build - use the existing path
      commands::Build::new(toplevel)
        .extra_arg("--out-link")
        .extra_arg(out_path)
        .extra_args(&self.extra_args)
        .passthrough(&self.common.passthrough)
        .message(message)
        .nom(!self.common.no_nom)
        .run()
        .wrap_err("Failed to build configuration")
    }
  }

  fn resolve_specialisation_and_profile(
    &self,
    out_path: &Path,
  ) -> Result<PathBuf> {
    let current_specialisation = std::fs::read_to_string(SPEC_LOCATION).ok();

    let target_specialisation = if self.no_specialisation {
      None
    } else {
      current_specialisation.or_else(|| self.specialisation.clone())
    };

    debug!("Target specialisation: {target_specialisation:?}");

    let target_profile = target_specialisation.as_ref().map_or_else(
      || out_path.to_path_buf(),
      |spec| out_path.join("specialisation").join(spec),
    );

    debug!("Output path: {out_path:?}");
    debug!("Target profile path: {}", target_profile.display());
    debug!("Target profile exists: {}", target_profile.exists());

    if !target_profile
      .try_exists()
      .context("Failed to check if target profile exists")?
    {
      return Err(eyre!(
        "Target profile path does not exist: {}",
        target_profile.display()
      ));
    }

    Ok(target_profile)
  }

  fn handle_dix_diff(&self, target_profile: &Path) {
    match self.common.diff {
      DiffType::Always => {
        let _ = print_dix_diff(&PathBuf::from(CURRENT_PROFILE), target_profile);
      },
      DiffType::Never => {
        debug!("Not running dix as the --diff flag is set to never.");
      },
      DiffType::Auto => {
        // Only run dix if no explicit hostname was provided and no remote
        // build/target host is specified, implying a local system build.
        if self.hostname.is_none()
          && self.target_host.is_none()
          && self.build_host.is_none()
        {
          debug!(
            "Comparing with target profile: {}",
            target_profile.display()
          );
          let _ =
            print_dix_diff(&PathBuf::from(CURRENT_PROFILE), target_profile);
        } else {
          debug!(
            "Not running dix as a remote host is involved or an explicit \
             hostname was provided."
          );
        }
      },
    }
  }

  // final_attr is the attribute of config.system.build.X to evaluate.
  // Used by Build and BuildVm subcommands which don't activate
  fn build_only(
    self,
    variant: &OsRebuildVariant,
    final_attr: Option<&String>,
    _elevation: ElevationStrategy,
  ) -> Result<()> {
    use OsRebuildVariant::{Build, BuildVm};

    let (_, target_hostname) = self.setup_build_context()?;

    let (out_path, _tempdir_guard) = self.determine_output_path(variant)?;

    let toplevel =
      self.resolve_installable_and_toplevel(&target_hostname, final_attr)?;

    let message = match variant {
      BuildVm => "Building NixOS VM image",
      _ => "Building NixOS configuration",
    };

    self.execute_build(toplevel, &out_path, message)?;

    let target_profile = self.resolve_specialisation_and_profile(&out_path)?;

    self.handle_dix_diff(&target_profile);

    // Build and BuildVm subcommands never activate
    debug_assert!(matches!(variant, Build | BuildVm));

    Ok(())
  }
}

impl OsRollbackArgs {
  #[expect(clippy::too_many_lines)]
  fn rollback(&self, elevation: ElevationStrategy) -> Result<()> {
    let elevate = has_elevation_status(self.bypass_root_check)?;

    // Find previous generation or specific generation
    let target_generation = if let Some(gen_number) = self.to {
      find_generation_by_number(gen_number)?
    } else {
      find_previous_generation()?
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
    let current_specialisation = fs::read_to_string(SPEC_LOCATION).ok();

    let target_specialisation = if self.no_specialisation {
      None
    } else {
      self.specialisation.clone().or(current_specialisation)
    };

    debug!("target_specialisation: {target_specialisation:?}");

    // Compare changes between current and target generation
    if matches!(self.diff, DiffType::Never) {
      debug!(
        "Not running dix as the target hostname is different from the system \
         hostname."
      );
    } else {
      debug!(
        "Comparing with target profile: {}",
        generation_link.display()
      );
      let _ = print_dix_diff(&PathBuf::from(CURRENT_PROFILE), &generation_link);
    }

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

    // Get current generation number for potential rollback
    let current_gen_number = match get_current_generation_number() {
      Ok(num) => num,
      Err(e) => {
        warn!("Failed to get current generation number: {}", e);
        0
      },
    };

    // Set the system profile
    info!("Setting system profile...");

    // Instead of direct symlink operations, use a command with proper elevation
    Command::new("ln")
            .arg("-sfn") // force, symbolic link
            .arg(&generation_link)
            .arg(SYSTEM_PROFILE)
            .elevate(elevate.then_some(elevation.clone()))
            .message("Setting system profile")
            .with_required_env()
            .run()
            .wrap_err("Failed to set system profile during rollback")?;

    // Determine the correct profile to use with specialisations
    let final_profile = match &target_specialisation {
      None => generation_link,
      Some(spec) => {
        let spec_path = generation_link.join("specialisation").join(spec);
        if spec_path.exists() {
          spec_path
        } else {
          warn!(
            "Specialisation '{}' does not exist in generation {}",
            spec, target_generation.number
          );
          warn!("Using base configuration without specialisations");
          generation_link
        }
      },
    };

    // Activate the configuration
    info!("Activating...");

    let switch_to_configuration =
      final_profile.join("bin").join("switch-to-configuration");

    if !switch_to_configuration.exists() {
      return Err(missing_switch_to_configuration_error());
    }

    match Command::new(&switch_to_configuration)
      .arg("switch")
      .elevate(elevate.then_some(elevation.clone()))
      .preserve_envs(["NIXOS_INSTALL_BOOTLOADER"])
      .with_required_env()
      .run()
    {
      Ok(()) => {
        info!(
          "Successfully rolled back to generation {}",
          target_generation.number
        );
      },
      Err(e) => {
        // If activation fails, rollback the profile
        if current_gen_number > 0 {
          let current_gen_link =
            profile_dir.join(format!("system-{current_gen_number}-link"));

          Command::new("ln")
                        .arg("-sfn") // Force, symbolic link
                        .arg(&current_gen_link)
                        .arg(SYSTEM_PROFILE)
                        .elevate(elevate.then_some(elevation))
                        .message("Rolling back system profile")
                        .with_required_env()
                        .run()
                        .wrap_err("NixOS: Failed to restore previous system profile after failed activation")?;
        }

        return Err(eyre!("Activation (switch) failed: {}", e))
          .context("Failed to activate configuration");
      },
    }

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
    .wrap_err_with(|| {
      format!("Failed to read directory {}", bin_dir.display())
    })?
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
    .ok_or_else(|| {
      eyre!("Could not find VM runner script in {}", bin_dir.display())
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
    .wrap_err_with(|| {
      format!("Failed to run VM script at {}", vm_script.display())
    })?;

  Ok(())
}

/// Returns an error indicating that the 'switch-to-configuration' binary is
/// missing, along with common reasons and solutions.
fn missing_switch_to_configuration_error() -> color_eyre::eyre::Report {
  eyre!(
    "The 'switch-to-configuration' binary is missing from the built \
     configuration.\n\nThis typically happens when 'system.switch.enable' is \
     set to false in your\nNixOS configuration. To fix this, please \
     either:\n1. Remove 'system.switch.enable = false' from your \
     configuration, or\n2. Set 'system.switch.enable = true' explicitly\n\nIf \
     the problem persists, please open an issue on our issue tracker!"
  )
}

/// Parses the `NH_OS_FLAKE` environment variable into an `Installable::Flake`.
///
/// If `NH_OS_FLAKE` is not set, it returns `Ok(None)`.
/// If `NH_OS_FLAKE` is set but invalid, it returns an `Err`.
fn get_nh_os_flake_env() -> Result<Option<Installable>> {
  if let Ok(os_flake) = env::var("NH_OS_FLAKE") {
    debug!("Using NH_OS_FLAKE: {}", os_flake);

    let mut elems = os_flake.splitn(2, '#');
    let reference = elems
      .next()
      .ok_or_else(|| eyre!("NH_OS_FLAKE missing reference part"))?
      .to_owned();
    let attribute = elems
      .next()
      .map(crate::installable::parse_attribute)
      .unwrap_or_default();

    Ok(Some(Installable::Flake {
      reference,
      attribute,
    }))
  } else {
    Ok(None)
  }
}

/// Checks if the current user is root and returns whether elevation is needed.
///
/// Returns `true` if elevation is required (not root and `bypass_root_check` is
/// false). Returns `false` if elevation is not required (root or
/// `bypass_root_check` is true).
///
/// # Arguments
/// * `bypass_root_check` - If true, bypasses the root check and assumes no
///   elevation is needed.
///
/// # Errors
/// Returns an error if `bypass_root_check` is false and the user is root,
/// as `nh os` subcommands should not be run directly as root.
fn has_elevation_status(bypass_root_check: bool) -> Result<bool> {
  if bypass_root_check {
    warn!("Bypassing root check, now running nix as root");
    Ok(false)
  } else {
    if nix::unistd::Uid::effective().is_root() {
      bail!(
        "Don't run nh os as root. It will escalate its privileges internally \
         as needed."
      );
    }
    Ok(true)
  }
}

fn find_previous_generation() -> Result<generations::GenerationInfo> {
  let generations = list_generations()?;
  if generations.is_empty() {
    bail!("No generations found");
  }

  let current_idx = get_current_generation_number()? as usize;

  if current_idx == 0 {
    bail!("No generation older than the current one exists");
  }

  Ok(generations[current_idx - 1].clone())
}

fn find_generation_by_number(
  number: u64,
) -> Result<generations::GenerationInfo> {
  list_generations()?
    .into_iter()
    .find(|g| g.number == number.to_string())
    .ok_or_else(|| eyre!("Generation {} not found", number))
}

fn get_current_generation_number() -> Result<u64> {
  let generations = list_generations()?;
  let current_gen = generations
    .iter()
    .find(|g| g.current)
    .ok_or_else(|| eyre!("Current generation not found"))?;

  current_gen
    .number
    .parse::<u64>()
    .wrap_err("Invalid generation number")
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
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
      if name.starts_with("system-") && name.ends_with("-link") {
        if let Some(gen_info) = generations::describe(&path) {
          generations.push(gen_info);
        }
      }
    }
  }

  if generations.is_empty() {
    bail!("No generations found");
  }

  generations.sort_by_key(|g| g.number.parse::<u64>().unwrap_or(0));

  Ok(generations)
}

pub fn toplevel_for<S: AsRef<str>>(
  hostname: S,
  installable: Installable,
  final_attr: &str,
) -> Result<Installable> {
  let mut res = installable;
  let hostname_str = hostname.as_ref();

  let toplevel = ["config", "system", "build", final_attr]
    .into_iter()
    .map(String::from);

  match res {
    Installable::Flake {
      ref mut attribute, ..
    } => {
      if attribute.is_empty() {
        attribute.push(String::from("nixosConfigurations"));
        attribute.push(hostname_str.to_owned());
      } else if attribute.len() == 1 && attribute[0] == "nixosConfigurations" {
        info!(
          "Inferring hostname '{}' for nixosConfigurations",
          hostname_str
        );
        attribute.push(hostname_str.to_owned());
      } else if attribute[0] == "nixosConfigurations" {
        if attribute.len() == 2 {
          // nixosConfigurations.hostname - fine
        } else if attribute.len() > 2 {
          bail!(
            "Attribute path is too specific: {}. Please either:\n  1. Use the \
             flake reference without attributes (e.g., '.')\n  2. Specify \
             only the configuration name (e.g., '.#{}')",
            attribute.join("."),
            attribute[1]
          );
        }
      } else {
        // User provided ".#myhost" - prepend nixosConfigurations
        attribute.insert(0, String::from("nixosConfigurations"));
      }
      attribute.extend(toplevel);
    },
    Installable::File {
      ref mut attribute, ..
    }
    | Installable::Expression {
      ref mut attribute, ..
    } => attribute.extend(toplevel),

    Installable::Store { .. } => {},

    Installable::Unspecified => {
      unreachable!(
        "Unspecified installable should have been resolved before calling \
         toplevel_for"
      )
    },
  }

  Ok(res)
}

impl OsReplArgs {
  fn run(self) -> Result<()> {
    // Use NH_OS_FLAKE if available, otherwise use the provided installable
    let target_installable =
      if let Some(flake_installable) = get_nh_os_flake_env()? {
        flake_installable
      } else {
        self.installable
      };

    let mut target_installable = match target_installable {
      Installable::Unspecified => Installable::try_find_default_for_os()?,
      other => other,
    };

    if matches!(target_installable, Installable::Store { .. }) {
      bail!("Nix doesn't support nix store installables.");
    }

    let hostname = get_hostname(self.hostname)?;

    if let Installable::Flake {
      ref mut attribute, ..
    } = target_installable
    {
      if attribute.is_empty() {
        attribute.push(String::from("nixosConfigurations"));
        attribute.push(hostname);
      }
    }

    Command::new("nix")
      .arg("repl")
      .args(target_installable.to_args())
      .with_required_env()
      .show_output(true)
      .run()?;

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
      return Err(eyre!(
        "No profile `{:?}` found",
        profile.file_name().unwrap_or_default()
      ));
    }

    let profile_dir = profile.parent().unwrap_or_else(|| Path::new("."));

    let generations: Vec<_> = fs::read_dir(profile_dir)?
      .filter_map(|entry| {
        entry.ok().and_then(|e| {
          let path = e.path();
          if path
            .file_name()?
            .to_str()?
            .starts_with(profile.file_name()?.to_str()?)
          {
            Some(path)
          } else {
            None
          }
        })
      })
      .collect();

    let descriptions: Vec<generations::GenerationInfo> = generations
      .iter()
      .filter_map(|gen_dir| generations::describe(gen_dir))
      .collect();

    generations::print_info(descriptions, self.fields.as_deref())?;

    Ok(())
  }
}

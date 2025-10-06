use std::{
  env,
  fs,
  path::{Path, PathBuf},
};

use color_eyre::eyre::{Context, Result, bail, eyre};
use tracing::{debug, info, warn};

use crate::{
  commands,
  commands::{Command, ElevationStrategy},
  generations,
  installable::Installable,
  interface::{
    self,
    DiffType,
    OsBuildVmArgs,
    OsGenerationsArgs,
    OsRebuildArgs,
    OsReplArgs,
    OsRollbackArgs,
    OsSubcommand::{self},
  },
  update::update,
  util::{ensure_ssh_key_login, get_resolved_hostname, print_dix_diff},
};

const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
const CURRENT_PROFILE: &str = "/run/current-system";

const SPEC_LOCATION: &str = "/etc/specialisation";

impl interface::OsArgs {
  pub fn run(self, elevation: ElevationStrategy) -> Result<()> {
    use OsRebuildVariant::{Boot, Build, Switch, Test};
    match self.subcommand {
      OsSubcommand::Boot(args) => args.rebuild(&Boot, None, elevation),
      OsSubcommand::Test(args) => args.rebuild(&Test, None, elevation),
      OsSubcommand::Switch(args) => args.rebuild(&Switch, None, elevation),
      OsSubcommand::Build(args) => {
        if args.common.ask || args.common.dry {
          warn!("`--ask` and `--dry` have no effect for `nh os build`");
        }
        args.rebuild(&Build, None, elevation)
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
    self
      .common
      .rebuild(&OsRebuildVariant::BuildVm, Some(attr), elevation)?;

    // If --run flag is set, execute the VM
    if self.run {
      run_vm(&out_path)?;
    }

    Ok(())
  }
}

impl OsRebuildArgs {
  // final_attr is the attribute of config.system.build.X to evaluate.
  #[expect(clippy::cognitive_complexity, clippy::too_many_lines)]
  fn rebuild(
    self,
    variant: &OsRebuildVariant,
    final_attr: Option<String>,
    elevation: ElevationStrategy,
  ) -> Result<()> {
    use OsRebuildVariant::{Boot, Build, BuildVm, Switch, Test};

    if self.build_host.is_some() || self.target_host.is_some() {
      // This can fail, we only care about prompting the user
      // for ssh key login beforehand.
      let _ = ensure_ssh_key_login();
    }

    let elevate = check_and_get_elevation_status(self.bypass_root_check)?;

    if self.update_args.update_all || self.update_args.update_input.is_some() {
      update(&self.common.installable, self.update_args.update_input)?;
    }

    let target_hostname = get_resolved_hostname(self.hostname.clone())?;

    // Only show the warning if we're explicitly building a VM
    // and no hostname was explicitly provided (--hostname was None)
    if self.hostname.is_none()
      && matches!(variant, OsRebuildVariant::BuildVm)
      && final_attr
        .as_deref()
        .is_some_and(|attr| attr == "vm" || attr == "vmWithBootLoader")
    {
      tracing::warn!(
        "Guessing system is {} for a VM image. If this isn't intended, use \
         --hostname to change.",
        target_hostname
      );
    }

    let (out_path, _tempdir_guard): (PathBuf, Option<tempfile::TempDir>) =
      match self.common.out_link {
        Some(ref p) => (p.clone(), None),
        None => {
          match variant {
            BuildVm | Build => (PathBuf::from("result"), None),
            _ => {
              let dir = tempfile::Builder::new().prefix("nh-os").tempdir()?;
              (dir.as_ref().join("result"), Some(dir))
            },
          }
        },
      };

    debug!("Output path: {out_path:?}");

    // Use NH_OS_FLAKE if available, otherwise use the provided installable
    let installable = if let Some(flake_installable) = parse_nh_os_flake_env()?
    {
      flake_installable
    } else {
      self.common.installable.clone()
    };

    let toplevel = toplevel_for(
      &target_hostname,
      installable,
      final_attr
        .unwrap_or_else(|| String::from("toplevel"))
        .as_str(),
    );

    let message = match variant {
      BuildVm => "Building NixOS VM image",
      _ => "Building NixOS configuration",
    };

    commands::Build::new(toplevel)
      .extra_arg("--out-link")
      .extra_arg(&out_path)
      .extra_args(&self.extra_args)
      .passthrough(&self.common.passthrough)
      .builder(self.build_host.clone())
      .message(message)
      .nom(!self.common.no_nom)
      .run()
      .wrap_err("Failed to build configuration")?;

    let current_specialisation = std::fs::read_to_string(SPEC_LOCATION).ok();

    let target_specialisation = if self.no_specialisation {
      None
    } else {
      current_specialisation.or_else(|| self.specialisation.clone())
    };

    debug!("Target specialisation: {target_specialisation:?}");

    let target_profile = target_specialisation.as_ref().map_or_else(
      || out_path.clone(),
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

    match self.common.diff {
      DiffType::Always => {
        let _ =
          print_dix_diff(&PathBuf::from(CURRENT_PROFILE), &target_profile);
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
            print_dix_diff(&PathBuf::from(CURRENT_PROFILE), &target_profile);
        } else {
          debug!(
            "Not running dix as a remote host is involved or an explicit \
             hostname was provided."
          );
        }
      },
    }

    if self.common.dry || matches!(variant, Build | BuildVm) {
      if self.common.ask {
        warn!("--ask has no effect as dry run was requested");
      }

      // For VM builds, print instructions on how to run the VM
      if matches!(variant, BuildVm) && !self.common.dry {
        print_vm_instructions(&out_path)?;
      }

      return Ok(());
    }

    if self.common.ask {
      let confirmation = inquire::Confirm::new("Apply the config?")
        .with_default(false)
        .prompt()?;

      if !confirmation {
        bail!("User rejected the new config");
      }
    }

    if let Some(target_host) = &self.target_host {
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
      return Err(eyre!(
        "The 'switch-to-configuration' binary is missing from the built \
         configuration.\n\nThis typically happens when 'system.switch.enable' \
         is set to false in your\nNixOS configuration. To fix this, please \
         either:\n1. Remove 'system.switch.enable = false' from your \
         configuration, or\n2. Set 'system.switch.enable = true' \
         explicitly\n\nIf the problem persists, please open an issue on our \
         issue tracker!"
      ));
    }

    let canonical_out_path =
      switch_to_configuration.to_str().ok_or_else(|| {
        eyre!("switch-to-configuration path contains invalid UTF-8")
      })?;

    if let Test | Switch = variant {
      Command::new(canonical_out_path)
        .arg("test")
        .ssh(self.target_host.clone())
        .message("Activating configuration")
        .elevate(elevate.then_some(elevation.clone()))
        .preserve_envs(["NIXOS_INSTALL_BOOTLOADER"])
        .with_required_env()
        .run()
        .wrap_err("Activation (test) failed")?;

      debug!("Completed {variant:?} operation with output path: {out_path:?}");
    }

    if let Boot | Switch = variant {
      Command::new("nix")
        .elevate(elevate.then_some(elevation.clone()))
        .args(["build", "--no-link", "--profile", SYSTEM_PROFILE])
        .arg(canonical_out_path)
        .ssh(self.target_host.clone())
        .with_required_env()
        .run()
        .wrap_err("Failed to set system profile")?;

      let mut cmd = Command::new(switch_to_configuration)
        .arg("boot")
        .ssh(self.target_host)
        .elevate(elevate.then_some(elevation))
        .message("Adding configuration to bootloader")
        .preserve_envs(["NIXOS_INSTALL_BOOTLOADER"]);

      if self.install_bootloader {
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

impl OsRollbackArgs {
  #[expect(clippy::too_many_lines)]
  fn rollback(&self, elevation: ElevationStrategy) -> Result<()> {
    let elevate = check_and_get_elevation_status(self.bypass_root_check)?;

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
      return Err(eyre!(
        "The 'switch-to-configuration' binary is missing from the built \
         configuration.\n\nThis typically happens when 'system.switch.enable' \
         is set to false in your\nNixOS configuration. To fix this, please \
         either:\n1. Remove 'system.switch.enable = false' from your \
         configuration, or\n2. Set 'system.switch.enable = true' \
         explicitly\n\nIf the problem persists, please open an issue on our \
         issue tracker!"
      ));
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

  if !bin_dir.exists() {
    bail!(
      "VM build output missing bin directory at {}",
      bin_dir.display()
    );
  }

  let entries = fs::read_dir(&bin_dir).wrap_err_with(|| {
    format!("Failed to read bin directory at {}", bin_dir.display())
  })?;

  let mut vm_script: Option<PathBuf> = None;
  for entry_result in entries {
    match entry_result {
      Ok(entry) => {
        let fname = entry.file_name();
        if fname
          .to_str()
          .is_some_and(|name| name.starts_with("run-") && name.ends_with("-vm"))
        {
          vm_script = Some(entry.path());
          break;
        }
      },
      Err(e) => {
        warn!(
          "Error reading entry in VM bin directory ({}): {}",
          bin_dir.display(),
          e
        );
      },
    }
  }
  let vm_script = vm_script.ok_or_else(|| {
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
fn print_vm_instructions(out_path: &Path) -> Result<()> {
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

  Ok(())
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

/// Parses the `NH_OS_FLAKE` environment variable into an `Installable::Flake`.
///
/// If `NH_OS_FLAKE` is not set, it returns `Ok(None)`.
/// If `NH_OS_FLAKE` is set but invalid, it returns an `Err`.
fn parse_nh_os_flake_env() -> Result<Option<Installable>> {
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
fn check_and_get_elevation_status(bypass_root_check: bool) -> Result<bool> {
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
  let generations = list_generations()?;
  generations
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
) -> Installable {
  let mut res = installable;
  let hostname = hostname.as_ref().to_owned();

  let toplevel = ["config", "system", "build", final_attr]
    .into_iter()
    .map(String::from);

  match res {
    Installable::Flake {
      ref mut attribute, ..
    } => {
      // If user explicitly selects some other attribute, don't push
      // nixosConfigurations
      if attribute.is_empty() {
        attribute.push(String::from("nixosConfigurations"));
        attribute.push(hostname);
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
  }

  res
}

impl OsReplArgs {
  fn run(self) -> Result<()> {
    // Use NH_OS_FLAKE if available, otherwise use the provided installable
    let mut target_installable =
      if let Some(flake_installable) = parse_nh_os_flake_env()? {
        flake_installable
      } else {
        self.installable
      };

    if matches!(target_installable, Installable::Store { .. }) {
      bail!("Nix doesn't support nix store installables.");
    }

    let hostname = get_resolved_hostname(self.hostname)?;

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

    generations::print_info(descriptions, &self.fields)?;

    Ok(())
  }
}

use std::fs;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{bail, Context};
use color_eyre::eyre::{eyre, Result};
use tracing::{debug, info, warn};

use crate::commands::Command;
use crate::generations;
use crate::installable::Installable;
use crate::interface::OsSubcommand::{self};
use crate::interface::{self, OsGenerationsArgs, OsRebuildArgs, OsReplArgs, OsRollbackArgs};
use crate::update::update;
use crate::util::ensure_ssh_key_login;
use crate::util::get_hostname;
use crate::util::platform;

/// Path to the system profile on `NixOS`
const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
/// Path to the current system profile on `NixOS`
const CURRENT_PROFILE: &str = "/run/current-system";
/// Path where `NixOS` stores specialisation information
const SPEC_LOCATION: &str = "/etc/specialisation";

impl interface::OsArgs {
    /// Entry point for processing `NixOS` commands
    ///
    /// Handles the various subcommands for `NixOS` configurations:
    /// - Switch: Builds, activates, and makes the configuration the boot default
    /// - Boot: Builds and makes the configuration the boot default
    /// - Test: Builds and activates the configuration
    /// - Build: Only builds the configuration
    /// - Repl: Opens a REPL for exploring the configuration
    /// - Info: Lists available generations
    /// - Rollback: Reverts to a previous generation
    /// - `BuildVm`: Builds a `NixOS` VM image
    pub fn run(self) -> Result<()> {
        use OsRebuildVariant::{Boot, Build, Switch, Test};
        // Always resolve installable from env var at the top, or use the provided one
        let fallback_installable = match &self.subcommand {
            OsSubcommand::Boot(args) => args.common.installable.clone(),
            OsSubcommand::Test(args) => args.common.installable.clone(),
            OsSubcommand::Switch(args) => args.common.installable.clone(),
            OsSubcommand::Build(args) => args.common.installable.clone(),
            OsSubcommand::BuildVm(args) => args.common.common.installable.clone(),
            OsSubcommand::Repl(args) => args.installable.clone(),
            _ => Installable::default(), // fallback for Info/Rollback, not used
        };

        let resolved_installable =
            platform::resolve_env_installable("NH_OS_FLAKE").unwrap_or(fallback_installable);
        match self.subcommand {
            OsSubcommand::Boot(args) => {
                args.rebuild_with_installable(Boot, None, resolved_installable)
            }
            OsSubcommand::Test(args) => {
                args.rebuild_with_installable(Test, None, resolved_installable)
            }
            OsSubcommand::Switch(args) => {
                args.rebuild_with_installable(Switch, None, resolved_installable)
            }
            OsSubcommand::Build(args) => {
                if args.common.ask || args.common.dry {
                    warn!("`--ask` and `--dry` have no effect for `nh os build`");
                }
                args.rebuild_with_installable(Build, None, resolved_installable)
            }
            OsSubcommand::BuildVm(args) => {
                let final_attr = get_final_attr(true, args.with_bootloader);
                args.common.rebuild_with_installable(
                    OsRebuildVariant::BuildVm,
                    Some(final_attr),
                    resolved_installable,
                )
            }
            OsSubcommand::Repl(args) => args.run_with_installable(resolved_installable),
            OsSubcommand::Info(args) => args.info(),
            OsSubcommand::Rollback(args) => args.rollback(),
        }
    }
}

/// Variants of the `NixOS` rebuild operation
///
/// Each variant represents a different mode of operation with distinct
/// activation behaviors:
/// - Build: Only build the configuration
/// - Switch: Build, activate, and make it the boot default
/// - Boot: Build and make it the boot default
/// - Test: Build and activate
/// - `BuildVm`: Build a VM image for testing
#[derive(Debug)]
enum OsRebuildVariant {
    Build,
    Switch,
    Boot,
    Test,
    BuildVm,
}

impl OsRebuildArgs {
    /// Rebuilds a `NixOS` configuration with the given variant and installable
    ///
    /// This is the core function for building and deploying `NixOS` configurations.
    /// It handles:
    /// 1. SSH key login for remote operations
    /// 2. Root privilege management
    /// 3. Flake updates if requested
    /// 4. Hostname resolution and validation
    /// 5. Building the configuration
    /// 6. Specialisation handling
    /// 7. Remote deployment if `target_host` is specified
    /// 8. Configuration activation based on variant
    ///
    /// The different variants determine which aspects of deployment are executed:
    /// - Build: Only build the configuration
    /// - Switch: Build, activate, and make boot default
    /// - Boot: Build and make boot default
    /// - Test: Build and activate
    /// - `BuildVm`: Build a VM image
    fn rebuild_with_installable(
        self,
        variant: OsRebuildVariant,
        final_attr: Option<String>,
        installable: Installable,
    ) -> Result<()> {
        use OsRebuildVariant::{Boot, Build, BuildVm, Switch, Test};
        if self.build_host.is_some() || self.target_host.is_some() {
            let _ = ensure_ssh_key_login();
        }

        // Check for root privileges and elevate if needed
        let elevate = platform::check_not_root(self.bypass_root_check)?;

        if self.update_args.update {
            update(&self.common.installable, self.update_args.update_input)?;
        }

        // Determine hostname and handle hostname mismatch
        let (target_hostname, hostname_mismatch) = platform::get_target_hostname(
            self.hostname.clone(),
            true, // Skip comparison when system hostname != target hostname
        )?;

        // Create temporary output path for the build result
        let out_path = platform::create_output_path(self.common.out_link, "nh-os")?;
        debug!(?out_path);

        // Determine the final attribute path
        let final_attribute_path = match final_attr {
            Some(ref attr) => attr.as_str(),
            None => match variant {
                BuildVm => "vm", // We moved with_bootloader check to get_final_attr
                _ => "toplevel",
            },
        };

        // Configure and build the NixOS configuration
        let target_profile = platform::handle_rebuild_workflow(
            installable,
            "nixosConfigurations",
            &["config", "system", "build", final_attribute_path],
            Some(target_hostname),
            out_path.as_ref(),
            &self.extra_args,
            self.build_host.clone(),
            match variant {
                BuildVm => "Building NixOS VM image",
                _ => "Building NixOS configuration",
            },
            self.common.no_nom,
            SPEC_LOCATION,
            self.no_specialisation,
            self.specialisation.clone(),
            CURRENT_PROFILE,
            hostname_mismatch,
        )?;

        // Handle dry run mode or check if confirmation is needed
        if self.common.dry || matches!(variant, Build | BuildVm) {
            if self.common.ask {
                warn!("--ask has no effect as dry run was requested");
            }
            return Ok(());
        }

        if !platform::confirm_action(self.common.ask, self.common.dry)? {
            return Ok(());
        }

        // Copy to target host if needed
        if let Some(target_host) = &self.target_host {
            Command::new("nix")
                .args([
                    "copy",
                    "--to",
                    format!("ssh://{target_host}").as_str(),
                    target_profile.to_str().unwrap(),
                ])
                .message("Copying configuration to target")
                .run()?;
        };

        // Activate configuration for test and switch variants
        if let Test | Switch = variant {
            platform::activate_nixos_configuration(
                &target_profile,
                "test",
                self.target_host.clone(),
                elevate,
                "Activating configuration",
            )?;
        }

        // Add configuration to bootloader for boot and switch variants
        if let Boot | Switch = variant {
            Command::new("nix")
                .elevate(elevate)
                .args(["build", "--no-link", "--profile", SYSTEM_PROFILE])
                .arg(out_path.get_path().canonicalize().unwrap())
                .ssh(self.target_host.clone())
                .run()?;

            platform::activate_nixos_configuration(
                out_path.get_path(),
                "boot",
                self.target_host,
                elevate,
                "Adding configuration to bootloader",
            )?;
        }

        drop(out_path);
        Ok(())
    }
}

impl OsRollbackArgs {
    /// Rolls back the system to a previous generation
    ///
    /// This function:
    /// 1. Finds the generation to roll back to (previous or specified)
    /// 2. Shows a diff between current and target generations
    /// 3. Sets the system profile to point to the target generation
    /// 4. Activates the configuration
    /// 5. Handles failures by rolling back the profile symlink if activation fails
    ///
    /// Generation specialisations are properly handled during rollback.
    fn rollback(&self) -> Result<()> {
        // Check if we need root permissions
        let elevate = platform::check_not_root(self.bypass_root_check)?;

        // Find previous generation or specific generation
        let target_generation = if let Some(gen_number) = self.to {
            find_generation_by_number(gen_number)?
        } else {
            find_previous_generation()?
        };

        info!("Rolling back to generation {}", target_generation.number);

        // Construct path to the generation
        let profile_dir = Path::new(SYSTEM_PROFILE)
            .parent()
            .unwrap_or(Path::new("/nix/var/nix/profiles"));
        let generation_link = profile_dir.join(format!("system-{}-link", target_generation.number));

        // Handle any system specialisations
        let target_specialisation = platform::process_specialisation(
            self.no_specialisation,
            self.specialisation.clone(),
            SPEC_LOCATION,
        )?;

        // Show diff between current and target configuration
        platform::compare_configurations(
            CURRENT_PROFILE,
            &generation_link,
            false,
            "Comparing changes",
        )?;

        if self.dry {
            info!(
                "Dry run: would roll back to generation {}",
                target_generation.number
            );
            return Ok(());
        }

        // Ask for confirmation if needed
        if !platform::confirm_action(self.ask, self.dry)? {
            return Ok(());
        }

        // Get current generation number for potential rollback
        let current_gen_number = match get_current_generation_number() {
            Ok(num) => num,
            Err(e) => {
                warn!("Failed to get current generation number: {}", e);
                0
            }
        };

        // Set the system profile
        info!("Setting system profile...");

        // Instead of direct symlink operations, use a command with proper elevation
        Command::new("ln")
            .arg("-sfn") // force, symbolic link
            .arg(&generation_link)
            .arg(SYSTEM_PROFILE)
            .elevate(elevate)
            .message("Setting system profile")
            .run()?;

        // Set up rollback protection flag
        let mut _rollback_profile = false;

        // Get the final profile path with specialisation if any
        let target_specialisation_option = target_specialisation;
        let final_profile =
            platform::get_target_profile(&generation_link, &target_specialisation_option);

        // Activate the configuration
        info!("Activating...");

        let switch_to_configuration = final_profile.join("bin").join("switch-to-configuration");

        match Command::new(&switch_to_configuration)
            .arg("switch")
            .elevate(elevate)
            .run()
        {
            Ok(()) => {
                info!(
                    "Successfully rolled back to generation {}",
                    target_generation.number
                );
            }
            Err(e) => {
                _rollback_profile = true;

                // If activation fails, rollback the profile
                if _rollback_profile && current_gen_number > 0 {
                    let current_gen_link =
                        profile_dir.join(format!("system-{current_gen_number}-link"));

                    Command::new("ln")
                        .arg("-sfn") // Force, symbolic link
                        .arg(&current_gen_link)
                        .arg(SYSTEM_PROFILE)
                        .elevate(elevate)
                        .message("Rolling back system profile")
                        .run()?;
                }

                return Err(e).context("Failed to activate configuration");
            }
        }

        Ok(())
    }
}

/// Finds the previous generation in the system profile
///
/// This function:
/// 1. Searches for available system generations
/// 2. Identifies which one is currently active
/// 3. Returns the generation immediately before the current one
///
/// Returns an error if there are no generations or if the current
/// generation is already the oldest one.
fn find_previous_generation() -> Result<generations::GenerationInfo> {
    let profile_path = PathBuf::from(SYSTEM_PROFILE);

    let mut generations: Vec<generations::GenerationInfo> = fs::read_dir(
        profile_path
            .parent()
            .unwrap_or(Path::new("/nix/var/nix/profiles")),
    )?
    .filter_map(|entry| {
        entry.ok().and_then(|e| {
            let path = e.path();
            if let Some(filename) = path.file_name() {
                if let Some(name) = filename.to_str() {
                    if name.starts_with("system-") && name.ends_with("-link") {
                        return generations::describe(&path, &profile_path);
                    }
                }
            }
            None
        })
    })
    .collect();

    if generations.is_empty() {
        bail!("No generations found");
    }

    generations.sort_by(|a, b| {
        a.number
            .parse::<u64>()
            .unwrap_or(0)
            .cmp(&b.number.parse::<u64>().unwrap_or(0))
    });

    let current_idx = generations
        .iter()
        .position(|g| g.current)
        .ok_or_else(|| eyre!("Current generation not found"))?;

    if current_idx == 0 {
        bail!("No generation older than the current one exists");
    }

    Ok(generations[current_idx - 1].clone())
}

/// Finds a specific generation by its number
///
/// Searches the system profiles directory for a generation with the
/// specified number and returns its information if found.
fn find_generation_by_number(number: u64) -> Result<generations::GenerationInfo> {
    let profile_path = PathBuf::from(SYSTEM_PROFILE);

    let generations: Vec<generations::GenerationInfo> = fs::read_dir(
        profile_path
            .parent()
            .unwrap_or(Path::new("/nix/var/nix/profiles")),
    )?
    .filter_map(|entry| {
        entry.ok().and_then(|e| {
            let path = e.path();
            if let Some(filename) = path.file_name() {
                if let Some(name) = filename.to_str() {
                    if name.starts_with("system-") && name.ends_with("-link") {
                        return generations::describe(&path, &profile_path);
                    }
                }
            }
            None
        })
    })
    .filter(|gen| gen.number == number.to_string())
    .collect();

    if generations.is_empty() {
        bail!("Generation {} not found", number);
    }

    Ok(generations[0].clone())
}

/// Gets the number of the currently active generation
///
/// This is useful for rollback operations, especially when needing
/// to restore the system if activation of an older generation fails.
fn get_current_generation_number() -> Result<u64> {
    let profile_path = PathBuf::from(SYSTEM_PROFILE);

    let generations: Vec<generations::GenerationInfo> = fs::read_dir(
        profile_path
            .parent()
            .unwrap_or(Path::new("/nix/var/nix/profiles")),
    )?
    .filter_map(|entry| {
        entry
            .ok()
            .and_then(|e| generations::describe(&e.path(), &profile_path))
    })
    .collect();

    let current_gen = generations
        .iter()
        .find(|g| g.current)
        .ok_or_else(|| eyre!("Current generation not found"))?;

    current_gen
        .number
        .parse::<u64>()
        .map_err(|_| eyre!("Invalid generation number"))
}

/// Determines the final attribute name for VM builds
///
/// Returns the appropriate Nix attribute based on whether
/// the VM should include a bootloader:
/// - "vmWithBootLoader" for VM with bootloader
/// - "vm" for standard VM
/// - "toplevel" for regular builds
pub fn get_final_attr(build_vm: bool, with_bootloader: bool) -> String {
    let attr = if build_vm && with_bootloader {
        "vmWithBootLoader"
    } else if build_vm {
        "vm"
    } else {
        "toplevel"
    };
    String::from(attr)
}

impl OsReplArgs {
    /// Opens a Nix REPL for exploring `NixOS` configurations
    ///
    /// Provides an interactive environment to explore and evaluate
    /// components of a `NixOS` configuration. This is useful for
    /// debugging or exploring available options.
    fn run_with_installable(self, installable: Installable) -> Result<()> {
        // Get hostname, with fallback to system hostname
        let hostname = match self.hostname {
            Some(h) => h,
            None => match get_hostname() {
                Ok(h) => {
                    debug!("Auto-detected hostname: {}", h);
                    h
                }
                Err(e) => {
                    warn!("Failed to get hostname automatically: {}", e);
                    bail!("Unable to fetch hostname, and no hostname supplied. Please specify with --hostname");
                }
            },
        };

        // Open a Nix REPL for interactively exploring the NixOS configuration
        platform::run_repl(
            installable,
            "nixosConfigurations",
            &[], // No trailing path needed for REPL
            Some(hostname),
            &[], // No extra args
        )
    }
}

impl OsGenerationsArgs {
    /// Lists information about available `NixOS` generations
    ///
    /// This function:
    /// 1. Identifies the profile and confirms it exists
    /// 2. Finds all generations associated with that profile
    /// 3. Collects metadata about each generation (number, date, etc.)
    /// 4. Displays the information in a formatted list
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
            .filter_map(|gen_dir| generations::describe(gen_dir, &profile))
            .collect();

        generations::print_info(descriptions);

        Ok(())
    }
}

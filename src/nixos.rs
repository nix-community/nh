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

const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
const CURRENT_PROFILE: &str = "/run/current-system";

const SPEC_LOCATION: &str = "/etc/specialisation";

impl interface::OsArgs {
    pub fn run(self) -> Result<()> {
        use OsRebuildVariant::{Boot, Build, Switch, Test};
        // Always resolve installable from env var at the top
        let resolved_installable = platform::resolve_env_installable(
            "NH_OS_FLAKE",
            match &self.subcommand {
                OsSubcommand::Boot(args) => args.common.installable.clone(),
                OsSubcommand::Test(args) => args.common.installable.clone(),
                OsSubcommand::Switch(args) => args.common.installable.clone(),
                OsSubcommand::Build(args) => args.common.installable.clone(),
                OsSubcommand::BuildVm(args) => args.common.common.installable.clone(),
                OsSubcommand::Repl(args) => args.installable.clone(),
                _ => Installable::default(), // fallback for Info/Rollback, not used
            },
        );
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

#[derive(Debug)]
enum OsRebuildVariant {
    Build,
    Switch,
    Boot,
    Test,
    BuildVm,
}

impl OsRebuildArgs {
    // Accept the resolved installable as a parameter
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

        let system_hostname = match get_hostname() {
            Ok(hostname) => Some(hostname),
            Err(err) => {
                tracing::warn!("{}", err.to_string());
                None
            }
        };

        let target_hostname = match &self.hostname {
            Some(h) => h.to_owned(),
            None => match &system_hostname {
                Some(hostname) => {
                    tracing::warn!("Guessing system is {hostname} for a VM image. If this isn't intended, use --hostname to change.");
                    hostname.clone()
                }
                None => return Err(eyre!("Unable to fetch hostname, and no hostname supplied.")),
            },
        };

        // Create temporary output path for the build result
        let out_path = platform::create_output_path(self.common.out_link, "nh-os")?;
        debug!(?out_path);

        // Configure installable with the right attributes for NixOS
        let toplevel = platform::extend_installable_for_platform(
            installable,
            "nixosConfigurations",
            &[
                "config",
                "system",
                "build",
                final_attr.as_deref().unwrap_or("toplevel"),
            ],
            Some(target_hostname.clone()),
            true,
            &self
                .extra_args
                .iter()
                .map(std::convert::Into::into)
                .collect::<Vec<_>>(),
        )?;

        let message = match variant {
            BuildVm => "Building NixOS VM image",
            _ => "Building NixOS configuration",
        };

        // Build the NixOS configuration
        platform::build_configuration(
            toplevel,
            out_path.as_ref(),
            &self.extra_args,
            self.build_host.clone(),
            message,
            self.common.no_nom,
        )?;

        // Process any system specialisations
        let target_specialisation = platform::process_specialisation(
            self.no_specialisation,
            self.specialisation.clone(),
            SPEC_LOCATION,
        )?;

        // Get the target system profile path
        let target_profile =
            platform::get_target_profile(out_path.as_ref(), &target_specialisation);

        debug!("exists: {}", target_profile.exists());
        target_profile.try_exists().context("Doesn't exist")?;

        if self.build_host.is_none()
            && self.target_host.is_none()
            && system_hostname.map_or(true, |h| h == target_hostname)
        {
            // Show diff between current and new configuration
            platform::compare_configurations(
                CURRENT_PROFILE,
                &target_profile,
                false,
                "Comparing changes",
            )?;
        } else {
            debug!("Not running nvd as the target hostname is different from the system hostname.");
        }

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

        if let Test | Switch = variant {
            let switch_to_configuration =
                target_profile.join("bin").join("switch-to-configuration");
            let switch_to_configuration = switch_to_configuration.canonicalize().unwrap();
            let switch_to_configuration = switch_to_configuration.to_str().unwrap();
            Command::new(switch_to_configuration)
                .arg("test")
                .ssh(self.target_host.clone())
                .message("Activating configuration")
                .elevate(elevate)
                .run()?;
        }

        if let Boot | Switch = variant {
            Command::new("nix")
                .elevate(elevate)
                .args(["build", "--no-link", "--profile", SYSTEM_PROFILE])
                .arg(out_path.get_path().canonicalize().unwrap())
                .ssh(self.target_host.clone())
                .run()?;
            let switch_to_configuration = out_path
                .get_path()
                .join("bin")
                .join("switch-to-configuration");
            let switch_to_configuration = switch_to_configuration.canonicalize().unwrap();
            let switch_to_configuration = switch_to_configuration.to_str().unwrap();
            Command::new(switch_to_configuration)
                .arg("boot")
                .ssh(self.target_host)
                .elevate(elevate)
                .message("Adding configuration to bootloader")
                .run()?;
        }
        drop(out_path);
        Ok(())
    }
}

impl OsRollbackArgs {
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
        let final_profile = platform::get_target_profile(&generation_link, &target_specialisation);

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
    fn run_with_installable(self, installable: Installable) -> Result<()> {
        // Launch the nix REPL with the NixOS configuration
        platform::run_repl(
            installable,
            "nixosConfigurations",
            &["config", "system", "build", "toplevel"],
            self.hostname,
            &[],
        )
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
            .filter_map(|gen_dir| generations::describe(gen_dir, &profile))
            .collect();

        generations::print_info(descriptions);

        Ok(())
    }
}

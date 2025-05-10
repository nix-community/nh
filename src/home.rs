use std::env;
use std::path::PathBuf;

use color_eyre::Result;
use tracing::{debug, info, warn};

use crate::commands::Command;
use crate::interface::{self, HomeRebuildArgs, HomeReplArgs, HomeSubcommand};
use crate::update::update;
use crate::util::platform;

impl interface::HomeArgs {
    pub fn run(self) -> Result<()> {
        use HomeRebuildVariant::{Build, Switch};
        match self.subcommand {
            HomeSubcommand::Switch(args) => args.rebuild(Switch),
            HomeSubcommand::Build(args) => {
                if args.common.ask || args.common.dry {
                    warn!("`--ask` and `--dry` have no effect for `nh home build`");
                }
                args.rebuild(Build)
            }
            HomeSubcommand::Repl(args) => args.run(),
        }
    }
}

#[derive(Debug)]
enum HomeRebuildVariant {
    Build,
    Switch,
}

impl HomeRebuildArgs {
    fn rebuild(self, variant: HomeRebuildVariant) -> Result<()> {
        use HomeRebuildVariant::Build;

        if self.update_args.update {
            update(&self.common.installable, self.update_args.update_input)?;
        }

        let out_path = platform::create_output_path(self.common.out_link, "nh-home")?;
        debug!(?out_path);

        // Check for environment variable override for flake path
        let installable =
            platform::resolve_env_installable("NH_HOME_FLAKE", self.common.installable.clone());

        // Set up the installable with the correct attribute path
        let toplevel = platform::extend_installable_for_platform(
            installable,
            "homeConfigurations",
            &["config", "home", "activationPackage"],
            self.configuration.clone(),
            true,
            &self
                .extra_args
                .iter()
                .map(std::convert::Into::into)
                .collect::<Vec<_>>(),
        )?;

        platform::build_configuration(
            toplevel,
            out_path.as_ref(),
            &self.extra_args,
            None,
            "Building Home-Manager configuration",
            self.common.no_nom,
        )?;

        // Find the previous home-manager generation if it exists
        let prev_generation: Option<PathBuf> = [
            PathBuf::from("/nix/var/nix/profiles/per-user")
                .join(env::var("USER").expect("Couldn't get username"))
                .join("home-manager"),
            PathBuf::from(env::var("HOME").expect("Couldn't get home directory"))
                .join(".local/state/nix/profiles/home-manager"),
        ]
        .into_iter()
        .find(|next| next.exists());

        debug!(?prev_generation);

        // Location where home-manager stores specialisation info
        let spec_location =
            PathBuf::from(std::env::var("HOME")?).join(".local/share/home-manager/specialisation");

        // Process any specialisations for home-manager
        let target_specialisation = platform::process_specialisation(
            self.no_specialisation,
            self.specialisation,
            spec_location.to_str().unwrap(),
        )?;

        // Get final path considering specialisations
        let target_profile =
            platform::get_target_profile(out_path.as_ref(), &target_specialisation);

        // Skip comparison for fresh installs (no previous generation)
        if let Some(generation) = prev_generation {
            platform::compare_configurations(
                &generation.to_string_lossy(),
                &target_profile,
                false,
                "Comparing changes",
            )?;
        }

        // Handle dry run or build-only mode
        if self.common.dry || matches!(variant, Build) {
            if self.common.ask {
                warn!("--ask has no effect as dry run was requested");
            }
            return Ok(());
        }

        // Ask for confirmation if needed
        if !platform::confirm_action(self.common.ask, self.common.dry)? {
            return Ok(());
        }

        // Configure backup extension if provided
        if let Some(ext) = &self.backup_extension {
            info!("Using {} as the backup extension", ext);
            env::set_var("HOME_MANAGER_BACKUP_EXT", ext);
        }

        // Run the activation script
        Command::new(target_profile.join("activate"))
            .message("Activating configuration")
            .run()?;

        // Make sure out_path is not accidentally dropped
        // https://docs.rs/tempfile/3.12.0/tempfile/index.html#early-drop-pitfall
        drop(target_profile);
        drop(out_path);

        Ok(())
    }
}

impl HomeReplArgs {
    fn run(self) -> Result<()> {
        // Load flake from environment variable or use provided one
        let installable = platform::resolve_env_installable("NH_HOME_FLAKE", self.installable);

        // Launch the nix REPL with the home-manager configuration
        platform::run_repl(
            installable,
            "homeConfigurations",
            &["config", "home", "activationPackage"],
            self.configuration,
            &self.extra_args,
        )
    }
}

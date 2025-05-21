use std::env;
use std::path::PathBuf;

use color_eyre::Result;
use tracing::{debug, info, warn};

use crate::commands::Command;
use crate::interface::{self, HomeRebuildArgs, HomeReplArgs, HomeSubcommand};
use crate::update::update;
use crate::util::platform;

impl interface::HomeArgs {
    /// Entry point for processing home-manager commands
    ///
    /// Handles the different subcommands for home-manager configurations:
    /// - Switch: Builds and activates the configuration
    /// - Build: Only builds the configuration
    /// - Repl: Opens a REPL for exploring the configuration
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

/// Variants of the home-manager rebuild operation
///
/// Represents different actions that can be taken with a home-manager configuration:
/// - Build: Only build the configuration without activating it
/// - Switch: Build and activate the configuration
#[derive(Debug)]
enum HomeRebuildVariant {
    Build,
    Switch,
}

impl HomeRebuildArgs {
    /// Performs the rebuild operation for home-manager configurations
    ///
    /// This function handles building and potentially activating home-manager configurations.
    /// The workflow:
    /// 1. Updates the flake if requested
    /// 2. Creates a temporary output path
    /// 3. Builds the configuration with proper specialisation handling
    /// 4. Compares with the previous generation if it exists
    /// 5. Activates the configuration if needed
    ///
    /// Home Manager has its own specialisation mechanism which this function handles
    /// by looking in ~/.local/share/home-manager/specialisation.
    fn rebuild(self, variant: HomeRebuildVariant) -> Result<()> {
        use HomeRebuildVariant::Build;

        if self.update_args.update {
            update(&self.common.installable, self.update_args.update_input)?;
        }

        // Create output path
        let out_path = platform::create_output_path(self.common.out_link, "nh-home")?;
        debug!(?out_path);

        // Use NH_HOME_FLAKE if available, otherwise use the provided installable
        let installable = platform::resolve_env_installable("NH_HOME_FLAKE")
            .unwrap_or_else(|| self.common.installable.clone());

        // Set up the specialisation path
        let spec_location =
            PathBuf::from(std::env::var("HOME")?).join(".local/share/home-manager/specialisation");

        // Get the target profile
        let _target_profile = platform::handle_rebuild_workflow(
            installable,
            "homeConfigurations",
            &["config", "home", "activationPackage"],
            None, // No explicit hostname for home-manager
            out_path.as_ref(),
            &self.extra_args,
            None, // No builder
            "Building Home-Manager configuration",
            self.common.no_nom,
            spec_location
                .to_str()
                .unwrap_or(".local/share/home-manager/specialisation"),
            self.no_specialisation,
            self.specialisation.clone(),
            "",   // Empty current profile - we'll handle the comparison separately
            true, // Skip comparison as we'll do it manually
        )?;

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

        let current_specialisation = std::fs::read_to_string(spec_location.to_str().unwrap()).ok();

        let target_specialisation = if self.no_specialisation {
            None
        } else {
            current_specialisation.or(self.specialisation)
        };

        debug!("target_specialisation: {target_specialisation:?}");

        let target_profile: Box<dyn crate::util::MaybeTempPath> = match &target_specialisation {
            None => out_path,
            Some(spec) => Box::new(out_path.get_path().join("specialisation").join(spec)),
        };

        // Just do nothing for None case (fresh installs)
        if let Some(generation) = prev_generation {
            platform::compare_configurations(
                generation.to_str().unwrap_or(""),
                target_profile.get_path(),
                false,
                "Comparing changes",
            )?;
        }

        if self.common.dry || matches!(variant, Build) {
            if self.common.ask {
                warn!("--ask has no effect as dry run was requested");
            }
            return Ok(());
        }

        // Check if user wants to proceed
        if !platform::confirm_action(self.common.ask, self.common.dry)? {
            return Ok(());
        }

        if let Some(ext) = &self.backup_extension {
            info!("Using {} as the backup extension", ext);
            env::set_var("HOME_MANAGER_BACKUP_EXT", ext);
        }

        Command::new(target_profile.get_path().join("activate"))
            .message("Activating configuration")
            .run()?;

        // Make sure out_path is not accidentally dropped
        // https://docs.rs/tempfile/3.12.0/tempfile/index.html#early-drop-pitfall
        drop(target_profile);

        Ok(())
    }
}

impl HomeReplArgs {
    /// Opens a Nix REPL for exploring home-manager configurations
    ///
    /// Provides an interactive environment to explore and evaluate
    /// components of a home-manager configuration. This is useful for
    /// debugging or exploring available options.
    fn run(self) -> Result<()> {
        // Use NH_HOME_FLAKE if available, otherwise use the provided installable
        let installable =
            platform::resolve_env_installable("NH_HOME_FLAKE").unwrap_or_else(|| self.installable);

        // Launch an interactive REPL session for exploring the configuration
        platform::run_repl(
            installable,
            "homeConfigurations",
            &[],  // No trailing path components
            None, // No explicit hostname
            &self
                .extra_args
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>(),
        )
    }
}

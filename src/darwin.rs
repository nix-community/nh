use color_eyre::eyre::{bail, Context};
use tracing::{debug, warn};

use crate::commands::Command;
use crate::installable::Installable;
use crate::interface::{DarwinArgs, DarwinRebuildArgs, DarwinReplArgs, DarwinSubcommand};
use crate::update::update;
use crate::util::get_hostname;
use crate::util::platform;
use crate::Result;

/// System profile path for darwin configurations
const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
/// Current system profile path for darwin
const CURRENT_PROFILE: &str = "/run/current-system";

impl DarwinArgs {
    /// Entry point for processing darwin commands
    ///
    /// Handles the different subcommands for darwin configurations:
    /// - Switch: Builds and activates the configuration
    /// - Build: Only builds the configuration
    /// - Repl: Opens a REPL for exploring the configuration
    pub fn run(self) -> Result<()> {
        use DarwinRebuildVariant::{Build, Switch};
        match self.subcommand {
            DarwinSubcommand::Switch(args) => args.rebuild(Switch),
            DarwinSubcommand::Build(args) => {
                if args.common.ask || args.common.dry {
                    warn!("`--ask` and `--dry` have no effect for `nh darwin build`");
                }
                args.rebuild(Build)
            }
            DarwinSubcommand::Repl(args) => args.run(),
        }
    }
}

/// Variants of the darwin rebuild operation
///
/// Each variant represents a different mode of operation:
/// - Switch: Build and activate the configuration
/// - Build: Only build the configuration without activation
enum DarwinRebuildVariant {
    Switch,
    Build,
}

impl DarwinRebuildArgs {
    /// Performs the rebuild operation for darwin configurations
    ///
    /// This function handles building and potentially activating darwin configurations.
    /// It first builds the configuration, then shows a diff of changes compared to the
    /// current system, and finally activates the configuration if needed.
    ///
    /// The darwin activation process is unique and requires special handling compared
    /// to `NixOS`, particularly around determining whether root privileges are needed.
    fn rebuild(self, variant: DarwinRebuildVariant) -> Result<()> {
        use DarwinRebuildVariant::{Build, Switch};

        // Check if running as root
        platform::check_not_root(false)?;

        if self.update_args.update {
            update(&self.common.installable, self.update_args.update_input)?;
        }

        let hostname = self.hostname.ok_or(()).or_else(|()| get_hostname())?;

        // Create temporary output path
        let out_path = platform::create_output_path(self.common.out_link, "nh-darwin")?;
        debug!(?out_path);

        // Resolve the installable from env var or from the provided argument
        let installable = platform::resolve_env_installable("NH_DARWIN_FLAKE")
            .unwrap_or_else(|| self.common.installable.clone());

        // Build the darwin configuration with proper attribute path handling
        let target_profile = platform::handle_rebuild_workflow(
            installable,
            "darwinConfigurations",
            &["toplevel"],
            Some(hostname),
            out_path.as_ref(),
            &self.extra_args,
            None, // Darwin doesn't use remote builders
            "Building Darwin configuration",
            self.common.no_nom,
            "", // Darwin doesn't use specialisations like NixOS
            false,
            None,
            CURRENT_PROFILE,
            false,
        )?;

        // Allow users to confirm before applying changes
        if !platform::confirm_action(
            self.common.ask && !matches!(variant, Build),
            self.common.dry,
        )? {
            return Ok(());
        }

        if matches!(variant, Switch) {
            Command::new("nix")
                .args(["build", "--no-link", "--profile", SYSTEM_PROFILE])
                .arg(&target_profile)
                .elevate(true)
                .dry(self.common.dry)
                .run()?;

            let darwin_rebuild = target_profile.join("sw/bin/darwin-rebuild");
            let activate_user = target_profile.join("activate-user");

            // Darwin activation may or may not need root privileges
            // This checks if we need elevation based on the activation-user script
            let needs_elevation = !activate_user
                .try_exists()
                .context("Failed to check if activate-user file exists")?
                || std::fs::read_to_string(&activate_user)
                    .context("Failed to read activate-user file")?
                    .contains("# nix-darwin: deprecated");

            // Actually activate the configuration using darwin-rebuild
            Command::new(darwin_rebuild)
                .arg("activate")
                .message("Activating configuration")
                .elevate(needs_elevation)
                .dry(self.common.dry)
                .run()?;
        }

        // Make sure out_path is not accidentally dropped
        // https://docs.rs/tempfile/3.12.0/tempfile/index.html#early-drop-pitfall

        Ok(())
    }
}

impl DarwinReplArgs {
    /// Opens a Nix REPL for exploring darwin configurations
    ///
    /// Provides an interactive environment to explore and evaluate
    /// components of a darwin configuration.
    fn run(self) -> Result<()> {
        if let Installable::Store { .. } = self.installable {
            bail!("Nix doesn't support nix store installables.");
        }

        let hostname = self.hostname.ok_or(()).or_else(|()| get_hostname())?;

        // Open an interactive REPL session for exploring darwin configurations
        platform::run_repl(
            platform::resolve_env_installable("NH_DARWIN_FLAKE")
                .unwrap_or_else(|| self.installable),
            "darwinConfigurations",
            &[], // REPL doesn't need additional path elements
            Some(hostname),
            &[], // No extra REPL args
        )
    }
}

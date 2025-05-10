use color_eyre::eyre::Context;
use tracing::{debug, warn};

use crate::commands::Command;
use crate::interface::{DarwinArgs, DarwinRebuildArgs, DarwinReplArgs, DarwinSubcommand};
use crate::update::update;
use crate::util::platform;
use crate::Result;

const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
const CURRENT_PROFILE: &str = "/run/current-system";

impl DarwinArgs {
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

enum DarwinRebuildVariant {
    Switch,
    Build,
}

impl DarwinRebuildArgs {
    fn rebuild(self, variant: DarwinRebuildVariant) -> Result<()> {
        use DarwinRebuildVariant::{Build, Switch};

        // Ensure we're not running as root
        platform::check_not_root(false)?;

        if self.update_args.update {
            update(&self.common.installable, self.update_args.update_input)?;
        }

        let hostname = self
            .hostname
            .ok_or(())
            .or_else(|()| crate::util::get_hostname())?;

        // Set up temporary directory for build results
        let out_path = platform::create_output_path(self.common.out_link, "nh-os")?;
        debug!(?out_path);

        // Check for environment variable override for flake path
        let installable =
            platform::resolve_env_installable("NH_DARWIN_FLAKE", self.common.installable.clone());

        // Configure the installable for Darwin
        let toplevel = platform::extend_installable_for_platform(
            installable,
            "darwinConfigurations",
            &["toplevel"],
            Some(hostname),
            true,
            &self
                .extra_args
                .iter()
                .map(std::convert::Into::into)
                .collect::<Vec<_>>(),
        )?;

        // Build the nix-darwin configuration
        platform::build_configuration(
            toplevel,
            out_path.as_ref(),
            &self.extra_args,
            None,
            "Building Darwin configuration",
            self.common.no_nom,
        )?;

        let target_profile = out_path.get_path().to_owned();
        target_profile.try_exists().context("Doesn't exist")?;

        // Show diff between current and new configuration
        platform::compare_configurations(
            CURRENT_PROFILE,
            &target_profile,
            false,
            "Comparing changes",
        )?;

        // Ask for confirmation if needed
        if !platform::confirm_action(self.common.ask, self.common.dry)? && !matches!(variant, Build)
        {
            return Ok(());
        }

        if matches!(variant, Switch) {
            Command::new("nix")
                .args(["build", "--no-link", "--profile", SYSTEM_PROFILE])
                .arg(out_path.get_path())
                .elevate(true)
                .dry(self.common.dry)
                .run()?;

            let darwin_rebuild = out_path.get_path().join("sw/bin/darwin-rebuild");
            let activate_user = out_path.get_path().join("activate-user");

            // Determine if we need to elevate privileges
            let needs_elevation = !activate_user
                .try_exists()
                .context("Failed to check if activate-user file exists")?
                || std::fs::read_to_string(&activate_user)
                    .context("Failed to read activate-user file")?
                    .contains("# nix-darwin: deprecated");

            // Create and run the activation command with or without elevation
            Command::new(darwin_rebuild)
                .arg("activate")
                .message("Activating configuration")
                .elevate(needs_elevation)
                .dry(self.common.dry)
                .run()?;
        }

        // Make sure out_path is not accidentally dropped
        // https://docs.rs/tempfile/3.12.0/tempfile/index.html#early-drop-pitfall
        drop(out_path);

        Ok(())
    }
}

impl DarwinReplArgs {
    fn run(self) -> Result<()> {
        // Check for environment variable override for flake path
        let installable = platform::resolve_env_installable("NH_DARWIN_FLAKE", self.installable);

        // Launch the nix REPL with the Darwin configuration
        platform::run_repl(
            installable,
            "darwinConfigurations",
            &["toplevel"],
            self.hostname,
            &[],
        )
    }
}

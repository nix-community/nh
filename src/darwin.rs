use std::env;

use color_eyre::eyre::{Context, bail};
use tracing::{debug, info, warn};

use crate::Result;
use crate::commands;
use crate::commands::Command;
use crate::installable::Installable;
use crate::interface::{DarwinArgs, DarwinRebuildArgs, DarwinReplArgs, DarwinSubcommand};
use crate::nixos::toplevel_for;
use crate::update::update;
use crate::util::get_hostname;

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

        if nix::unistd::Uid::effective().is_root() {
            bail!("Don't run nh os as root. I will call sudo internally as needed");
        }

        if self.update_args.update {
            update(&self.common.installable, self.update_args.update_input)?;
        }

        let hostname = self.hostname.ok_or(()).or_else(|()| get_hostname())?;

        let out_path: Box<dyn crate::util::MaybeTempPath> = match self.common.out_link {
            Some(ref p) => Box::new(p.clone()),
            None => Box::new({
                let dir = tempfile::Builder::new().prefix("nh-os").tempdir()?;
                (dir.as_ref().join("result"), dir)
            }),
        };

        debug!(?out_path);

        // Use NH_DARWIN_FLAKE if available, otherwise use the provided installable
        let installable = if let Ok(darwin_flake) = env::var("NH_DARWIN_FLAKE") {
            debug!("Using NH_DARWIN_FLAKE: {}", darwin_flake);

            let mut elems = darwin_flake.splitn(2, '#');
            let reference = elems.next().unwrap().to_owned();
            let attribute = elems
                .next()
                .map(crate::installable::parse_attribute)
                .unwrap_or_default();

            Installable::Flake {
                reference,
                attribute,
            }
        } else {
            self.common.installable.clone()
        };

        let mut processed_installable = installable;
        if let Installable::Flake {
            ref mut attribute, ..
        } = processed_installable
        {
            // If user explicitly selects some other attribute, don't push darwinConfigurations
            if attribute.is_empty() {
                attribute.push(String::from("darwinConfigurations"));
                attribute.push(hostname.clone());
            }
        }

        let toplevel = toplevel_for(hostname, processed_installable, "toplevel");

        commands::Build::new(toplevel)
            .extra_arg("--out-link")
            .extra_arg(out_path.get_path())
            .extra_args(&self.extra_args)
            .message("Building Darwin configuration")
            .nom(!self.common.no_nom)
            .run()?;

        let target_profile = out_path.get_path().to_owned();

        // Take a strong reference to out_path to prevent premature dropping
        // We need to keep this alive through the entire function scope to prevent
        // the tempdir from being dropped early, which would cause nvd diff to fail
        #[allow(unused_variables)]
        let keep_alive = out_path.get_path().to_owned();
        debug!(
            "Registered keep_alive reference to: {}",
            keep_alive.display()
        );

        target_profile.try_exists().context("Doesn't exist")?;

        Command::new("nvd")
            .arg("diff")
            .arg(CURRENT_PROFILE)
            .arg(&target_profile)
            .message("Comparing changes")
            .show_output(true)
            .run()?;

        if self.common.ask && !self.common.dry && !matches!(variant, Build) {
            info!("Apply the config?");
            let confirmation = dialoguer::Confirm::new().default(false).interact()?;

            if !confirmation {
                bail!("User rejected the new config");
            }
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

        // Make sure out_path is not acidentally dropped
        // https://docs.rs/tempfile/3.12.0/tempfile/index.html#early-drop-pitfall
        debug!(
            "Completed operation with output path: {:?}",
            out_path.get_path()
        );
        drop(out_path);

        Ok(())
    }
}

impl DarwinReplArgs {
    fn run(self) -> Result<()> {
        // Use NH_DARWIN_FLAKE if available, otherwise use the provided installable
        let mut target_installable = if let Ok(darwin_flake) = env::var("NH_DARWIN_FLAKE") {
            debug!("Using NH_DARWIN_FLAKE: {}", darwin_flake);

            let mut elems = darwin_flake.splitn(2, '#');
            let reference = elems.next().unwrap().to_owned();
            let attribute = elems
                .next()
                .map(crate::installable::parse_attribute)
                .unwrap_or_default();

            Installable::Flake {
                reference,
                attribute,
            }
        } else {
            self.installable
        };

        if matches!(target_installable, Installable::Store { .. }) {
            bail!("Nix doesn't support nix store installables.");
        }

        let hostname = self.hostname.ok_or(()).or_else(|()| get_hostname())?;

        if let Installable::Flake {
            ref mut attribute, ..
        } = target_installable
        {
            if attribute.is_empty() {
                attribute.push(String::from("darwinConfigurations"));
                attribute.push(hostname);
            }
        }

        Command::new("nix")
            .arg("repl")
            .args(target_installable.to_args())
            .run()?;

        Ok(())
    }
}

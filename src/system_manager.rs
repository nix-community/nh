use std::env;

use color_eyre::eyre::{Context, bail};
use tracing::{debug, info, warn};

use crate::Result;
use crate::commands;
use crate::commands::Command;
use crate::installable::Installable;
use crate::interface::{SystemManagerArgs, SystemManagerRebuildArgs, SystemManagerSubcommand};
use crate::update::update;
use crate::util::get_hostname;

const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";

impl SystemManagerArgs {
    pub fn run(self) -> Result<()> {
        use SystemManagerRebuildVariant::{Build, Switch};
        match self.subcommand {
            SystemManagerSubcommand::Switch(args) => args.rebuild(Switch),
            SystemManagerSubcommand::Build(args) => {
                if args.common.ask || args.common.dry {
                    warn!("`--ask` and `--dry` have no effect for `nh system-manager build`");
                }
                args.rebuild(Build)
            }
        }
    }
}

enum SystemManagerRebuildVariant {
    Switch,
    Build,
}

impl SystemManagerRebuildArgs {
    fn rebuild(self, variant: SystemManagerRebuildVariant) -> Result<()> {
        use SystemManagerRebuildVariant::{Build, Switch};

        if nix::unistd::Uid::effective().is_root() {
            bail!("Don't run nh system-manager as root. I will call sudo internally as needed");
        }

        if self.update_args.update_all || self.update_args.update_input.is_some() {
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

        // Use NH_SYSTEM_FLAKE if available, otherwise use the provided installable
        let installable = if let Ok(system_manager_flake) = env::var("NH_SYSTEM_FLAKE") {
            debug!("Using NH_SYSTEM_FLAKE: {}", system_manager_flake);

            let mut elems = system_manager_flake.splitn(2, '#');
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
            // If user explicitly selects some other attribute, don't push systemConfigs
            if attribute.is_empty() {
                attribute.push(String::from("systemConfigs"));
                attribute.push(hostname.clone());
            }
        }

        commands::Build::new(processed_installable)
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
            .arg(SYSTEM_PROFILE)
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
            Command::new("/nix/var/nix/profiles/default/bin/nix")
                .args(["build", "--no-link", "--profile", SYSTEM_PROFILE])
                .arg(out_path.get_path())
                .elevate(true)
                .dry(self.common.dry)
                .run()?;

            let system_manager_activate = out_path.get_path().join("bin/activate");

            // Create and run the activation command
            Command::new(system_manager_activate)
                .message("Activating configuration")
                .elevate(true)
                .dry(self.common.dry)
                .run()?;
        }

        // Make sure out_path is not accidentally dropped
        // https://docs.rs/tempfile/3.12.0/tempfile/index.html#early-drop-pitfall
        debug!(
            "Completed operation with output path: {:?}",
            out_path.get_path()
        );
        drop(out_path);

        Ok(())
    }
}

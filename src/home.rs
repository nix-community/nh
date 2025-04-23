use std::env;
use std::path::PathBuf;

use color_eyre::eyre::bail;
use color_eyre::Result;
use tracing::{debug, info, warn};

use crate::commands;
use crate::commands::Command;
use crate::installable::Installable;
use crate::interface::{self, HomeRebuildArgs, HomeReplArgs, HomeSubcommand};
use crate::update::update;
use crate::util::get_hostname;

impl interface::HomeArgs {
    pub fn run(self) -> Result<()> {
        use HomeRebuildVariant::*;
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
        use HomeRebuildVariant::*;

        if self.update_args.update {
            update(&self.common.installable, self.update_args.update_input)?;
        }

        let out_path: Box<dyn crate::util::MaybeTempPath> = match self.common.out_link {
            Some(ref p) => Box::new(p.clone()),
            None => Box::new({
                let dir = tempfile::Builder::new().prefix("nh-os").tempdir()?;
                (dir.as_ref().join("result"), dir)
            }),
        };

        debug!(?out_path);

        // Use NH_HOME_FLAKE if available, otherwise use the provided installable
        let installable = if let Ok(home_flake) = env::var("NH_HOME_FLAKE") {
            debug!("Using NH_HOME_FLAKE: {}", home_flake);

            let mut elems = home_flake.splitn(2, '#');
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

        let toplevel = toplevel_for(installable, true, &self.extra_args)?;

        commands::Build::new(toplevel)
            .extra_arg("--out-link")
            .extra_arg(out_path.get_path())
            .extra_args(&self.extra_args)
            .message("Building Home-Manager configuration")
            .nom(!self.common.no_nom)
            .run()?;

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

        let spec_location =
            PathBuf::from(std::env::var("HOME")?).join(".local/share/home-manager/specialisation");

        let current_specialisation = std::fs::read_to_string(spec_location.to_str().unwrap()).ok();

        let target_specialisation = if self.no_specialisation {
            None
        } else {
            current_specialisation.or(self.specialisation)
        };

        debug!("target_specialisation: {target_specialisation:?}");

        let target_profile = match &target_specialisation {
            None => out_path,
            Some(spec) => Box::new(out_path.get_path().join("specialisation").join(spec)),
        };

        // just do nothing for None case (fresh installs)
        if let Some(generation) = prev_generation {
            Command::new("nvd")
                .arg("diff")
                .arg(generation)
                .arg(target_profile.get_path())
                .message("Comparing changes")
                .run()?;
        }

        if self.common.dry || matches!(variant, Build) {
            if self.common.ask {
                warn!("--ask has no effect as dry run was requested");
            }
            return Ok(());
        }

        if self.common.ask {
            info!("Apply the config?");
            let confirmation = dialoguer::Confirm::new().default(false).interact()?;

            if !confirmation {
                bail!("User rejected the new config");
            }
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

fn toplevel_for<I, S>(
    installable: Installable,
    push_drv: bool,
    extra_args: I,
) -> Result<Installable>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut res = installable.clone();
    let extra_args = {
        let mut vec = Vec::new();
        for elem in extra_args.into_iter() {
            vec.push(elem.as_ref().to_owned());
        }
        vec
    };

    let toplevel = ["config", "home", "activationPackage"]
        .into_iter()
        .map(String::from);

    match res {
        Installable::Flake {
            ref reference,
            ref mut attribute,
        } => 'flake: {
            // If user explicitly selects some other attribute, don't push homeConfigurations
            if !attribute.is_empty() {
                break 'flake;
            }

            attribute.push(String::from("homeConfigurations"));

            // check for <user> and <user@hostname>
            let username = std::env::var("USER").expect("Couldn't get username");
            let hostname = get_hostname()?;

            let flake_reference = reference.clone();

            let mut tried = vec![];

            for attr in [format!("{username}@{hostname}"), username.to_string()] {
                let func = format!(r#" x: x ? "{}" "#, attr);
                let res = commands::Command::new("nix")
                    .arg("eval")
                    .args(&extra_args)
                    .arg("--apply")
                    .arg(func)
                    .args(
                        (Installable::Flake {
                            reference: flake_reference.clone(),
                            attribute: attribute.clone(),
                        })
                        .to_args(),
                    )
                    .run_capture()
                    .expect("Checking home-manager output");

                tried.push({
                    let mut attribute = attribute.clone();
                    attribute.push(attr.clone());
                    attribute
                });

                match res.map(|s| s.trim().to_owned()).as_deref() {
                    Some("true") => {
                        attribute.push(attr.clone());
                        if push_drv {
                            attribute.extend(toplevel);
                        }
                        break 'flake;
                    }
                    _ => {
                        continue;
                    }
                }
            }

            let tried_str = tried
                .into_iter()
                .map(|a| {
                    let f = Installable::Flake {
                        reference: flake_reference.clone(),
                        attribute: a,
                    };
                    f.to_args().join(" ")
                })
                .collect::<Vec<_>>()
                .join(", ");

            bail!("Couldn't find home-manager configuration, tried {tried_str}");
        }
        Installable::File {
            ref mut attribute, ..
        } => {
            if push_drv {
                attribute.extend(toplevel);
            }
        }
        Installable::Expression {
            ref mut attribute, ..
        } => {
            if push_drv {
                attribute.extend(toplevel);
            }
        }
        Installable::Store { .. } => {}
    }

    Ok(res)
}

impl HomeReplArgs {
    fn run(self) -> Result<()> {
        // Use NH_HOME_FLAKE if available, otherwise use the provided installable
        let installable = if let Ok(home_flake) = env::var("NH_HOME_FLAKE") {
            debug!("Using NH_HOME_FLAKE: {}", home_flake);

            let mut elems = home_flake.splitn(2, '#');
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

        let toplevel = toplevel_for(installable, false, &self.extra_args)?;

        Command::new("nix")
            .arg("repl")
            .args(toplevel.to_args())
            .run()?;

        Ok(())
    }
}

use std::{convert::Into, env, ffi::OsString, path::PathBuf};

use color_eyre::{
  Result,
  eyre::{Context, bail, eyre},
};
use nh_installable::{CommandContext, Installable};
use tracing::{debug, info, warn};

use crate::{
  commands,
  commands::Command,
  interface::{self, DiffType, HomeRebuildArgs, HomeReplArgs, HomeSubcommand},
  remote::{self, RemoteBuildConfig},
  update::update,
  util::{get_hostname, print_dix_diff},
};

impl interface::HomeArgs {
  /// Run the `home` subcommand.
  ///
  /// # Parameters
  ///
  /// * `self` - The Home Manager operation arguments
  ///
  /// # Returns
  ///
  /// Returns `Ok(())` if the operation succeeds.
  ///
  /// # Errors
  ///
  /// Returns an error if:
  ///
  /// - Build or activation operations fail
  /// - Remote operations encounter network or SSH issues
  /// - Nix evaluation or building fails
  /// - File system operations fail
  pub fn run(self) -> Result<()> {
    use HomeRebuildVariant::{Build, Switch};
    match self.subcommand {
      HomeSubcommand::Switch(args) => args.rebuild(&Switch),
      HomeSubcommand::Build(args) => {
        if args.common.ask || args.common.dry {
          warn!("`--ask` and `--dry` have no effect for `nh home build`");
        }
        args.rebuild(&Build)
      },
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
  fn rebuild(self, variant: &HomeRebuildVariant) -> Result<()> {
    use HomeRebuildVariant::Build;

    let (out_path, _tempdir_guard): (PathBuf, Option<tempfile::TempDir>) =
      if let Some(ref p) = self.common.out_link {
        (p.clone(), None)
      } else {
        let dir = tempfile::Builder::new().prefix("nh-home").tempdir()?;
        (dir.as_ref().join("result"), Some(dir))
      };

    debug!("Output path: {out_path:?}");

    let installable = self
      .common
      .installable
      .clone()
      .resolve(CommandContext::Home)?;

    let installable = match installable {
      Installable::Unspecified => Installable::try_find_default_for_home()?,
      other => other,
    };

    if self.update_args.update_all || self.update_args.update_input.is_some() {
      update(
        &installable,
        self.update_args.update_input,
        self.common.passthrough.commit_lock_file,
      )?;
    }

    let toplevel = toplevel_for(
      installable,
      true,
      &self.extra_args,
      self.configuration.clone(),
    )?;

    // If a build host is specified, use remote build semantics
    if let Some(build_host) = self.build_host {
      info!("Building Home-Manager configuration");

      let config = RemoteBuildConfig {
        build_host,
        target_host: None,
        use_nom: !self.common.no_nom,
        use_substitutes: self.common.passthrough.use_substitutes,
        extra_args: self
          .extra_args
          .iter()
          .map(Into::into)
          .chain(
            self
              .common
              .passthrough
              .generate_passthrough_args()
              .into_iter()
              .map(Into::into),
          )
          .collect(),
      };

      // Initialize SSH control - guard will cleanup connections on drop
      let _ssh_guard = remote::init_ssh_control();

      remote::build_remote(&toplevel, &config, Some(&out_path))
        .wrap_err("Failed to build Home-Manager configuration")?;
    } else {
      commands::Build::new(toplevel)
        .extra_arg("--out-link")
        .extra_arg(&out_path)
        .extra_args(&self.extra_args)
        .passthrough(&self.common.passthrough)
        .message("Building Home-Manager configuration")
        .nom(!self.common.no_nom)
        .run()
        .wrap_err("Failed to build Home-Manager configuration")?;
    }

    let username =
      env::var("USER").map_err(|_| eyre!("Couldn't get username"))?;
    let home_dir =
      env::var("HOME").map_err(|_| eyre!("Couldn't get home directory"))?;
    let state_home =
      env::var("XDG_STATE_HOME").unwrap_or(format!("{home_dir}/.local/state"));
    let data_home =
      env::var("XDG_DATA_HOME").unwrap_or(format!("{home_dir}/.local/share"));

    // Match Home Manager's profile discovery: prefer $XDG_STATE_HOME if set,
    // otherwise fall back to the global per-user profile directory.
    let prev_generation: Option<PathBuf> = [
      PathBuf::from(&state_home).join("nix/profiles/home-manager"),
      PathBuf::from("/nix/var/nix/profiles/per-user")
        .join(&username)
        .join("home-manager"),
    ]
    .into_iter()
    .find(|next| next.exists());

    debug!("Previous generation: {prev_generation:?}");

    let spec_location =
      PathBuf::from(data_home).join("home-manager/specialisation");

    let current_specialisation = spec_location.to_str().map_or_else(
      || {
        tracing::warn!("spec_location path is not valid UTF-8");
        None
      },
      |s| std::fs::read_to_string(s).ok().map(|s| s.trim().to_owned()),
    );

    let target_specialisation = if self.no_specialisation {
      None
    } else {
      self.specialisation.or(current_specialisation)
    };

    debug!("target_specialisation: {target_specialisation:?}");

    let target_profile: PathBuf = if let Some(spec) = &target_specialisation {
      out_path.join("specialisation").join(spec)
    } else {
      out_path
    };

    // just do nothing for None case (fresh installs)
    if let Some(generation) = prev_generation {
      match self.common.diff {
        DiffType::Never => {
          debug!("Not running dix as the --diff flag is set to never.");
        },
        _ => {
          let _ = print_dix_diff(&generation, &target_profile);
        },
      }
    }

    if self.common.dry || matches!(variant, Build) {
      if self.common.ask {
        warn!("--ask has no effect as dry run was requested");
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

    if let Some(ext) = &self.backup_extension {
      info!("Using {} as the backup extension", ext);
      unsafe {
        env::set_var("HOME_MANAGER_BACKUP_EXT", ext);
      }
    }

    Command::new(target_profile.join("activate"))
      .with_required_env()
      .message("Activating configuration")
      .show_output(self.show_activation_logs)
      .run()
      .wrap_err("Activation failed")?;

    debug!("Completed operation with output path: {target_profile:?}");

    Ok(())
  }
}

fn toplevel_for<I, S>(
  installable: Installable,
  push_drv: bool,
  extra_args: I,
  configuration_name: Option<String>,
) -> Result<Installable>
where
  I: IntoIterator<Item = S>,
  S: AsRef<std::ffi::OsStr>,
{
  let mut res = installable;
  let extra_args: Vec<OsString> = {
    let mut vec = Vec::new();
    for elem in extra_args {
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
    } => {
      if !attribute.is_empty() {
        // Check if the path is too specific
        if attribute[0] == "homeConfigurations" {
          if attribute.len() > 2 {
            bail!(
              "Attribute path is too specific: {}. Home Manager only allows \
               configuration names. Please either:\n  1. Use the flake \
               reference without attributes (e.g., '.')\n  2. Specify only \
               the configuration name (e.g., '.#{}')",
              attribute.join("."),
              attribute.get(1).unwrap_or(&"<unknown>".to_string())
            );
          }
        } else if attribute.len() > 1 {
          // User provided ".#myconfig" or similar - prepend homeConfigurations
          attribute.insert(0, String::from("homeConfigurations"));
          // Re-validate after prepending
          if attribute.len() > 2 {
            bail!(
              "Attribute path is too specific: {}. Home Manager only allows \
               configuration names. Please either:\n  1. Use the flake \
               reference without attributes (e.g., '.')\n  2. Specify only \
               the configuration name (e.g., '.#{}')",
              attribute.join("."),
              attribute.get(1).unwrap_or(&"<unknown>".to_string())
            );
          }
        }

        debug!(
          "Using explicit attribute path from installable: {:?}",
          attribute
        );
        return Ok(res);
      }

      attribute.push(String::from("homeConfigurations"));

      let flake_reference = reference.clone();
      let mut found_config = false;

      // Check if an explicit configuration name was provided via the flag
      if let Some(config_name) = configuration_name {
        // Verify the provided configuration exists
        let func = format!(r#" x: x ? "{config_name}" "#);
        let check_res = commands::Command::new("nix")
          .with_required_env()
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
          .wrap_err(format!(
            "Failed running nix eval to check for explicit configuration \
             '{config_name}'"
          ))?;

        if check_res.map(|s| s.trim().to_owned()).as_deref() == Some("true") {
          debug!("Using explicit configuration from flag: {config_name:?}");

          attribute.push(config_name);
          if push_drv {
            attribute.extend(toplevel.clone());
          }

          found_config = true;
        } else {
          // Explicit config provided but not found
          let tried_attr_path = {
            let mut attr_path = attribute.clone();
            attr_path.push(config_name);
            Installable::Flake {
              reference: flake_reference,
              attribute: attr_path,
            }
            .to_args()
            .join(" ")
          };
          bail!(
            "Explicitly specified home-manager configuration not found: \
             {tried_attr_path}"
          );
        }
      }

      // If no explicit config was found via flag, try automatic detection
      if !found_config {
        let username =
          std::env::var("USER").map_err(|_| eyre!("Couldn't get username"))?;
        let hostname = get_hostname(None)?;
        let mut tried = vec![];

        for attr_name in [format!("{username}@{hostname}"), username] {
          let func = format!(r#" x: x ? "{attr_name}" "#);
          let check_res = commands::Command::new("nix")
            .with_required_env()
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
            .wrap_err(format!(
              "Failed running nix eval to check for automatic configuration \
               '{attr_name}'"
            ))?;

          let current_try_attr = {
            let mut attr_path = attribute.clone();
            attr_path.push(attr_name.clone());
            attr_path
          };
          tried.push(current_try_attr.clone());

          if check_res.map(|s| s.trim().to_owned()).as_deref() == Some("true") {
            debug!("Using automatically detected configuration: {}", attr_name);
            attribute.push(attr_name);
            if push_drv {
              attribute.extend(toplevel.clone());
            }
            found_config = true;
            break;
          }
        }

        // If still not found after automatic detection, error out
        if !found_config {
          let tried_str = tried
            .into_iter()
            .map(|a| {
              Installable::Flake {
                reference: flake_reference.clone(),
                attribute: a,
              }
              .to_args()
              .join(" ")
            })
            .collect::<Vec<_>>()
            .join(", ");
          bail!(
            "Couldn't find home-manager configuration automatically, tried: \
             {tried_str}"
          );
        }
      }
    },
    Installable::File {
      ref mut attribute, ..
    }
    | Installable::Expression {
      ref mut attribute, ..
    } => {
      if push_drv {
        attribute.extend(toplevel);
      }
    },
    Installable::Store { .. } => {},
    #[allow(clippy::unreachable, reason = "Should never happen")]
    Installable::Unspecified => {
      unreachable!(
        "Unspecified installable should have been resolved before calling \
         toplevel_for"
      )
    },
  }

  Ok(res)
}

impl HomeReplArgs {
  fn run(self) -> Result<()> {
    let installable = self.installable.resolve(CommandContext::Home)?;

    let installable = match installable {
      Installable::Unspecified => Installable::try_find_default_for_home()?,
      other => other,
    };

    let toplevel = toplevel_for(
      installable,
      false,
      &self.extra_args,
      self.configuration.clone(),
    )?;

    Command::new("nix")
      .with_required_env()
      .arg("repl")
      .args(toplevel.to_args())
      .show_output(true)
      .run()?;

    Ok(())
  }
}

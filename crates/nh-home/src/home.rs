pub mod args;

use std::{convert::Into, env, ffi::OsString, path::PathBuf};

use args::{HomeRebuildArgs, HomeReplArgs, HomeSubcommand};
use color_eyre::{
  Result,
  eyre::{Context, bail, eyre},
};
use nh_core::{
  command::{self, Command, CommandKind, NixCommand},
  update::update_with_args,
  util::{get_hostname, use_nom},
};
use nh_diff::print_dix_diff;
use nh_installable::{
  CommandContext,
  ConfigurationInstallable,
  ConfigurationLayout,
  Installable,
};
use nh_remote::{self, RemoteBuildConfig};
use tracing::{debug, info, warn};

fn capture_nix_stdout(command: &NixCommand) -> Result<String> {
  let output = command.output().wrap_err("Failed to run nix command")?;
  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
      bail!("nix command failed (exit status {:?})", output.status);
    }
    bail!(
      "nix command failed (exit status {:?})\nstderr:\n{stderr}",
      output.status
    );
  }

  String::from_utf8(output.stdout)
    .wrap_err("nix command emitted non-UTF-8 stdout")
}

impl args::HomeArgs {
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
      .resolve_or_default(CommandContext::Home)?;

    if self.update_args.update_all || self.update_args.update_input.is_some() {
      update_with_args(
        &installable,
        self.update_args.update_input,
        &self.common.passthrough,
      )?;
    }

    let eval_args = self
      .extra_args
      .iter()
      .cloned()
      .chain(self.common.passthrough.generate_evaluation_args());
    let toplevel =
      toplevel_for(installable, true, eval_args, self.configuration.clone())?;

    // If a build host is specified, use remote build semantics
    if let Some(build_host) = self.build_host {
      info!("Building Home-Manager configuration");

      let config = RemoteBuildConfig {
        build_host,
        target_host: None,
        use_nom: use_nom(self.common.no_nom),
        use_substitutes: self.common.passthrough.use_substitutes
          && !self.common.passthrough.network_restricted(),
        execution_args: self
          .extra_args
          .iter()
          .map(Into::into)
          .chain(
            self
              .common
              .passthrough
              .generate_remote_build_args()
              .into_iter()
              .map(Into::into),
          )
          .collect(),
      };

      // Initialize SSH control - guard will cleanup connections on drop
      let _ssh_guard = nh_remote::init_ssh_control();

      nh_remote::build_remote_with_args(
        &toplevel,
        &config,
        Some(&out_path),
        &self.common.passthrough.generate_evaluation_args(),
      )
      .wrap_err("Failed to build Home-Manager configuration")?;
    } else {
      command::Build::new(toplevel)
        .extra_arg("--out-link")
        .extra_arg(&out_path)
        .extra_args(&self.extra_args)
        .passthrough(&self.common.passthrough)
        .message("Building Home-Manager configuration")
        .nom(use_nom(self.common.no_nom))
        .run()
        .wrap_err("Failed to build Home-Manager configuration")?;
    }

    let username =
      env::var("USER").map_err(|_| eyre!("Couldn't get username"))?;
    let home_dir =
      env::var("HOME").map_err(|_| eyre!("Couldn't get home directory"))?;
    let state_home = env::var("XDG_STATE_HOME")
      .unwrap_or_else(|_| format!("{home_dir}/.local/state"));
    let data_home = env::var("XDG_DATA_HOME")
      .unwrap_or_else(|_| format!("{home_dir}/.local/share"));

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
        nh_core::args::DiffType::Never => {
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
  let extra_args: Vec<OsString> = extra_args
    .into_iter()
    .map(|a| a.as_ref().to_owned())
    .collect();

  // A build needs the activation package. `nh home repl` stops at the
  // configuration itself.
  let build_attr: &[&str] = if push_drv {
    &["config", "home", "activationPackage"]
  } else {
    &[]
  };
  let layout = ConfigurationLayout {
    set: "homeConfigurations",
    build_attr,
  };

  let mut res = installable;

  // A flake without a configuration name in its attribute needs the name
  // discovered (from the `--configuration` flag or the current user/host)
  // before it can be resolved. Every other case, including an explicit
  // `.#name`, is a pure rewrite.
  if let Installable::Flake {
    reference,
    attribute,
  } = &res
    && !names_a_configuration(attribute)
  {
    let name =
      discover_home_configuration(reference, &extra_args, configuration_name)?;
    res.resolve_configuration(layout, Some(&name))?;
  } else {
    res.resolve_configuration(layout, None)?;
  }

  Ok(res)
}

/// Whether a flake attribute already names a Home Manager configuration, as
/// opposed to being empty or a bare `homeConfigurations`.
fn names_a_configuration(attribute: &[String]) -> bool {
  match attribute.split_first() {
    Some((first, rest)) if first == "homeConfigurations" => !rest.is_empty(),
    Some(_) => true,
    None => false,
  }
}

/// Finds the Home Manager configuration to build when the flake attribute does
/// not name one.
///
/// A name given via `--configuration` must exist. Otherwise this tries
/// `user@host`, then `user`, against the flake's `homeConfigurations`.
///
/// # Errors
///
/// Returns an error when a `nix eval` probe fails, when an explicitly named
/// configuration is absent, or when no configuration matches the current user
/// and host.
fn discover_home_configuration(
  reference: &str,
  extra_args: &[OsString],
  configuration_name: Option<String>,
) -> Result<String> {
  let exists = |candidate: &str| -> Result<bool> {
    let probe = Installable::Flake {
      reference: reference.to_owned(),
      attribute: vec![String::from("homeConfigurations")],
    };
    let output = capture_nix_stdout(
      &NixCommand::new(CommandKind::Eval)
        .with_required_env()
        .args(extra_args)
        .arg("--apply")
        .arg(format!(r#" x: x ? "{candidate}" "#))
        .args(probe.to_args()?),
    )
    .wrap_err(format!(
      "Failed running nix eval to check for configuration '{candidate}'"
    ))?;
    Ok(output.trim() == "true")
  };

  if let Some(name) = configuration_name {
    if exists(&name)? {
      debug!("Using explicit configuration from flag: {name:?}");
      return Ok(name);
    }
    bail!("Explicitly specified home-manager configuration not found: {name}");
  }

  let username =
    std::env::var("USER").map_err(|_| eyre!("Couldn't get username"))?;
  let hostname = get_hostname(None)?;
  let candidates = [format!("{username}@{hostname}"), username];

  for candidate in &candidates {
    if exists(candidate)? {
      debug!("Using automatically detected configuration: {candidate}");
      return Ok(candidate.clone());
    }
  }

  bail!(
    "Couldn't find home-manager configuration automatically, tried: {}",
    candidates.join(", ")
  )
}

impl HomeReplArgs {
  fn run(self) -> Result<()> {
    let installable =
      self.installable.resolve_or_default(CommandContext::Home)?;

    let toplevel = toplevel_for(
      installable,
      false,
      &self.extra_args,
      self.configuration.clone(),
    )?;

    let status = NixCommand::new(CommandKind::Repl)
      .args(toplevel.to_args()?)
      .with_required_env()
      .run_with_logs()?;
    if !status.success() {
      bail!("nix repl failed (exit status {status:?})");
    }

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use nh_installable::Installable;

  use super::toplevel_for;

  fn flake(attribute: &[&str]) -> Installable {
    Installable::Flake {
      reference: String::from("."),
      attribute: attribute.iter().map(|s| (*s).to_string()).collect(),
    }
  }

  fn resolve(attribute: &[&str], push_drv: bool) -> Installable {
    toplevel_for(flake(attribute), push_drv, Vec::<String>::new(), None)
      .expect("explicit attribute should resolve")
  }

  fn attribute_of(installable: &Installable) -> Vec<String> {
    match installable {
      Installable::Flake { attribute, .. } => attribute.clone(),
      other => panic!("expected a flake installable, got {other:?}"),
    }
  }

  #[test]
  fn single_name_gets_home_configurations_prefix_and_activation_package() {
    let resolved = resolve(&["jacob@odyssey"], true);
    assert_eq!(attribute_of(&resolved), [
      "homeConfigurations",
      "jacob@odyssey",
      "config",
      "home",
      "activationPackage",
    ]);
  }

  #[test]
  fn repl_stops_at_the_configuration_without_activation_package() {
    let resolved = resolve(&["jacob@odyssey"], false);
    assert_eq!(attribute_of(&resolved), [
      "homeConfigurations",
      "jacob@odyssey"
    ]);
  }

  #[test]
  fn explicit_home_configurations_prefix_is_accepted() {
    let resolved = resolve(&["homeConfigurations", "jacob@odyssey"], false);
    assert_eq!(attribute_of(&resolved), [
      "homeConfigurations",
      "jacob@odyssey"
    ]);
  }

  #[test]
  fn nested_attribute_is_rejected_as_too_specific() {
    let err =
      toplevel_for(flake(&["foo", "bar"]), true, Vec::<String>::new(), None)
        .expect_err("nested attribute should be rejected");
    assert!(err.to_string().contains("too specific"));
  }
}

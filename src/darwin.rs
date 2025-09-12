use std::{env, fmt, path::PathBuf};

use color_eyre::eyre::{Context, bail, eyre};
use tracing::{debug, warn};

use crate::{
  Result,
  commands::{self, Command, ElevationStrategy},
  installable::Installable,
  interface::{
    DarwinArgs,
    DarwinRebuildArgs,
    DarwinReplArgs,
    DarwinSubcommand,
    DiffType,
    NotifyAskMode,
  },
  nixos::toplevel_for,
  notify::NotificationSender,
  update::update,
  util::{get_hostname, print_dix_diff},
};

const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
const CURRENT_PROFILE: &str = "/run/current-system";

impl DarwinArgs {
  /// Run the `darwin` subcommand.
  ///
  /// # Errors
  ///
  /// Returns an error if the operation fails.
  pub fn run(self, elevation: ElevationStrategy) -> Result<()> {
    use DarwinRebuildVariant::{Build, Switch};
    match self.subcommand {
      DarwinSubcommand::Switch(args) => args.rebuild(&Switch, elevation),
      DarwinSubcommand::Build(args) => {
        if args.common.ask.is_some() || args.common.dry {
          warn!("`--ask` and `--dry` have no effect for `nh darwin build`");
        }
        args.rebuild(&Build, elevation)
      },
      DarwinSubcommand::Repl(args) => args.run(),
    }
  }
}

enum DarwinRebuildVariant {
  Switch,
  Build,
}

impl fmt::Display for DarwinRebuildVariant {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let s = match self {
      DarwinRebuildVariant::Build => "build",
      DarwinRebuildVariant::Switch => "switch",
    };
    write!(f, "{s}")
  }
}

impl DarwinRebuildArgs {
  fn rebuild(
    self,
    variant: &DarwinRebuildVariant,
    elevation: ElevationStrategy,
  ) -> Result<()> {
    use DarwinRebuildVariant::{Build, Switch};

    if nix::unistd::Uid::effective().is_root() && !self.bypass_root_check {
      bail!("Don't run nh os as root. I will call sudo internally as needed");
    }

    if self.update_args.update_all || self.update_args.update_input.is_some() {
      update(&self.common.installable, self.update_args.update_input)?;
    }

    let hostname = self.hostname.ok_or(()).or_else(|()| get_hostname())?;

    let (out_path, _tempdir_guard): (PathBuf, Option<tempfile::TempDir>) =
      if let Some(ref p) = self.common.out_link {
        (p.clone(), None)
      } else {
        let dir = tempfile::Builder::new().prefix("nh-os").tempdir()?;
        (dir.as_ref().join("result"), Some(dir))
      };

    debug!("Output path: {out_path:?}");

    // Use NH_DARWIN_FLAKE if available, otherwise use the provided installable
    let installable = if let Ok(darwin_flake) = env::var("NH_DARWIN_FLAKE") {
      debug!("Using NH_DARWIN_FLAKE: {}", darwin_flake);

      let mut elems = darwin_flake.splitn(2, '#');
      let reference = match elems.next() {
        Some(r) => r.to_owned(),
        None => return Err(eyre!("NH_DARWIN_FLAKE missing reference part")),
      };
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
      // If user explicitly selects some other attribute, don't push
      // darwinConfigurations
      if attribute.is_empty() {
        attribute.push(String::from("darwinConfigurations"));
        attribute.push(hostname.clone());
      }
    }

    let toplevel = toplevel_for(hostname, processed_installable, "toplevel");

    commands::Build::new(toplevel)
      .extra_arg("--out-link")
      .extra_arg(&out_path)
      .extra_args(&self.extra_args)
      .passthrough(&self.common.passthrough)
      .message("Building Darwin configuration")
      .nom(!self.common.no_nom)
      .run()
      .wrap_err("Failed to build Darwin configuration")?;

    let target_profile = out_path.clone();

    target_profile.try_exists().context("Doesn't exist")?;

    debug!(
      "Comparing with target profile: {}",
      target_profile.display()
    );

    // Compare changes between current and target generation
    if matches!(self.common.diff, DiffType::Never) {
      debug!("Not running dix as the --diff flag is set to never.");
    } else {
      debug!(
        "Comparing with target profile: {}",
        target_profile.display()
      );
      let _ = print_dix_diff(&PathBuf::from(CURRENT_PROFILE), &target_profile);
    }

    if !self.common.dry && !matches!(variant, Build) {
      if let Some(ask) = self.common.ask {
        let confirmation = match ask {
          NotifyAskMode::Prompt => {
            inquire::Confirm::new("Apply the config?")
              .with_default(false)
              .prompt()?
          },
          // MacOS doesn't support notification actions
          NotifyAskMode::Notify | NotifyAskMode::Both => {
            NotificationSender::new(&format!("nh darwin {variant}"), "testing")
              .send()
              .unwrap();

            inquire::Confirm::new("Apply the config?")
              .with_default(false)
              .prompt()?
          },
        };

        if !confirmation {
          bail!("User rejected the new config");
        }
      }
    }

    if matches!(variant, Switch) {
      Command::new("nix")
        .args(["build", "--no-link", "--profile", SYSTEM_PROFILE])
        .arg(&out_path)
        .elevate(Some(elevation.clone()))
        .dry(self.common.dry)
        .with_required_env()
        .run()
        .wrap_err("Failed to set Darwin system profile")?;

      let darwin_rebuild = out_path.join("sw/bin/darwin-rebuild");
      let activate_user = out_path.join("activate-user");

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
        .elevate(needs_elevation.then_some(elevation))
        .dry(self.common.dry)
        .show_output(true)
        .with_required_env()
        .run()
        .wrap_err("Darwin activation failed")?;
    }

    debug!("Completed operation with output path: {out_path:?}");

    Ok(())
  }
}

impl DarwinReplArgs {
  fn run(self) -> Result<()> {
    // Use NH_DARWIN_FLAKE if available, otherwise use the provided installable
    let mut target_installable =
      if let Ok(darwin_flake) = env::var("NH_DARWIN_FLAKE") {
        debug!("Using NH_DARWIN_FLAKE: {}", darwin_flake);

        let mut elems = darwin_flake.splitn(2, '#');
        let reference = match elems.next() {
          Some(r) => r.to_owned(),
          None => return Err(eyre!("NH_DARWIN_FLAKE missing reference part")),
        };
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
      .with_required_env()
      .show_output(true)
      .run()?;

    Ok(())
  }
}

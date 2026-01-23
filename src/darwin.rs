use std::{
  convert::Into,
  env,
  path::{Path, PathBuf},
};

use color_eyre::eyre::{Context, Result, bail, eyre};
use tracing::{debug, info, warn};

use crate::{
  commands,
  commands::{Command, ElevationStrategy},
  installable::Installable,
  interface::{
    DarwinArgs,
    DarwinRebuildArgs,
    DarwinReplArgs,
    DarwinSubcommand,
    DiffType,
  },
  remote,
  update::update,
  util::{ensure_ssh_key_login, get_hostname, print_dix_diff},
};

const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";
const CURRENT_PROFILE: &str = "/run/current-system";

/// Essential files that must exist in a valid Darwin system closure. Each tuple
/// contains the file path relative to the system profile and its description.
/// The descriptions are used on log messages or errors.
const ESSENTIAL_FILES: &[(&str, &str)] = &[
  ("sw/bin/darwin-rebuild", "activation script"),
  ("activate", "system activation script"),
  ("sw/bin", "system path"),
];

impl DarwinArgs {
  /// Run the `darwin` subcommand.
  ///
  /// # Parameters
  ///
  /// * `self` - The Darwin operation arguments
  /// * `elevation` - The privilege elevation strategy (sudo/doas/none)
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
  #[cfg_attr(feature = "hotpath", hotpath::measure)]
  pub fn run(self, elevation: ElevationStrategy) -> Result<()> {
    use DarwinRebuildVariant::{Build, Switch};
    match self.subcommand {
      DarwinSubcommand::Switch(args) => {
        args.rebuild_and_activate(&Switch, None, elevation)
      },
      DarwinSubcommand::Build(args) => {
        if args.common.ask || args.common.dry {
          warn!("`--ask` and `--dry` have no effect for `nh darwin build`");
        }
        args.rebuild_and_activate(&Build, None, elevation)
      },
      DarwinSubcommand::Repl(args) => args.run(),
    }
  }
}

#[derive(Debug)]
enum DarwinRebuildVariant {
  Switch,
  Build,
}

impl DarwinRebuildArgs {
  // final_attr is the attribute of config.system.build.X to evaluate.
  fn rebuild_and_activate(
    self,
    variant: &DarwinRebuildVariant,
    final_attr: Option<&String>,
    elevation: ElevationStrategy,
  ) -> Result<()> {
    use DarwinRebuildVariant::Build;

    let (elevate, target_hostname) = self.setup_build_context(&elevation)?;

    let (out_path, _tempdir_guard) = self.determine_output_path(variant)?;

    let toplevel =
      self.resolve_installable_and_toplevel(&target_hostname, final_attr)?;

    let message = "Building Darwin configuration";

    // Initialize SSH control early if we have remote hosts - guard will keep
    // connections alive for both build and activation
    let _ssh_guard = if self.build_host.is_some() || self.target_host.is_some()
    {
      Some(remote::init_ssh_control())
    } else {
      None
    };

    let actual_store_path = self.execute_build(toplevel, &out_path, message)?;

    let target_profile = out_path.clone();

    self.handle_dix_diff(&target_profile);

    if self.common.dry || matches!(variant, Build) {
      if self.common.ask {
        warn!("--ask has no effect as dry run was requested");
      }

      return Ok(());
    }

    self.activate_rebuilt_config(
      variant,
      &out_path,
      &target_profile,
      actual_store_path.as_deref(),
      elevate,
      elevation,
    )?;

    Ok(())
  }

  fn activate_rebuilt_config(
    &self,
    variant: &DarwinRebuildVariant,
    out_path: &Path,
    target_profile: &Path,
    actual_store_path: Option<&Path>,
    elevate: bool,
    elevation: ElevationStrategy,
  ) -> Result<()> {
    use DarwinRebuildVariant::Switch;

    if self.common.ask {
      let confirmation = inquire::Confirm::new("Apply the config?")
        .with_default(false)
        .prompt()?;

      if !confirmation {
        bail!("User rejected the new config");
      }
    }

    if let Some(target_host) = &self.target_host {
      // Only copy if the output path exists locally (i.e., was copied back from
      // remote build)
      if out_path.exists() {
        let target = remote::RemoteHost::parse(target_host)
          .wrap_err("Invalid target host specification")?;
        remote::copy_to_remote(
          &target,
          target_profile,
          self.common.passthrough.use_substitutes,
        )
        .context("Failed to copy configuration to target host")?;
      }
    }

    // Validate system closure before activation, unless bypassed. For remote
    // builds, use the actual store path returned from the build. For local
    // builds, canonicalize the target_profile.
    let is_remote_build = self.target_host.is_some();
    let resolved_profile: PathBuf = if let Some(store_path) = actual_store_path
    {
      // Remote build - use the actual store path from the build output
      store_path.to_path_buf()
    } else if is_remote_build && !out_path.exists() {
      // Remote build with no local result and no store path captured
      // (shouldn't happen, but fallback)
      target_profile.to_path_buf()
    } else {
      // Local build - canonicalize the symlink to get the store path
      target_profile
        .canonicalize()
        .context("Failed to resolve output path to actual store path")?
    };

    let should_skip = self.no_validate;

    if should_skip {
      warn!(
        "Skipping pre-activation validation (--no-validate or NH_NO_VALIDATE \
         set)"
      );
      warn!(
        "This may result in activation failures if the system closure is \
         incomplete"
      );
    } else if let Some(target_host) = &self.target_host {
      // For remote activation, validate on the remote host using the resolved
      // store path
      validate_system_closure_remote(
        &resolved_profile,
        target_host,
        self.build_host.as_deref(),
      )?;
    } else {
      // For local activation, validate locally
      validate_system_closure(&resolved_profile)?;
    }

    if matches!(variant, Switch) {
      if let Some(target_host) = &self.target_host {
        let target = remote::RemoteHost::parse(target_host)
          .wrap_err("Invalid target host specification")?;

        remote::activate_remote(
          &target,
          &resolved_profile,
          &remote::ActivateRemoteConfig {
            platform:           remote::Platform::Darwin,
            activation_type:    remote::ActivationType::Switch,
            install_bootloader: false,
            show_logs:          self.show_activation_logs,
            elevation:          elevate.then_some(elevation),
          },
        )
        .wrap_err("Activation failed")?;
      } else {
        Command::new("nix")
          .args(["build", "--no-link", "--profile", SYSTEM_PROFILE])
          .arg(out_path)
          .elevate(elevate.then_some(elevation.clone()))
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
          .show_output(self.show_activation_logs)
          .with_required_env()
          .run()
          .wrap_err("Darwin activation failed")?;
      }
    }

    if let Some(store_path) = actual_store_path {
      debug!("Completed {variant:?} operation with store path: {store_path:?}");
    } else {
      debug!(
        "Completed {variant:?} operation with local output path: {out_path:?}"
      );
    }

    Ok(())
  }

  /// Performs initial setup and gathers context for a Darwin rebuild operation.
  ///
  /// This includes:
  /// - Ensuring SSH key login if a remote build/target host is involved.
  /// - Checking and determining elevation status.
  /// - Performing updates to Nix inputs if specified.
  /// - Resolving the target hostname for the build.
  ///
  /// # Returns
  ///
  /// `Result` containing a tuple:
  ///
  /// - `bool`: `true` if elevation is required, `false` otherwise.
  /// - `String`: The resolved target hostname.
  fn setup_build_context(
    &self,
    elevation: &ElevationStrategy,
  ) -> Result<(bool, String)> {
    // Only check SSH key login if remote hosts are involved
    if self.build_host.is_some() || self.target_host.is_some() {
      ensure_ssh_key_login()?;
    }

    let elevate = has_elevation_status(self.bypass_root_check, elevation)?;

    if self.update_args.update_all || self.update_args.update_input.is_some() {
      update(
        &self.common.installable,
        self.update_args.update_input.clone(),
      )?;
    }

    let target_hostname = get_hostname(self.hostname.clone())?;
    Ok((elevate, target_hostname))
  }

  fn determine_output_path(
    &self,
    variant: &DarwinRebuildVariant,
  ) -> Result<(PathBuf, Option<tempfile::TempDir>)> {
    use DarwinRebuildVariant::Build;
    if let Some(p) = self.common.out_link.clone() {
      Ok((p, None))
    } else {
      let (path, guard) = if matches!(variant, Build) {
        (PathBuf::from("result"), None)
      } else {
        let dir = tempfile::Builder::new().prefix("nh-darwin").tempdir()?;
        (dir.as_ref().join("result"), Some(dir))
      };
      Ok((path, guard))
    }
  }

  fn resolve_installable_and_toplevel(
    &self,
    target_hostname: &str,
    final_attr: Option<&String>,
  ) -> Result<Installable> {
    let installable = (get_nh_darwin_flake_env()?)
      .unwrap_or_else(|| self.common.installable.clone());

    let installable = match installable {
      Installable::Unspecified => Installable::try_find_default_for_darwin()?,
      other => other,
    };

    toplevel_for(
      target_hostname,
      installable,
      final_attr.map_or("toplevel", |v| v),
    )
  }

  fn execute_build(
    &self,
    toplevel: Installable,
    out_path: &Path,
    message: &str,
  ) -> Result<Option<PathBuf>> {
    // If a build host is specified, use proper remote build semantics:
    //
    // 1. Evaluate derivation locally
    // 2. Copy derivation to build host (user-initiated SSH)
    // 3. Build on remote host
    // 4. Copy result back (to localhost or target_host)
    if let Some(ref build_host_str) = self.build_host {
      info!("{message}");

      let build_host = remote::RemoteHost::parse(build_host_str)
        .wrap_err("Invalid build host specification")?;

      let target_host = self
        .target_host
        .as_ref()
        .map(|s| remote::RemoteHost::parse(s))
        .transpose()
        .wrap_err("Invalid target host specification")?;

      let config = remote::RemoteBuildConfig {
        build_host,
        target_host,
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

      let actual_store_path =
        remote::build_remote(&toplevel, &config, Some(out_path))?;

      Ok(Some(actual_store_path))
    } else {
      // Local build - use the existing path
      commands::Build::new(toplevel)
        .extra_arg("--out-link")
        .extra_arg(out_path)
        .extra_args(&self.extra_args)
        .passthrough(&self.common.passthrough)
        .message(message)
        .nom(!self.common.no_nom)
        .run()
        .wrap_err("Failed to build configuration")?;

      Ok(None) // Local builds don't have separate store path
    }
  }

  fn handle_dix_diff(&self, target_profile: &Path) {
    match self.common.diff {
      DiffType::Always => {
        let _ = print_dix_diff(&PathBuf::from(CURRENT_PROFILE), target_profile);
      },
      DiffType::Never => {
        debug!("Not running dix as the --diff flag is set to never.");
      },
      DiffType::Auto => {
        // Only run dix if no explicit hostname was provided and no remote
        // build/target host is specified, implying a local system build.
        if self.hostname.is_none()
          && self.target_host.is_none()
          && self.build_host.is_none()
        {
          debug!(
            "Comparing with target profile: {}",
            target_profile.display()
          );
          let _ =
            print_dix_diff(&PathBuf::from(CURRENT_PROFILE), target_profile);
        } else {
          debug!(
            "Not running dix as a remote host is involved or an explicit \
             hostname was provided."
          );
        }
      },
    }
  }
}

impl DarwinReplArgs {
  fn run(self) -> Result<()> {
    // Use NH_DARWIN_FLAKE if available, otherwise use the provided installable
    let target_installable =
      if let Some(flake_installable) = get_nh_darwin_flake_env()? {
        flake_installable
      } else {
        self.installable
      };

    let mut target_installable = match target_installable {
      Installable::Unspecified => Installable::try_find_default_for_darwin()?,
      other => other,
    };

    if matches!(target_installable, Installable::Store { .. }) {
      bail!("Nix doesn't support nix store installables.");
    }

    let hostname = get_hostname(self.hostname)?;

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

/// Validates that essential files exist in the system closure.
///
/// Checks for a few critical files that must be present in a complete Darwin
/// system.
///
/// - sw/bin/darwin-rebuild: activation script
/// - activate: system activation script
/// - sw/bin: system path binaries
///
/// # Returns
///
/// `Ok(())` if all files exist, or an error listing missing files.
fn validate_system_closure(system_path: &Path) -> Result<()> {
  let mut missing = Vec::new();
  for (file, description) in ESSENTIAL_FILES {
    let path = system_path.join(file);
    if !path.exists() {
      missing.push(format!("  - {file} ({description})"));
    }
  }

  if !missing.is_empty() {
    let missing_list = missing.join("\n");
    return Err(eyre!(
      "System closure validation failed. Missing essential files:\n{}\n\nThis \
       typically happens when:\n1. Required system components are disabled in \
       your configuration\n2. The build was incomplete or corrupted\n3. \
       You're using an incomplete derivation\n\nTo fix this:\n1. Verify your \
       configuration enables all required components\n2. Rebuild your system \
       configuration\n3. If the problem persists, verify your system closure \
       is complete\n\nSystem path checked: {}",
      missing_list,
      system_path.display()
    ));
  }

  Ok(())
}

/// Validates essential files on a remote host via SSH.
///
/// Similar to [`validate_system_closure`] but executes checks on a remote host.
fn validate_system_closure_remote(
  system_path: &Path,
  target_host: &str,
  build_host: Option<&str>,
) -> Result<()> {
  let target = remote::RemoteHost::parse(target_host)
    .wrap_err("Invalid target host specification")?;

  // Build context string for error messages
  let context = build_host.map(|build| {
    if build == target_host {
      "also build host".to_string()
    } else {
      format!("built on '{build}'")
    }
  });

  // Delegate to the generic remote validation function
  remote::validate_closure_remote(
    &target,
    system_path,
    ESSENTIAL_FILES,
    context.as_deref(),
  )
}

/// Parses the `NH_DARWIN_FLAKE` environment variable into an
/// `Installable::Flake`.
///
/// If `NH_DARWIN_FLAKE` is not set, it returns `Ok(None)`.
/// If `NH_DARWIN_FLAKE` is set but invalid, it returns an `Err`.
fn get_nh_darwin_flake_env() -> Result<Option<Installable>> {
  if let Ok(darwin_flake) = env::var("NH_DARWIN_FLAKE") {
    debug!("Using NH_DARWIN_FLAKE: {}", darwin_flake);

    let mut elems = darwin_flake.splitn(2, '#');
    let reference = elems
      .next()
      .ok_or_else(|| eyre!("NH_DARWIN_FLAKE missing reference part"))?
      .to_owned();
    let attribute = elems
      .next()
      .map(crate::installable::parse_attribute)
      .unwrap_or_default();

    Ok(Some(Installable::Flake {
      reference,
      attribute,
    }))
  } else {
    Ok(None)
  }
}

/// Checks if the current user is root and returns whether elevation is needed.
///
/// Returns `true` if elevation is required (not root and `bypass_root_check` is
/// false). Returns `false` if elevation is not required (root or
/// `bypass_root_check` is true).
///
/// # Arguments
///
/// * `bypass_root_check` - If true, bypasses the root check and assumes no
///   elevation is needed.
///
/// # Errors
///
/// Returns an error if `bypass_root_check` is false and the user is root,
/// as `nh darwin` subcommands should not be run directly as root.
fn has_elevation_status(
  bypass_root_check: bool,
  elevation: &commands::ElevationStrategy,
) -> Result<bool> {
  // If elevation strategy is None, never elevate
  if matches!(elevation, commands::ElevationStrategy::None) {
    return Ok(false);
  }

  if bypass_root_check {
    warn!("Bypassing root check, now running nix as root");
    Ok(false)
  } else {
    if nix::unistd::Uid::effective().is_root() {
      bail!(
        "Don't run nh darwin as root. It will escalate its privileges \
         internally as needed."
      );
    }
    Ok(true)
  }
}

pub fn toplevel_for<S: AsRef<str>>(
  hostname: S,
  installable: Installable,
  final_attr: &str,
) -> Result<Installable> {
  let mut res = installable;
  let hostname_str = hostname.as_ref();

  let toplevel = ["config", "system", "build", final_attr]
    .into_iter()
    .map(String::from);

  match res {
    Installable::Flake {
      ref mut attribute, ..
    } => {
      if attribute.is_empty() {
        attribute.push(String::from("darwinConfigurations"));
        attribute.push(hostname_str.to_owned());
      } else if attribute.len() == 1 && attribute[0] == "darwinConfigurations" {
        info!(
          "Inferring hostname '{}' for darwinConfigurations",
          hostname_str
        );
        attribute.push(hostname_str.to_owned());
      } else if attribute[0] == "darwinConfigurations" {
        if attribute.len() == 2 {
          // darwinConfigurations.hostname - fine
        } else if attribute.len() > 2 {
          bail!(
            "Attribute path is too specific: {}. Please either:\n  1. Use the \
             flake reference without attributes (e.g., '.')\n  2. Specify \
             only the configuration name (e.g., '.#{}')",
            attribute.join("."),
            attribute[1]
          );
        }
      } else {
        // User provided ".#myhost" - prepend darwinConfigurations
        attribute.insert(0, String::from("darwinConfigurations"));
      }
      attribute.extend(toplevel);
    },
    Installable::File {
      ref mut attribute, ..
    }
    | Installable::Expression {
      ref mut attribute, ..
    } => attribute.extend(toplevel),

    Installable::Store { .. } => {},

    Installable::Unspecified => {
      unreachable!(
        "Unspecified installable should have been resolved before calling \
         toplevel_for"
      )
    },
  }

  Ok(res)
}

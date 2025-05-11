use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use color_eyre::eyre::bail;
use color_eyre::Result;
use tracing::{debug, info, warn};

use crate::commands;
use crate::installable::Installable;

/// Resolves an Installable from an environment variable, or falls back to the provided one.
pub fn resolve_env_installable(var: &str, fallback: Installable) -> Installable {
    if let Ok(val) = env::var(var) {
        let mut elems = val.splitn(2, '#');
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
        fallback
    }
}

/// Extends an Installable with the appropriate attribute path for a platform.
///
/// - `config_type`: e.g. "homeConfigurations", "nixosConfigurations", "darwinConfigurations"
/// - `extra_path`: e.g. ["config", "home", "activationPackage"]
/// - `config_name`: Optional configuration name (e.g. username@hostname)
/// - `push_drv`: Whether to push the drv path (platform-specific)
/// - `extra_args`: Extra args for nix eval (for config detection)
pub fn extend_installable_for_platform(
    mut installable: Installable,
    config_type: &str,
    extra_path: &[&str],
    config_name: Option<String>,
    push_drv: bool,
    extra_args: &[OsString],
) -> Result<Installable> {
    use tracing::debug;

    use crate::commands;
    use crate::util::get_hostname;
    match &mut installable {
        Installable::Flake {
            reference,
            attribute,
        } => {
            if !attribute.is_empty() {
                debug!(
                    "Using explicit attribute path from installable: {:?}",
                    attribute
                );
                return Ok(installable);
            }
            attribute.push(config_type.to_string());
            let flake_reference = reference.clone();
            let mut found_config = false;
            if let Some(config_name) = config_name {
                let func = format!(r#"x: x ? "{config_name}""#);
                let check_res = commands::Command::new("nix")
                    .arg("eval")
                    .args(extra_args)
                    .arg("--apply")
                    .arg(&func)
                    .args(
                        (Installable::Flake {
                            reference: flake_reference.clone(),
                            attribute: attribute.clone(),
                        })
                        .to_args(),
                    )
                    .run_capture();
                if let Ok(res) = check_res {
                    if res.map(|s| s.trim().to_owned()).as_deref() == Some("true") {
                        debug!("Using explicit configuration from flag: {}", config_name);
                        attribute.push(config_name);
                        if push_drv {
                            attribute.extend(extra_path.iter().map(|s| (*s).to_string()));
                        }
                        found_config = true;
                    }
                }
                if !found_config {
                    return Err(color_eyre::eyre::eyre!(
                        "Explicitly specified configuration not found in flake."
                    ));
                }
            }
            if !found_config {
                // Try to auto-detect the configuration if none was specified
                let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
                let hostname = get_hostname().unwrap_or_else(|_| "host".to_string());
                for attr_name in [format!("{username}@{hostname}"), username] {
                    let func = format!(r#"x: x ? "{attr_name}""#);
                    let check_res = commands::Command::new("nix")
                        .arg("eval")
                        .args(extra_args)
                        .arg("--apply")
                        .arg(&func)
                        .args(
                            (Installable::Flake {
                                reference: flake_reference.clone(),
                                attribute: attribute.clone(),
                            })
                            .to_args(),
                        )
                        .run_capture();
                    if let Ok(res) = check_res {
                        if res.map(|s| s.trim().to_owned()).as_deref() == Some("true") {
                            debug!("Using automatically detected configuration: {}", attr_name);
                            attribute.push(attr_name);
                            if push_drv {
                                attribute.extend(extra_path.iter().map(|s| (*s).to_string()));
                            }
                            found_config = true;
                            break;
                        }
                    }
                }
                if !found_config {
                    return Err(color_eyre::eyre::eyre!(
                        "Couldn't find configuration automatically in flake."
                    ));
                }
            }
        }
        Installable::File { attribute, .. } | Installable::Expression { attribute, .. } => {
            if push_drv {
                attribute.extend(extra_path.iter().map(|s| (*s).to_string()));
            }
        }
        Installable::Store { .. } => {
            // Nothing to do for store paths
        }
    }
    Ok(installable)
}

/// Handles common specialisation logic for all platforms
pub fn handle_specialisation(
    specialisation_path: &str,
    no_specialisation: bool,
    explicit_specialisation: Option<String>,
) -> Option<String> {
    if no_specialisation {
        None
    } else {
        let current_specialisation = std::fs::read_to_string(specialisation_path).ok();
        explicit_specialisation.or(current_specialisation)
    }
}

/// Checks if the user wants to proceed with applying the configuration
pub fn confirm_action(ask: bool, dry: bool) -> Result<bool> {
    use tracing::{info, warn};

    if dry {
        if ask {
            warn!("--ask has no effect as dry run was requested");
        }
        return Ok(false);
    }

    if ask {
        info!("Apply the config?");
        let confirmation = dialoguer::Confirm::new().default(false).interact()?;

        if !confirmation {
            bail!("User rejected the new config");
        }
    }

    Ok(true)
}

/// Common function to ensure we're not running as root
pub fn check_not_root(bypass_root_check: bool) -> Result<bool> {
    use tracing::warn;

    if bypass_root_check {
        warn!("Bypassing root check, now running nix as root");
        return Ok(false);
    }

    if nix::unistd::Uid::effective().is_root() {
        // Protect users from themselves
        bail!("Don't run nh os as root. I will call sudo internally as needed");
    }

    Ok(true)
}

/// Creates a temporary output path for build results
pub fn create_output_path(
    out_link: Option<impl AsRef<std::path::Path>>,
    prefix: &str,
) -> Result<Box<dyn crate::util::MaybeTempPath>> {
    let out_path: Box<dyn crate::util::MaybeTempPath> = match out_link {
        Some(ref p) => Box::new(std::path::PathBuf::from(p.as_ref())),
        None => Box::new({
            let dir = tempfile::Builder::new().prefix(prefix).tempdir()?;
            (dir.as_ref().join("result"), dir)
        }),
    };

    Ok(out_path)
}

/// Compare configurations using nvd diff
pub fn compare_configurations(
    current_profile: &str,
    target_profile: &std::path::Path,
    skip_compare: bool,
    message: &str,
) -> Result<()> {
    if skip_compare {
        debug!("Skipping configuration comparison");
        return Ok(());
    }

    commands::Command::new("nvd")
        .arg("diff")
        .arg(current_profile)
        .arg(target_profile)
        .message(message)
        .run()?;

    Ok(())
}

/// Build a configuration using the nix build command
pub fn build_configuration(
    installable: Installable,
    out_path: &dyn crate::util::MaybeTempPath,
    extra_args: &[impl AsRef<std::ffi::OsStr>],
    builder: Option<String>,
    message: &str,
    no_nom: bool,
) -> Result<()> {
    commands::Build::new(installable)
        .extra_arg("--out-link")
        .extra_arg(out_path.get_path())
        .extra_args(extra_args)
        .builder(builder)
        .message(message)
        .nom(!no_nom)
        .run()?;

    Ok(())
}

/// Determine the target profile path considering specialisation
pub fn get_target_profile(
    out_path: &dyn crate::util::MaybeTempPath,
    target_specialisation: &Option<String>,
) -> PathBuf {
    match target_specialisation {
        None => out_path.get_path().to_owned(),
        Some(spec) => out_path.get_path().join("specialisation").join(spec),
    }
}

/// Common logic for handling REPL for different platforms
pub fn run_repl(
    installable: Installable,
    config_type: &str,
    extra_path: &[&str],
    config_name: Option<String>,
    extra_args: &[String],
) -> Result<()> {
    // Store paths don't work with REPL
    if let Installable::Store { .. } = installable {
        bail!("Nix doesn't support nix store installables with repl.");
    }

    let installable = extend_installable_for_platform(
        installable,
        config_type,
        extra_path,
        config_name,
        false,
        &[],
    )?;

    debug!("Running nix repl with installable: {:?}", installable);

    // Note: Using stdlib Command directly is necessary for interactive REPL
    use std::process::{Command as StdCommand, Stdio};

    let mut command = StdCommand::new("nix");
    command.arg("repl");

    // Add installable arguments
    for arg in installable.to_args() {
        command.arg(arg);
    }

    // Add any extra arguments
    for arg in extra_args {
        command.arg(arg);
    }

    // Configure for interactive use
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    // Execute and wait for completion
    let status = command.status()?;

    if !status.success() {
        bail!("nix repl exited with non-zero status: {}", status);
    }

    Ok(())
}

/// Process the target specialisation based on common patterns
pub fn process_specialisation(
    no_specialisation: bool,
    specialisation: Option<String>,
    specialisation_path: &str,
) -> Result<Option<String>> {
    let target_specialisation =
        handle_specialisation(specialisation_path, no_specialisation, specialisation);

    debug!("target_specialisation: {target_specialisation:?}");

    Ok(target_specialisation)
}

/// Execute common actions for a rebuild operation across platforms
/// This unifies the core workflow that was previously duplicated across platforms
pub fn handle_rebuild_workflow(
    installable: Installable,
    config_type: &str,
    extra_path: &[&str],
    config_name: Option<String>,
    out_path: &dyn crate::util::MaybeTempPath,
    extra_args: &[impl AsRef<std::ffi::OsStr>],
    builder: Option<String>,
    message: &str,
    no_nom: bool,
    specialisation_path: &str,
    no_specialisation: bool,
    specialisation: Option<String>,
    current_profile: &str,
    skip_compare: bool,
) -> Result<PathBuf> {
    // Configure the installable with platform-specific attributes
    let configured_installable = extend_installable_for_platform(
        installable,
        config_type,
        extra_path,
        config_name,
        true,
        &extra_args
            .iter()
            .map(std::convert::AsRef::as_ref)
            .map(std::convert::Into::into)
            .collect::<Vec<_>>(),
    )?;

    // Build the configuration
    build_configuration(
        configured_installable,
        out_path,
        extra_args,
        builder,
        message,
        no_nom,
    )?;

    // Process any specialisations
    let target_specialisation =
        process_specialisation(no_specialisation, specialisation, specialisation_path)?;

    // Get target profile path
    let target_profile = get_target_profile(out_path, &target_specialisation);

    // Compare configurations if applicable
    if !skip_compare {
        compare_configurations(current_profile, &target_profile, false, "Comparing changes")?;
    }

    Ok(target_profile)
}

/// Determine proper hostname based on provided or automatically detected
pub fn get_target_hostname(
    explicit_hostname: Option<String>,
    skip_if_mismatch: bool,
) -> Result<(String, bool)> {
    let system_hostname = match crate::util::get_hostname() {
        Ok(hostname) => {
            debug!("Auto-detected hostname: {}", hostname);
            Some(hostname)
        }
        Err(err) => {
            warn!("Failed to detect hostname: {}", err);
            None
        }
    };

    let target_hostname = match explicit_hostname {
        Some(hostname) => hostname,
        None => match system_hostname.clone() {
            Some(hostname) => hostname,
            None => bail!("Unable to fetch hostname automatically, and no hostname supplied. Please specify explicitly.")
        }
    };

    // Skip comparison when system hostname != target hostname if requested
    let hostname_mismatch = skip_if_mismatch
        && system_hostname.is_some()
        && system_hostname.unwrap() != target_hostname;

    Ok((target_hostname, hostname_mismatch))
}

/// Common function to activate configurations in `NixOS`
pub fn activate_nixos_configuration(
    target_profile: &Path,
    variant: &str,
    target_host: Option<String>,
    elevate: bool,
    message: &str,
) -> Result<()> {
    let switch_to_configuration = target_profile.join("bin").join("switch-to-configuration");
    let switch_to_configuration = switch_to_configuration.canonicalize().map_err(|e| {
        color_eyre::eyre::eyre!("Failed to canonicalize switch-to-configuration path: {}", e)
    })?;

    commands::Command::new(switch_to_configuration)
        .arg(variant)
        .ssh(target_host)
        .message(message)
        .elevate(elevate)
        .run()
}

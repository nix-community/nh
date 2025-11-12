use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::bail;
use tracing::{debug, info, warn};

use crate::commands;
use crate::installable::Installable;
use crate::interface::NixBuildPassthroughArgs;

/// Resolves an Installable from an environment variable.
///
/// Returns `Some(Installable)` if the environment variable is set and can be parsed,
/// or `None` if the environment variable is not set.
pub fn resolve_env_installable(var: &str) -> Option<Installable> {
    env::var(var).ok().map(|val| {
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
    })
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

    use crate::util::get_hostname;

    match &mut installable {
        Installable::Flake {
            reference,
            attribute,
        } => {
            // If attribute path is already specified, use it as-is
            if !attribute.is_empty() {
                debug!(
                    "Using explicit attribute path from installable: {:?}",
                    attribute
                );
                return Ok(installable);
            }

            // Otherwise, build the attribute path
            attribute.push(config_type.to_string());
            let flake_reference = reference.clone();

            // Try to find the configuration by name if one was provided
            if let Some(config_name) = config_name {
                if find_config_in_flake(
                    &config_name,
                    attribute,
                    &flake_reference,
                    extra_args,
                    push_drv,
                    extra_path,
                )? {
                    return Ok(installable);
                }

                return Err(color_eyre::eyre::eyre!(
                    "Explicitly specified configuration not found in flake."
                ));
            }

            // Try to auto-detect the configuration
            let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
            let hostname = get_hostname().unwrap_or_else(|_| "host".to_string());

            for attr_name in [format!("{username}@{hostname}"), username] {
                if find_config_in_flake(
                    &attr_name,
                    attribute,
                    &flake_reference,
                    extra_args,
                    push_drv,
                    extra_path,
                )? {
                    return Ok(installable);
                }
            }

            return Err(color_eyre::eyre::eyre!(
                "Couldn't find configuration automatically in flake."
            ));
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

/// Find a configuration in a flake
///
/// Returns true if the configuration was found, false otherwise
fn find_config_in_flake(
    config_name: &str,
    attribute: &mut Vec<String>,
    flake_reference: &str,
    extra_args: &[OsString],
    push_drv: bool,
    extra_path: &[&str],
) -> Result<bool> {
    let func = format!(r#"x: x ? "{config_name}""#);
    let check_res = commands::Command::new("nix")
        .arg("eval")
        .args(extra_args)
        .arg("--apply")
        .arg(&func)
        .args(
            (Installable::Flake {
                reference: flake_reference.to_string(),
                attribute: attribute.clone(),
            })
            .to_args(),
        )
        .run_capture();

    if let Ok(res) = check_res {
        if res.map(|s| s.trim().to_owned()).as_deref() == Some("true") {
            debug!("Found configuration: {}", config_name);
            attribute.push(config_name.to_string());

            if push_drv {
                attribute.extend(extra_path.iter().map(|s| (*s).to_string()));
            }

            return Ok(true);
        }
    }

    Ok(false)
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
        let confirmation = Confirm::new("Apply the config?")
            .with_default(false)
            .prompt()?;

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
#[cfg_attr(feature = "hotpath", hotpath::measure)]
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
        .run()
        .with_context(|| {
            format!(
                "Failed to compare configurations with nvd: {} vs {}",
                current_profile,
                target_profile.display()
            )
        })?;

    Ok(())
}

/// Build a configuration using the nix build command
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn build_configuration(
    installable: Installable,
    out_path: &dyn crate::util::MaybeTempPath,
    extra_args: &[impl AsRef<std::ffi::OsStr>],
    builder: Option<String>,
    message: &str,
    no_nom: bool,
    passthrough_args: NixBuildPassthroughArgs,
) -> Result<()> {
    let passthrough = passthrough_args.parse_passthrough_args()?;

    commands::Build::new(installable)
        .extra_arg("--out-link")
        .extra_arg(out_path.get_path())
        .extra_args(extra_args)
        .passthrough(&self.passthrough)
        .builder(builder)
        .message(message)
        .nom(!no_nom)
        .run()
        .with_context(|| format!("Failed to build configuration: {}", message))?;

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

    // NOTE: Using stdlib Command directly is necessary for interactive REPL
    // Interactivity implodes otherwise.
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
///
/// This function handles the core workflow for building and managing system
/// configurations across different platforms (`NixOS`, Darwin, Home Manager).
/// It unifies what would otherwise be duplicated across platform-specific modules.
///
/// The function takes care of:
/// 1. Properly configuring the attribute path based on platform type
/// 2. Building the configuration
/// 3. Handling specialisations where applicable
/// 4. Comparing the new configuration with the current one
///
/// # Arguments
///
/// * `installable` - The Nix installable representing the configuration
/// * `config_type` - The configuration type (e.g., "nixosConfigurations", "darwinConfigurations")
/// * `extra_path` - Additional path elements for the attribute path
/// * `config_name` - Optional hostname or configuration name
/// * `out_path` - Output path for the build result
/// * `extra_args` - Additional arguments to pass to the build command
/// * `builder` - Optional remote builder to use
/// * `message` - Message to display during the build process
/// * `no_nom` - Whether to disable nix-output-monitor
/// * `specialisation_path` - Path to read specialisations from
/// * `no_specialisation` - Whether to ignore specialisations
/// * `specialisation` - Optional explicit specialisation to use
/// * `current_profile` - Path to the current system profile for comparison
/// * `skip_compare` - Whether to skip comparing the new and current configuration
///
/// # Returns
///
/// The path to the built configuration, which can be used for activation
#[cfg_attr(feature = "hotpath", hotpath::measure)]
#[allow(clippy::too_many_arguments)]
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
    passthrough_args: NixBuildPassthroughArgs,
) -> Result<PathBuf> {
    // Convert the extra_args to OsString for the config struct
    let extra_args_vec: Vec<OsString> = extra_args
        .iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect();

    // Create a config struct from the parameters
    let config = RebuildWorkflowConfig {
        installable,
        config_type,
        extra_path,
        config_name,
        out_path,
        extra_args: extra_args_vec,
        builder,
        message,
        no_nom,
        specialisation_path,
        no_specialisation,
        specialisation,
        current_profile,
        skip_compare,
        passthrough_args,
    };

    // Delegate to the new implementation
    handle_rebuild_workflow_with_config(config)
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
            None => bail!(
                "Unable to fetch hostname automatically. Please specify explicitly with --hostname."
            ),
        },
    };

    // Skip comparison when system hostname != target hostname if requested
    let hostname_mismatch = skip_if_mismatch
        && system_hostname.is_some()
        && system_hostname.unwrap() != target_hostname;

    debug!(
        ?target_hostname,
        ?hostname_mismatch,
        "Determined target hostname"
    );
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

/// Configuration options for rebuilding workflows
pub struct RebuildWorkflowConfig<'a> {
    /// The Nix installable representing the configuration
    pub installable: Installable,

    /// The configuration type (e.g., "nixosConfigurations", "darwinConfigurations")
    pub config_type: &'a str,

    /// Additional path elements for the attribute path
    pub extra_path: &'a [&'a str],

    /// Optional hostname or configuration name
    pub config_name: Option<String>,

    /// Output path for the build result
    pub out_path: &'a dyn crate::util::MaybeTempPath,

    /// Additional arguments to pass to the build command as OsStrings
    pub extra_args: Vec<OsString>,

    /// Optional remote builder to use
    pub builder: Option<String>,

    /// Message to display during the build process
    pub message: &'a str,

    /// Whether to disable nix-output-monitor
    pub no_nom: bool,

    /// Path to read specialisations from
    pub specialisation_path: &'a str,

    /// Whether to ignore specialisations
    pub no_specialisation: bool,

    /// Optional explicit specialisation to use
    pub specialisation: Option<String>,

    /// Path to the current system profile for comparison
    pub current_profile: &'a str,

    /// Whether to skip comparing the new and current configuration
    pub skip_compare: bool,

    /// Arguments to pass to Nix
    pub passthrough_args: NixBuildPassthroughArgs,
}

/// Execute common actions for a rebuild operation across platforms using configuration struct
///
/// This function takes a configuration struct instead of many individual parameters
fn handle_rebuild_workflow_with_config(config: RebuildWorkflowConfig) -> Result<PathBuf> {
    // Special handling for darwin configurations
    if config.config_type == "darwinConfigurations" {
        // First construct the proper attribute path for darwin configs
        let mut processed_installable = config.installable;
        if let Installable::Flake {
            ref mut attribute, ..
        } = processed_installable
        {
            // Only set the attribute path if user hasn't already specified one
            if attribute.is_empty() {
                attribute.push(String::from(config.config_type));
                if let Some(name) = &config.config_name {
                    attribute.push(name.clone());
                }
            }
        }

        // Next, add config.system.build.<attr> to the path to access the derivation
        let mut toplevel_attr = processed_installable;
        if let Installable::Flake {
            ref mut attribute, ..
        } = toplevel_attr
        {
            // All darwin configurations expose their outputs under system.build
            let toplevel_path = ["config", "system", "build"];
            attribute.extend(toplevel_path.iter().map(|s| (*s).to_string()));

            // Add the final component (usually "toplevel")
            if !config.extra_path.is_empty() {
                attribute.push(config.extra_path[0].to_string());
            }
        }

        // Build the configuration
        build_configuration(
            toplevel_attr,
            config.out_path,
            &config.extra_args,
            config.builder.clone(),
            config.message,
            config.no_nom,
            config.passthrough_args,
        )?;

        // Darwin doesn't use the specialisation mechanism like NixOS
        let target_profile = config.out_path.get_path().to_owned();

        // Run the diff to show changes
        if !config.skip_compare {
            compare_configurations(
                config.current_profile,
                &target_profile,
                false,
                "Comparing changes",
            )?;
        }

        return Ok(target_profile);
    }

    // Configure the installable with platform-specific attributes
    let configured_installable = extend_installable_for_platform(
        config.installable,
        config.config_type,
        config.extra_path,
        config.config_name.clone(),
        true,
        &config.extra_args,
    )?;

    // Build the configuration
    build_configuration(
        configured_installable,
        config.out_path,
        &config.extra_args,
        config.builder.clone(),
        config.message,
        config.no_nom,
        config.passthrough_args,
    )?;

    // Process any specialisations (NixOS/Home-Manager specific feature)
    let target_specialisation = process_specialisation(
        config.no_specialisation,
        config.specialisation.clone(),
        config.specialisation_path,
    )?;

    // Get target profile path
    let target_profile = get_target_profile(config.out_path, &target_specialisation);

    // Compare configurations if applicable
    if !config.skip_compare {
        compare_configurations(
            config.current_profile,
            &target_profile,
            false,
            "Comparing changes",
        )?;
    }

    Ok(target_profile)
}

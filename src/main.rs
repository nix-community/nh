mod clean;
mod commands;
mod completion;
mod darwin;
mod generations;
mod home;
mod installable;
mod interface;
mod json;
mod logging;
mod nixos;
mod plugins;
mod search;
mod update;
mod util;

use color_eyre::Result;
use tracing::debug;

const NH_VERSION: &str = env!("CARGO_PKG_VERSION");
const NH_REV: Option<&str> = option_env!("NH_REV");

fn main() -> Result<()> {
    let mut do_warn = false;
    if let Ok(f) = std::env::var("FLAKE") {
        // Set NH_FLAKE if it's not already set
        if std::env::var("NH_FLAKE").is_err() {
            std::env::set_var("NH_FLAKE", f);

            // Only warn if FLAKE is set and we're using it to set NH_FLAKE
            // AND none of the command-specific env vars are set
            if std::env::var("NH_OS_FLAKE").is_err()
                && std::env::var("NH_HOME_FLAKE").is_err()
                && std::env::var("NH_DARWIN_FLAKE").is_err()
            {
                do_warn = true;
            }
        }
    }

    let args = <crate::interface::Main as clap::Parser>::parse();
    crate::logging::setup_logging(args.verbose)?;
    tracing::debug!("{args:#?}");
    tracing::debug!(%NH_VERSION, ?NH_REV);

    if do_warn {
        tracing::warn!(
            "nh {NH_VERSION} now uses NH_FLAKE instead of FLAKE, please modify your configuration"
        );
    }

    // Initialize the plugin system
    let plugin_manager = match plugins::init() {
        Ok(pm) => Some(pm),
        Err(e) => {
            tracing::warn!("Failed to initialize plugin system: {}", e);
            None
        }
    };

    // Register the NH API if plugin system is initialized
    if let Some(pm) = &plugin_manager {
        if let Err(e) = plugins::register_api(pm.lua()) {
            tracing::warn!("Failed to register plugin API: {}", e);
        }
    }

    // Create a context for the command if plugin system is initialized
    if let Some(pm) = &plugin_manager {
        // Get command string
        let command_str = format!("{:?}", args.command);

        // Create data table for plugin context
        let table = match pm.lua().create_table() {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Failed to create Lua table: {}", e);
                return args.command.run();
            }
        };

        // Create plugin context
        let mut context = plugins::PluginContext {
            lua: pm.lua(),
            command: Some(command_str.clone()),
            args: std::env::args().skip(1).collect(),
            data: table,
        };

        // First try specific command hook
        let specific_event = plugins::PluginEvent::PreCommand(command_str.clone());
        match pm.trigger_hook(&specific_event, &mut context) {
            plugins::PluginResult::Error(err) => {
                tracing::error!("Plugin specific pre-command error: {}", err);
                return Err(color_eyre::eyre::eyre!("Plugin error: {}", err));
            }
            plugins::PluginResult::Skip => {
                tracing::info!("Command execution skipped by plugin (specific hook)");
                return Ok(());
            }
            _ => {}
        }

        // Then try generic command hook
        let generic_event = plugins::PluginEvent::PreCommand(String::new());
        match pm.trigger_hook(&generic_event, &mut context) {
            plugins::PluginResult::Error(err) => {
                tracing::error!("Plugin generic pre-command error: {}", err);
                return Err(color_eyre::eyre::eyre!("Plugin error: {}", err));
            }
            plugins::PluginResult::Skip => {
                tracing::info!("Command execution skipped by plugin (generic hook)");
                return Ok(());
            }
            _ => {}
        }

        // Run the command
        let result = args.command.run();

        // Trigger both specific and generic post-command hooks
        pm.trigger_hook(
            &plugins::PluginEvent::PostCommand(command_str),
            &mut context,
        );
        pm.trigger_hook(
            &plugins::PluginEvent::PostCommand(String::new()),
            &mut context,
        );

        // Trigger before_exit hooks
        pm.trigger_hook(&plugins::PluginEvent::BeforeExit, &mut context);

        result
    } else {
        // Run without plugins
        args.command.run()
    }
}

fn self_elevate() -> ! {
    use std::os::unix::process::CommandExt;

    let mut cmd = std::process::Command::new("sudo");
    cmd.args(std::env::args());
    debug!("{:?}", cmd);
    let err = cmd.exec();
    panic!("{}", err);
}

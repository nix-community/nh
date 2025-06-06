mod checks;
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
mod search;
mod update;
mod util;

use color_eyre::Result;
use tracing::debug;

const NH_VERSION: &str = env!("CARGO_PKG_VERSION");
const NH_REV: Option<&str> = option_env!("NH_REV");

fn main() -> Result<()> {
    let args = <crate::interface::Main as clap::Parser>::parse();

    // Set up logging
    crate::logging::setup_logging(args.verbose)?;
    tracing::debug!("{args:#?}");
    tracing::debug!(%NH_VERSION, ?NH_REV);

    // Check Nix version upfront
    checks::verify_nix_environment()?;

    // Once we assert required Nix features, validate NH environment checks
    // For now, this is just NH_* variables being set. More checks may be
    // added to setup_environment in the future.
    if checks::setup_environment()? {
        tracing::warn!(
            "nh {NH_VERSION} now uses NH_FLAKE instead of FLAKE, please modify your configuration"
        );
    }

    args.command.run()
}

/// Self-elevates the current process by re-executing it with sudo
fn self_elevate() -> ! {
    use std::os::unix::process::CommandExt;

    let mut cmd = std::process::Command::new("sudo");
    cmd.arg("--preserve-env");

    if cfg!(target_os = "macos") {
        cmd.args(["--set-home", "--preserve-env=PATH", "env"]);
    }

    // use NH_SUDO_ASKPASS program for sudo if present
    let askpass = std::env::var("NH_SUDO_ASKPASS");
    if let Ok(askpass) = askpass {
        cmd.env("SUDO_ASKPASS", askpass).arg("-A");
    }

    cmd.args(std::env::args());
    debug!("{:?}", cmd);
    let err = cmd.exec();
    panic!("{}", err);
}

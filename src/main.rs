mod check;
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
    let do_warn = check::setup_environment()?;

    let args = <crate::interface::Main as clap::Parser>::parse();
    crate::logging::setup_logging(args.verbose)?;
    tracing::debug!("{args:#?}");
    tracing::debug!(%NH_VERSION, ?NH_REV);

    if do_warn {
        tracing::warn!(
            "nh {NH_VERSION} now uses NH_FLAKE instead of FLAKE, please modify your configuration"
        );
    }

    // Verify the Nix environment before running commands
    check::verify_nix_environment()?;

    args.command.run()
}

/// Self-elevates the current process by re-executing it with sudo
fn self_elevate() -> ! {
    use std::os::unix::process::CommandExt;

    let mut cmd = std::process::Command::new("sudo");

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

use clap_complete::generate;
use color_eyre::Result;
use tracing::instrument;

use crate::{interface, interface::Main};

impl interface::CompletionArgs {
  #[instrument(ret, level = "trace")]
  /// Run the completion subcommand.
  ///
  /// # Errors
  ///
  /// Returns an error if completion script generation or output fails.
  #[cfg_attr(feature = "hotpath", hotpath::measure)]
  pub fn run(&self) -> Result<()> {
    let mut cmd = <Main as clap::CommandFactory>::command();
    match self.shell {
      interface::Shell::Bash => {
        generate(
          clap_complete::Shell::Bash,
          &mut cmd,
          "nh",
          &mut std::io::stdout(),
        )
      },
      interface::Shell::Elvish => {
        generate(
          clap_complete::Shell::Elvish,
          &mut cmd,
          "nh",
          &mut std::io::stdout(),
        )
      },
      interface::Shell::Fish => {
        generate(
          clap_complete::Shell::Fish,
          &mut cmd,
          "nh",
          &mut std::io::stdout(),
        )
      },
      interface::Shell::PowerShell => {
        generate(
          clap_complete::Shell::PowerShell,
          &mut cmd,
          "nh",
          &mut std::io::stdout(),
        )
      },
      interface::Shell::Zsh => {
        generate(
          clap_complete::Shell::Zsh,
          &mut cmd,
          "nh",
          &mut std::io::stdout(),
        )
      },
      interface::Shell::Nushell => {
        generate(
          clap_complete_nushell::Nushell,
          &mut cmd,
          "nh",
          &mut std::io::stdout(),
        )
      },
    }
    Ok(())
  }
}

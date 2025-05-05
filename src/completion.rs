use clap_complete::generate;
use color_eyre::Result;
use tracing::instrument;

use crate::{
    interface,
    interface::Main,
};

impl interface::CompletionArgs {
    #[instrument(ret, level = "trace")]
    pub fn run(&self) -> Result<()> {
        let mut cmd = <Main as clap::CommandFactory>::command();
        generate(self.shell, &mut cmd, "nh", &mut std::io::stdout());
        Ok(())
    }
}

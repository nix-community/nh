use std::{
  env,
  path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};

mod man;

#[derive(Parser)]
struct Cli {
  #[command(subcommand)]
  command: Command,
}

#[derive(Subcommand)]
enum Command {
  /// Generate manpage
  Man {
    /// Output directory for manpage
    #[arg(long, default_value = "gen")]
    out_dir: String,
  },
}

fn main() {
  let Cli { command } = Cli::parse();
  env::set_current_dir(project_root()).unwrap();
  match command {
    Command::Man { out_dir } => man::r#gen(&out_dir),
  }
}

fn project_root() -> PathBuf {
  Path::new(
    &env::var("CARGO_MANIFEST_DIR")
      .unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_owned()),
  )
  .ancestors()
  .nth(1)
  .unwrap()
  .to_path_buf()
}

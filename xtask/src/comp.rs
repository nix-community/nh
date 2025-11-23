use std::path::{Path, PathBuf};

use clap::{CommandFactory, ValueEnum};
use clap_complete::generate_to;

const BINARY_NAME: &str = "nh";

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum CompletionShell {
  Bash,
  Elvish,
  Fish,
  PowerShell,
  Zsh,
  Nushell,
}

impl std::fmt::Display for CompletionShell {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Bash => write!(f, "Bash"),
      Self::Elvish => write!(f, "Elvish"),
      Self::Fish => write!(f, "Fish"),
      Self::PowerShell => write!(f, "PowerShell"),
      Self::Zsh => write!(f, "Zsh"),
      Self::Nushell => write!(f, "Nushell"),
    }
  }
}

const ALL_SHELLS: [CompletionShell; 6] = [
  CompletionShell::Bash,
  CompletionShell::Elvish,
  CompletionShell::Fish,
  CompletionShell::PowerShell,
  CompletionShell::Zsh,
  CompletionShell::Nushell,
];

pub fn generate(
  out_dir: &str,
  shell: Option<CompletionShell>,
) -> Result<(), String> {
  let gen_dir = Path::new(out_dir);
  if !gen_dir.exists() {
    std::fs::create_dir_all(gen_dir).map_err(|e| {
      format!("Failed to create output directory '{}': {}", out_dir, e)
    })?;
  }

  let mut cmd = nh::interface::Main::command();

  match shell {
    Some(shell) => {
      generate_single(shell, &mut cmd, gen_dir)?;
      println!("Generated {} completion to {}", shell, out_dir);
    },
    None => {
      for shell in ALL_SHELLS {
        generate_single(shell, &mut cmd, gen_dir)?;
      }
      println!("Generated all completions to {}", out_dir);
    },
  }

  Ok(())
}

fn generate_single(
  shell: CompletionShell,
  cmd: &mut clap::Command,
  out_dir: &Path,
) -> Result<PathBuf, String> {
  match shell {
    CompletionShell::Bash => {
      generate_to(clap_complete::Shell::Bash, cmd, BINARY_NAME, out_dir)
        .map_err(|e| format!("Failed to generate Bash completion: {}", e))
    },
    CompletionShell::Elvish => {
      generate_to(clap_complete::Shell::Elvish, cmd, BINARY_NAME, out_dir)
        .map_err(|e| format!("Failed to generate Elvish completion: {}", e))
    },
    CompletionShell::Fish => {
      generate_to(clap_complete::Shell::Fish, cmd, BINARY_NAME, out_dir)
        .map_err(|e| format!("Failed to generate Fish completion: {}", e))
    },
    CompletionShell::PowerShell => {
      generate_to(clap_complete::Shell::PowerShell, cmd, BINARY_NAME, out_dir)
        .map_err(|e| format!("Failed to generate PowerShell completion: {}", e))
    },
    CompletionShell::Zsh => {
      generate_to(clap_complete::Shell::Zsh, cmd, BINARY_NAME, out_dir)
        .map_err(|e| format!("Failed to generate Zsh completion: {}", e))
    },
    CompletionShell::Nushell => {
      generate_to(clap_complete_nushell::Nushell, cmd, BINARY_NAME, out_dir)
        .map_err(|e| format!("Failed to generate Nushell completion: {}", e))
    },
  }
}

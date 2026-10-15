use std::path::{Path, PathBuf};

use pound::ValueEnum;

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
      format!("Failed to create output directory '{out_dir}': {e}")
    })?;
  }

  if let Some(shell) = shell {
    generate_single(shell, gen_dir)?;
    println!("Generated {shell} completion to {out_dir}");
  } else {
    for shell in ALL_SHELLS {
      generate_single(shell, gen_dir)?;
    }
    println!("Generated all completions to {out_dir}");
  }

  Ok(())
}

fn generate_single(
  shell: CompletionShell,
  out_dir: &Path,
) -> Result<PathBuf, String> {
  let (generator, file_name) = match shell {
    CompletionShell::Bash => (caress::Shell::Bash, "nh.bash"),
    CompletionShell::Elvish => (caress::Shell::Elvish, "nh.elv"),
    CompletionShell::Fish => (caress::Shell::Fish, "nh.fish"),
    CompletionShell::PowerShell => (caress::Shell::PowerShell, "_nh.ps1"),
    CompletionShell::Zsh => (caress::Shell::Zsh, "nh.zsh"),
    CompletionShell::Nushell => (caress::Shell::Nu, "nh.nu"),
  };
  let path = out_dir.join(file_name);
  std::fs::write(&path, generator.script(BINARY_NAME))
    .map_err(|e| format!("Failed to generate {shell} completion: {e}"))?;
  Ok(path)
}

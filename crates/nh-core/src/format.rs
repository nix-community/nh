use std::io::{BufRead, BufReader, Write};

use subprocess::Exec;
use yansi::Paint;

use crate::ui::{
  BLUE,
  GREEN,
  GREY,
  ICON_ARROW,
  ICON_BULLET,
  ICON_INFO,
  ICON_SUCCESS,
  ICON_WARNING,
  PURPLE,
  RED,
  YELLOW,
};

pub enum LogLine {
  HomebrewUsing(String),
  HomebrewInstalling(String),
  HomebrewUpgrading(String),
  HomebrewComplete(usize),
  HomeManagerActivating(String),
  DarwinInfo(String),
  DarwinSuccess(String),
  SectionHeader(String),
  FlakeInputUpdating(String),
  Warning(String),
  Other(String),
}

#[must_use]
pub fn process_line(line: &str) -> LogLine {
  let line = line.trim();
  if let Some(dep) = line.strip_prefix("Using ") {
    return LogLine::HomebrewUsing(dep.to_string());
  }
  if let Some(dep) = line.strip_prefix("Installing ") {
    return LogLine::HomebrewInstalling(dep.to_string());
  }
  if let Some(dep) = line.strip_prefix("Upgrading ") {
    return LogLine::HomebrewUpgrading(dep.to_string());
  }
  if let Some(caps) = line.strip_prefix("`brew bundle` complete! ")
    && let Some(count_str) = caps.split_whitespace().next()
    && let Ok(count) = count_str.parse::<usize>()
  {
    return LogLine::HomebrewComplete(count);
  }
  if let Some(caps) = line.strip_prefix("Homebrew Bundle complete! ")
    && let Some(count_str) = caps.split_whitespace().next()
    && let Ok(count) = count_str.parse::<usize>()
  {
    return LogLine::HomebrewComplete(count);
  }
  if let Some(input) = line.strip_prefix("updating input '") {
    let input = input.trim_end_matches('\'');
    return LogLine::FlakeInputUpdating(input.to_string());
  }
  if let Some(module) = line.strip_prefix("Activating ") {
    return LogLine::HomeManagerActivating(module.to_string());
  }
  if let Some(msg) = line.strip_prefix("✓ ").or_else(|| line.strip_prefix("✔ "))
  {
    if msg.contains("Error:") || msg.contains("failed") {
      return LogLine::Warning(msg.to_string());
    }
    return LogLine::DarwinSuccess(msg.to_string());
  }
  if let Some(section) = line.strip_prefix("➜ ") {
    return LogLine::SectionHeader(section.to_string());
  }
  if let Some(info) = line.strip_prefix("ℹ️ ") {
    return LogLine::DarwinInfo(info.to_string());
  }
  if let Some(warn) = line.strip_prefix("Warning: ") {
    return LogLine::Warning(warn.to_string());
  }
  if line.contains("━━━") || line.contains("━━━━") {
    let content = line.trim_matches('━').trim();
    if !content.is_empty() {
      return LogLine::DarwinInfo(content.to_string());
    }
  }

  LogLine::Other(line.to_string())
}

#[derive(Default)]
pub struct ActivationState {
  brew_using_count: usize,
  brew_installed:   Vec<String>,
  brew_upgraded:    Vec<String>,
  brew_missing:     usize,
  hm_count:         usize,
  darwin_success:   usize,
  flake_updates:    usize,
  last_info:        Option<String>,
}

#[allow(clippy::missing_errors_doc)]
pub fn run_pretty(exec: Exec) -> color_eyre::Result<()> {
  let mut popen = exec.start()?;
  let stdout = popen
    .stdout
    .take()
    .ok_or_else(|| color_eyre::eyre::eyre!("Failed to capture stdout"))?;
  let reader = BufReader::new(stdout);
  let mut state = ActivationState::default();

  for line_result in reader.lines() {
    let line = line_result?;
    if line.trim().is_empty() {
      continue;
    }

    match process_line(&line) {
      LogLine::HomebrewUsing(_) => {
        // Already-installed deps are the common case and noisy. Just count
        // them; they'll appear in the summary as "working fine: N".
        state.brew_using_count += 1;
      },
      LogLine::HomebrewInstalling(name) => {
        println!(
          "  {} installing {}",
          Paint::new(ICON_SUCCESS).fg(GREEN).bold(),
          Paint::new(&name).bold()
        );
        state.brew_installed.push(name);
      },
      LogLine::HomebrewUpgrading(name) => {
        println!(
          "  {} upgrading {}",
          Paint::new(ICON_ARROW).fg(BLUE).bold(),
          Paint::new(&name).bold()
        );
        state.brew_upgraded.push(name);
      },
      LogLine::HomebrewComplete(count) => {
        // Per-package install/upgrade lines were already printed above; here
        // just confirm the bundle finished and show the headline numbers.
        let changed = state.brew_installed.len() + state.brew_upgraded.len();
        if count == 0 {
          // Do nothing
        } else if changed == 0 {
          println!(
            "  {} Homebrew: {} dependencies, all up to date",
            Paint::new(ICON_SUCCESS).fg(GREEN),
            count
          );
        } else {
          println!(
            "  {} Homebrew: {} dependencies — {} installed, {} upgraded",
            Paint::new(ICON_SUCCESS).fg(GREEN),
            count,
            Paint::new(state.brew_installed.len()).fg(GREEN).bold(),
            Paint::new(state.brew_upgraded.len()).fg(BLUE).bold()
          );
        }
      },
      LogLine::HomeManagerActivating(_) => {
        state.hm_count += 1;
        print!(
          "\r\x1b[2K  {} Home-Manager: {} modules activated",
          Paint::new(ICON_INFO).fg(BLUE),
          state.hm_count
        );
        let _ = std::io::stdout().flush();
      },
      LogLine::FlakeInputUpdating(_) => {
        state.flake_updates += 1;
        print!(
          "\r\x1b[2K  {} Flake: {} inputs updated",
          Paint::new(ICON_INFO).fg(BLUE),
          state.flake_updates
        );
        let _ = std::io::stdout().flush();
      },
      LogLine::DarwinInfo(info) => {
        if state.last_info.as_ref() != Some(&info) {
          println!("\n{} {}", Paint::new(ICON_ARROW).fg(PURPLE).bold(), info);
          state.last_info = Some(info);
        }
      },
      LogLine::DarwinSuccess(msg) => {
        state.darwin_success += 1;
        println!("\r\x1b[2K  {} {}", Paint::new(ICON_SUCCESS).fg(GREEN), msg);
      },
      LogLine::SectionHeader(section) => {
        println!(
          "\n{} {}",
          Paint::new(ICON_ARROW).fg(PURPLE).bold(),
          section.trim()
        );
      },
      LogLine::Warning(warn) => {
        if warn.contains("not installed") {
          state.brew_missing += 1;
        }
        println!(
          "\r\x1b[2K  {} Warning: {}",
          Paint::new(ICON_WARNING).fg(YELLOW),
          warn
        );
      },
      LogLine::Other(other) => {
        if other.contains("Starting Home Manager activation") {
          println!("\r\x1b[2K");
        } else if other.contains("Error:") || other.contains("failed") {
          println!(
            "\r\x1b[2K  {} {}",
            Paint::new(ICON_WARNING).fg(RED),
            Paint::new(other).fg(RED)
          );
        } else if !other.is_empty() {
          println!("\r\x1b[2K  {}", Paint::new(other).dim());
        }
      },
    }
  }

  let status = popen.wait()?;
  if !status.success() {
    return Err(color_eyre::eyre::eyre!(
      "Activation failed with status {:?}",
      status
    ));
  }

  // Print a final summary at the end
  println!(
    "\n{} Activation Summary",
    Paint::new(ICON_SUCCESS).fg(GREEN).bold()
  );
  if !state.brew_installed.is_empty()
    || !state.brew_upgraded.is_empty()
    || state.brew_using_count > 0
    || state.brew_missing > 0
  {
    println!("  {} Homebrew", Paint::new(ICON_ARROW).fg(PURPLE));
    if !state.brew_installed.is_empty() {
      println!(
        "    {} installed ({}): {}",
        Paint::new(ICON_SUCCESS).fg(GREEN),
        Paint::new(state.brew_installed.len()).bold(),
        Paint::new(state.brew_installed.join(", ")).bold()
      );
    }
    if !state.brew_upgraded.is_empty() {
      println!(
        "    {} upgraded  ({}): {}",
        Paint::new(ICON_ARROW).fg(BLUE),
        Paint::new(state.brew_upgraded.len()).bold(),
        Paint::new(state.brew_upgraded.join(", ")).bold()
      );
    }
    if state.brew_using_count > 0 {
      println!(
        "    {} working fine: {}",
        Paint::new(ICON_BULLET).fg(GREY),
        Paint::new(state.brew_using_count).dim()
      );
    }
    if state.brew_missing > 0 {
      println!(
        "    {} missing: {}",
        Paint::new(ICON_WARNING).fg(YELLOW),
        Paint::new(state.brew_missing).fg(YELLOW).bold()
      );
    }
  }
  if state.hm_count > 0 {
    println!(
      "  {} Home Manager: {} modules activated",
      Paint::new(ICON_ARROW).fg(PURPLE),
      state.hm_count
    );
  }
  if state.darwin_success > 0 {
    println!(
      "  {} Darwin: {} system settings applied",
      Paint::new(ICON_ARROW).fg(PURPLE),
      state.darwin_success
    );
  }
  if state.flake_updates > 0 {
    println!(
      "  {} Flake: {} inputs updated",
      Paint::new(ICON_ARROW).fg(PURPLE),
      state.flake_updates
    );
  }

  if state.brew_installed.is_empty()
    && state.brew_upgraded.is_empty()
    && state.brew_missing == 0
    && state.hm_count == 0
    && state.darwin_success == 0
    && state.flake_updates == 0
  {
    println!(
      "  {} Configuration is already up to date",
      Paint::new(ICON_SUCCESS).fg(GREEN)
    );
  }

  Ok(())
}

//! Simple synchronous terminal prompts using crossterm.
use std::{
  io::{self, Write, stdout},
  sync::OnceLock,
};

use color_eyre::Result;
use crossterm::{
  event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
  execute,
  style::{ResetColor, SetForegroundColor},
  terminal::{
    DisableLineWrap,
    EnableLineWrap,
    disable_raw_mode,
    enable_raw_mode,
  },
};

static HYPERLINKS_SUPPORTED: OnceLock<bool> = OnceLock::new();

/// Prints a clickable hyperlink in terminals that support it,
/// otherwise prints plain text.
pub fn print_hyperlink(text: &str, link: &str) {
  let supported =
    *HYPERLINKS_SUPPORTED.get_or_init(supports_hyperlinks::supports_hyperlinks);

  if supported {
    print!("\x1b]8;;{link}\x07");
    println!("{text}\x1b]8;;\x07");
  } else {
    println!("{text}");
  }
}

/// RAII guard for terminal raw mode.
///
/// Enables raw mode on creation and guarantees it is disabled when dropped.
struct RawModeGuard;

impl RawModeGuard {
  fn new() -> io::Result<Self> {
    enable_raw_mode()?;
    Ok(Self)
  }
}

impl Drop for RawModeGuard {
  fn drop(&mut self) {
    disable_raw_mode().ok();
  }
}

/// Prompts the user for a password with hidden input.
///
/// # Errors
///
/// Returns an error if reading from stdin fails.
pub fn prompt_password(prompt: &str) -> Result<String> {
  let mut stdout = stdout();

  execute!(stdout, DisableLineWrap)?;
  execute!(stdout, crossterm::style::Print(prompt))?;
  print!(": ");
  stdout.flush()?;

  let guard = RawModeGuard::new()?;
  let mut password = String::new();

  loop {
    if let Event::Key(KeyEvent {
      code, modifiers, ..
    }) = event::read()?
    {
      match code {
        KeyCode::Enter => {
          println!();
          break;
        },
        KeyCode::Backspace => {
          if !password.is_empty() {
            password.pop();
            print!("\x08 \x08");
            stdout.flush()?;
          }
        },
        KeyCode::Char(c) => {
          if modifiers == KeyModifiers::CONTROL && c == 'c' {
            return Ok(String::new());
          }
          password.push(c);
          print!("*");
          stdout.flush()?;
        },
        _ => {},
      }
    }
  }

  drop(guard);
  execute!(stdout, EnableLineWrap)?;
  execute!(stdout, ResetColor)?;

  Ok(password)
}

/// Prompts the user for a yes/no confirmation. Defaults to `false` when user
/// presses Enter without input.
///
/// # Errors
///
/// Returns an error if stdin is not a TTY or reading fails.
pub fn prompt_confirm(prompt: &str) -> Result<bool> {
  let mut stdout = stdout();

  execute!(stdout, SetForegroundColor(crossterm::style::Color::Green))?;
  execute!(stdout, crossterm::style::Print("? "))?;
  execute!(stdout, SetForegroundColor(crossterm::style::Color::Reset))?;
  execute!(stdout, crossterm::style::Print(prompt))?;
  print!(" (y/n): ");
  stdout.flush()?;

  let _guard = RawModeGuard::new()?;

  loop {
    if let Event::Key(KeyEvent { code, .. }) = event::read()? {
      match code {
        KeyCode::Char('y' | 'Y') => {
          println!("yes");
          return Ok(true);
        },
        KeyCode::Char('n' | 'N') => {
          println!("no");
          return Ok(false);
        },
        KeyCode::Enter => {
          println!();
          return Ok(false);
        },
        _ => {},
      }
    }
  }
}

//! Simple synchronous terminal prompts using crossterm.

use std::{
  io::{self, Read, Write, stdout},
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
    SetTitle,
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

fn read_char() -> io::Result<char> {
  loop {
    let mut buf = [0u8; 1];
    if io::stdin().read(&mut buf)? == 1 {
      return Ok(buf[0] as char);
    }
  }
}

/// Prompts the user for a password with hidden input.
///
/// # Errors
///
/// Returns an error if reading from stdin fails.
pub fn prompt_password(prompt: &str) -> Result<String> {
  let mut stdout = stdout();

  execute!(stdout, SetTitle("Password Input"))?;
  execute!(stdout, DisableLineWrap)?;
  execute!(stdout, crossterm::style::Print(prompt))?;
  print!(": ");
  stdout.flush()?;

  let _guard = RawModeGuard::new()?;
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

  drop(_guard);
  execute!(stdout, EnableLineWrap)?;
  execute!(stdout, ResetColor)?;

  Ok(password)
}

/// Prompts the user for a yes/no confirmation.
///
/// # Errors
///
/// Returns an error if reading from stdin fails.
pub fn prompt_confirm(prompt: &str) -> Result<bool> {
  let mut stdout = stdout();

  execute!(stdout, SetTitle("Confirmation"))?;

  execute!(stdout, SetForegroundColor(crossterm::style::Color::Green))?;
  execute!(stdout, crossterm::style::Print("? "))?;
  execute!(stdout, SetForegroundColor(crossterm::style::Color::Reset))?;
  execute!(stdout, crossterm::style::Print(prompt))?;
  print!(" (y/n): ");
  stdout.flush()?;

  loop {
    match read_char()? {
      'y' | 'Y' => {
        println!("yes");
        return Ok(true);
      },
      'n' | 'N' => {
        println!("no");
        return Ok(false);
      },
      '\n' | '\r' => {
        println!();
        return Ok(false);
      },
      _ => {},
    }
  }
}

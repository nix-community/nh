//! Simple synchronous terminal prompts using crossterm.
use std::io::{self, Read, Write, stdout};

use color_eyre::Result;
use crossterm::{
  execute,
  style::{ResetColor, SetForegroundColor},
  terminal::{DisableLineWrap, EnableLineWrap, SetTitle},
};

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

  let mut password = String::new();
  loop {
    match read_char()? {
      '\n' | '\r' => {
        println!();
        break;
      },
      '\u{7f}' | '\u{8}' => {
        if !password.is_empty() {
          password.pop();
          print!("\x08 \x08");
          stdout.flush()?;
        }
      },
      c => {
        password.push(c);
        print!("*");
        stdout.flush()?;
      },
    }
  }

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

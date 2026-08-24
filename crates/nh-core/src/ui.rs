use yansi::{Color, Paint};

pub const PURPLE: Color = Color::Magenta;
pub const BLUE: Color = Color::Blue;
pub const GREEN: Color = Color::Green;
pub const YELLOW: Color = Color::Yellow;
pub const RED: Color = Color::Red;
pub const CYAN: Color = Color::Cyan;
pub const GREY: Color = Color::Fixed(244);

pub const ICON_ARROW: &str = "➜";
pub const ICON_SUCCESS: &str = "✔";
pub const ICON_DRY_RUN: &str = "→";
pub const ICON_INFO: &str = "ℹ";
pub const ICON_WARNING: &str = "◎";
pub const ICON_SKIP: &str = "·";
pub const ICON_BULLET: &str = "•";
pub const ICON_DIVIDER: &str = "─";

/// Render a byte count as a colored, human-readable size:
/// green for KB, yellow for MB, magenta-bold for GB.
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn colored_size(bytes: u64) -> String {
  let kb = bytes as f64 / 1024.0;
  let mb = kb / 1024.0;
  let gb = mb / 1024.0;

  if gb >= 1.0 {
    Paint::new(format!("{gb:.2} GB"))
      .fg(PURPLE)
      .bold()
      .to_string()
  } else if mb >= 1.0 {
    Paint::new(format!("{mb:.2} MB")).fg(YELLOW).to_string()
  } else if kb >= 1.0 {
    Paint::new(format!("{kb:.2} KB")).fg(GREEN).to_string()
  } else {
    Paint::new(format!("{bytes} B")).fg(GREEN).dim().to_string()
  }
}

/// Render a horizontal divider line of the given width.
#[must_use]
pub fn divider(width: usize) -> String {
  Paint::new(ICON_DIVIDER.repeat(width)).fg(GREY).to_string()
}

/// Available (free) disk space in bytes for the filesystem containing `path`.
/// Returns None if statvfs is unsupported or fails.
#[cfg(unix)]
#[must_use]
pub fn available_space_bytes(path: &str) -> Option<u64> {
  use std::path::Path;
  nix::sys::statvfs::statvfs(Path::new(path))
    .ok()
    .map(|s| u64::from(s.blocks_available()) * s.fragment_size())
}

#[cfg(not(unix))]
#[must_use]
pub fn available_space_bytes(_path: &str) -> Option<u64> {
  None
}

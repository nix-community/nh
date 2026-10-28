use std::time::Duration;

use indicatif::{ProgressBar, ProgressFinish, ProgressStyle};

pub type Spinner = ProgressBar;

const DEFAULT_SPINNER_TICK: Duration = Duration::from_millis(80);
const SPINNER_FRAMES: &[&str] = &["⢹", "⢺", "⢼", "⣸", "⣇", "⡧", "⡗", "⡏"];

/// Returns a spinner with a blue animation and message.
///
/// The spinner is cleared when it is dropped, so early returns don't leave it
/// behind.
///
/// # Panics
///
/// Panics if the hardcoded spinner template is invalid.
#[must_use]
pub fn spinner(message: impl Into<String>) -> Spinner {
  #[expect(clippy::expect_used)]
  let style = ProgressStyle::with_template("{spinner:.blue} {msg}")
    .expect("Static spinner template is valid")
    .tick_strings(SPINNER_FRAMES);

  // Haha spinner go brr
  let spinner = ProgressBar::new_spinner()
    .with_style(style)
    .with_finish(ProgressFinish::AndClear);
  spinner.set_message(message.into());
  spinner.enable_steady_tick(DEFAULT_SPINNER_TICK);

  spinner
}

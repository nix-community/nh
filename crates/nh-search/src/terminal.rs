use std::time::Duration;

use indicatif::ProgressBar;

pub struct SearchProgress {
  spinner: Option<ProgressBar>,
}

impl SearchProgress {
  pub fn start(json: bool, message: String) -> Self {
    if json {
      return Self { spinner: None };
    }

    let spinner = ProgressBar::new_spinner();
    spinner.enable_steady_tick(Duration::from_millis(100));
    spinner.set_message(message);
    Self {
      spinner: Some(spinner),
    }
  }

  pub fn set_message(&self, message: &'static str) {
    if let Some(spinner) = &self.spinner {
      spinner.set_message(message);
    }
  }

  pub fn finish(self) {
    if let Some(spinner) = self.spinner {
      spinner.finish_and_clear();
    }
  }
}

use nh_core::progress::{self, Spinner};

pub struct SearchProgress {
  spinner: Option<Spinner>,
}

impl SearchProgress {
  pub fn start(json: bool, message: String) -> Self {
    if json {
      return Self { spinner: None };
    }

    Self {
      spinner: Some(progress::spinner(message)),
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

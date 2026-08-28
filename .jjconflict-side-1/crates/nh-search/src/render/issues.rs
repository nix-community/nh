use yansi::{Color, Paint};

use super::common;
use crate::github::{Issue, IssueState};

pub fn print(issues: &[Issue]) {
  for issue in issues {
    println!(
      "{} ({}) {}",
      Paint::new(&issue.title).fg(Color::Blue),
      colored_status(issue.state),
      common::hyperlink(&format!("#{}", issue.number), &issue.url),
    );
  }
}

fn colored_status(state: IssueState) -> String {
  match state {
    IssueState::Open => format!("{}", Paint::new("open").fg(Color::Blue)),
    IssueState::Closed => format!("{}", Paint::new("closed").fg(Color::Red)),
  }
}

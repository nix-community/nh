//! Static values shared across nh's crates: environment variable names,
//! well-known filesystem paths and version information.
//!
//! Everything a user can observe is declared here once, with its
//! documentation, so the man page is generated from the same source the code
//! reads.

mod env_vars;
pub mod paths;

pub use env_vars::EnvVars;

/// The version of nh.
pub const NH_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The revision nh was built from, if the build provided one.
pub const NH_REV: Option<&str> = option_env!("NH_REV");

/// A documented value, rendered into nh's man page.
#[derive(Debug, Clone, Copy)]
pub struct Documented {
  /// The value itself, e.g. an environment variable name or a path.
  pub name:  &'static str,
  /// Documentation, one source line per line. Empty lines separate
  /// paragraphs.
  pub doc:   &'static str,
  /// The release that introduced it, if tracked.
  pub since: Option<Since>,
}

/// The release that introduced a documented value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Since {
  /// A released version, e.g. `4.5.0`.
  Version(&'static str),
  /// Not released yet. Replaced with the new version when releasing.
  NextRelease,
}

impl Since {
  /// A sentence stating when the value was added, e.g. `Added in nh 4.5.0.`
  #[must_use]
  pub fn sentence(self) -> String {
    match self {
      Self::Version(version) => format!("Added in nh {version}."),
      Self::NextRelease => "Added in the next release of nh.".to_owned(),
    }
  }
}

impl Documented {
  /// Iterate over the paragraphs of the documentation, with each paragraph's
  /// lines joined by single spaces.
  pub fn paragraphs(&self) -> impl Iterator<Item = String> {
    self
      .doc
      .split("\n\n")
      .map(|paragraph| {
        paragraph
          .lines()
          .map(str::trim)
          .filter(|line| !line.is_empty())
          .collect::<Vec<_>>()
          .join(" ")
      })
      .filter(|paragraph| !paragraph.is_empty())
  }
}

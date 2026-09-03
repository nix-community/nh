//! Parsing and rendering of Nix installables.
//!
//! A Nix *installable* is anything `nix build`, `nix eval`, and related
//! commands can act on. This crate models the four kinds Nix accepts as
//! [`Installable`] and provides the two conversions needed to move between the
//! command line and that model:
//!
//! - [`parse_flake_reference`] and [`parse_attribute`] turn user input into the
//!   structured form.
//! - [`Installable::to_args`] turns the structured form back into the arguments
//!   Nix expects.
//!
//! [new Nix CLI]: https://nix.dev/manual/nix/2.35/command-ref/new-cli/nix
//!
//! The grammar follows the [new Nix CLI]. The crate holds no policy of its own:
//! no environment lookups, defaulting, or CLI framework. Those belong to
//! callers such as the `nh-installable` wrapper.
//!
//! # Examples
//!
//! ```
//! use nix_installable::Installable;
//!
//! let (reference, attribute) =
//!   nix_installable::parse_flake_reference("github:NixOS/nixpkgs#hello")
//!     .expect("valid flake reference");
//! let installable = Installable::Flake {
//!   reference,
//!   attribute,
//! };
//! assert_eq!(installable.to_args().unwrap(), [
//!   "github:NixOS/nixpkgs#hello"
//! ]);
//! ```

use std::path::PathBuf;

/// A target that Nix can build, evaluate, or enter.
///
/// The variants correspond to the installable forms Nix accepts on the command
/// line. `Flake`, `File`, and `Expression` each carry an attribute path (empty
/// when none was given); `Store` is a concrete `/nix/store` path with no
/// attribute path.
#[derive(Debug, Clone)]
pub enum Installable {
  /// A flake reference (`FLAKEREF`) with an attribute path into its outputs.
  Flake {
    /// The flake reference, e.g. `.`, `github:owner/repo`, or `path:/repo`.
    reference: String,
    /// The attribute path within the flake's outputs.
    attribute: Vec<String>,
  },
  /// A Nix file (`--file`) with an attribute path into the value it evaluates
  /// to.
  File {
    /// Path to the `.nix` file.
    path:      PathBuf,
    /// The attribute path within the evaluated file.
    attribute: Vec<String>,
  },
  /// A realised `/nix/store` path.
  Store {
    /// The store path.
    path: PathBuf,
  },
  /// A raw Nix expression (`--expr`) with an attribute path into its result.
  Expression {
    /// The Nix expression source.
    expression: String,
    /// The attribute path within the evaluated expression.
    attribute:  Vec<String>,
  },
}

/// Error returned when an attribute path or flake reference cannot be parsed.
///
/// The message is a fragment intended to be appended to the offending input by
/// the caller, e.g. `format!("attribute path {err}")`.
pub type ParseError = &'static str;

/// Splits an attribute path into its components.
///
/// Components are separated by unquoted `.`. A component may be wrapped in
/// double quotes to contain literal `.` characters. Following Nix, there is no
/// escape mechanism: every other character, including `\`, is literal, and a
/// `"` always toggles quoting. An empty input yields an empty path.
///
/// # Errors
///
/// Returns an error when a quoted segment is left unclosed.
///
/// # Examples
///
/// ```
/// use nix_installable::parse_attribute;
///
/// assert_eq!(parse_attribute("a.b.c").unwrap(), ["a", "b", "c"]);
/// assert_eq!(parse_attribute(r#"pkgs."foo.bar""#).unwrap(), [
///   "pkgs", "foo.bar"
/// ]);
/// assert_eq!(parse_attribute(r"a\b").unwrap(), [r"a\b"]);
/// assert!(parse_attribute(r#"pkgs."unterminated"#).is_err());
/// ```
pub fn parse_attribute(input: &str) -> Result<Vec<String>, ParseError> {
  let mut components = Vec::new();

  if input.is_empty() {
    return Ok(components);
  }

  let mut in_quote = false;
  let mut current = String::new();

  for ch in input.chars() {
    match ch {
      '.' if !in_quote => components.push(std::mem::take(&mut current)),
      '"' => in_quote = !in_quote,
      _ => current.push(ch),
    }
  }

  if in_quote {
    return Err("contains an unclosed quoted attribute segment");
  }

  components.push(current);
  Ok(components)
}

/// Splits a `FLAKEREF[#ATTRPATH]` string into its reference and attribute path.
///
/// The reference is everything before the first `#`; the remainder is parsed as
/// an attribute path with [`parse_attribute`]. When there is no `#`, the whole
/// input is the reference and the attribute path is empty.
///
/// # Errors
///
/// Returns an error when the input is empty, when the reference before `#` is
/// empty (so Nix would otherwise search the current directory), or when the
/// attribute path is malformed.
///
/// # Examples
///
/// ```
/// use nix_installable::parse_flake_reference;
///
/// assert_eq!(
///   parse_flake_reference("nixpkgs#hello").unwrap(),
///   ("nixpkgs".to_string(), vec!["hello".to_string()]),
/// );
/// assert_eq!(
///   parse_flake_reference(".").unwrap(),
///   (".".to_string(), Vec::new()),
/// );
/// assert!(parse_flake_reference("#hello").is_err());
/// ```
pub fn parse_flake_reference(
  input: &str,
) -> Result<(String, Vec<String>), ParseError> {
  // Reject an empty reference so Nix never turns `""` or `#attr` into an
  // implicit search of the current directory.
  if input.is_empty() {
    return Err("is empty. Set it to a flake reference or remove it.");
  }

  let (reference, attribute) = input.split_once('#').unwrap_or((input, ""));

  if reference.is_empty() {
    return Err("missing reference part before `#`");
  }

  Ok((reference.to_owned(), parse_attribute(attribute)?))
}

/// Error returned when an [`Installable`] cannot be rendered to command-line
/// arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
  /// A file or store path was not valid UTF-8.
  NonUtf8Path,

  /// An attribute component contains a `"`. Nix attribute paths have no escape
  /// mechanism, so such a component cannot be expressed on the command line.
  UnrepresentableAttribute(String),
}

impl std::fmt::Display for RenderError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::NonUtf8Path => f.write_str("path is not valid UTF-8"),
      Self::UnrepresentableAttribute(component) => {
        write!(
          f,
          "attribute component {component:?} contains a `\"`, which Nix \
           attribute paths cannot represent"
        )
      },
    }
  }
}

impl std::error::Error for RenderError {}

/// Renders an attribute path back into a single Nix attribute-path string.
///
/// Wraps a component in double quotes when it is empty or contains `.`. All
/// other characters, including `\`, are written literally, because Nix has no
/// escape mechanism. The result round-trips through [`parse_attribute`].
///
/// # Errors
///
/// Returns [`RenderError::UnrepresentableAttribute`] when a component contains
/// a `"`, which has no representation in a Nix attribute path.
fn render_attribute<I>(attribute: I) -> Result<String, RenderError>
where
  I: IntoIterator,
  I::Item: AsRef<str>,
{
  let mut rendered = String::new();

  for (index, component) in attribute.into_iter().enumerate() {
    if index > 0 {
      rendered.push('.');
    }

    let component = component.as_ref();
    if component.contains('"') {
      return Err(RenderError::UnrepresentableAttribute(component.to_owned()));
    }

    if component.is_empty() || component.contains('.') {
      rendered.push('"');
      rendered.push_str(component);
      rendered.push('"');
    } else {
      rendered.push_str(component);
    }
  }

  Ok(rendered)
}

impl Installable {
  /// Renders the installable into the arguments Nix expects on its command
  /// line.
  ///
  /// `Flake` renders as a single `reference#attrpath` argument. `File` and
  /// `Expression` render as `--file`/`--expr` followed by their source and
  /// attribute path. `Store` renders as the bare path.
  ///
  /// # Errors
  ///
  /// Returns a [`RenderError`] when a file or store path is not valid UTF-8, or
  /// when an attribute component cannot be represented (see
  /// [`RenderError::UnrepresentableAttribute`]).
  ///
  /// # Examples
  ///
  /// ```
  /// use nix_installable::Installable;
  ///
  /// let installable = Installable::Flake {
  ///   reference: ".".to_string(),
  ///   attribute: vec!["packages".to_string(), "x86_64-linux".to_string()],
  /// };
  /// assert_eq!(installable.to_args().unwrap(), [".#packages.x86_64-linux"]);
  /// ```
  pub fn to_args(&self) -> Result<Vec<String>, RenderError> {
    let path_str = |path: &std::path::Path| {
      path
        .to_str()
        .map(str::to_owned)
        .ok_or(RenderError::NonUtf8Path)
    };

    Ok(match self {
      Self::Flake {
        reference,
        attribute,
      } => vec![format!("{reference}#{}", render_attribute(attribute)?)],
      Self::File { path, attribute } => {
        vec![
          String::from("--file"),
          path_str(path)?,
          render_attribute(attribute)?,
        ]
      },
      Self::Expression {
        expression,
        attribute,
      } => {
        vec![
          String::from("--expr"),
          expression.clone(),
          render_attribute(attribute)?,
        ]
      },
      Self::Store { path } => vec![path_str(path)?],
    })
  }

  /// Returns a short human-readable name for the installable's kind, suitable
  /// for diagnostics.
  #[must_use]
  pub const fn str_kind(&self) -> &'static str {
    match self {
      Self::Flake { .. } => "flake",
      Self::File { .. } => "file",
      Self::Store { .. } => "store path",
      Self::Expression { .. } => "expression",
    }
  }

  /// Returns the installable with `component` appended to its attribute path.
  ///
  /// A store path has no attribute path, so `None` is returned for it.
  ///
  /// # Examples
  ///
  /// ```
  /// use nix_installable::Installable;
  ///
  /// let installable = Installable::Flake {
  ///   reference: ".".to_string(),
  ///   attribute: vec!["config".to_string()],
  /// };
  /// let drv = installable.with_attribute("drvPath").unwrap();
  /// assert_eq!(drv.to_args().unwrap(), [".#config.drvPath"]);
  /// ```
  #[must_use]
  pub fn with_attribute(&self, component: impl Into<String>) -> Option<Self> {
    let mut next = self.clone();
    match &mut next {
      Self::Flake { attribute, .. }
      | Self::File { attribute, .. }
      | Self::Expression { attribute, .. } => attribute.push(component.into()),
      Self::Store { .. } => return None,
    }
    Some(next)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_attribute_empty_input_is_empty_path() {
    assert_eq!(parse_attribute("").unwrap(), Vec::<String>::new());
  }

  #[test]
  fn parse_attribute_splits_on_unquoted_dots() {
    assert_eq!(parse_attribute("foo.bar.baz").unwrap(), [
      "foo", "bar", "baz"
    ]);
  }

  #[test]
  fn parse_attribute_keeps_dots_inside_quotes() {
    assert_eq!(parse_attribute(r#"foo."bar.baz""#).unwrap(), [
      "foo", "bar.baz"
    ]);
  }

  #[test]
  fn parse_attribute_treats_backslash_as_literal() {
    // Nix attribute paths have no escape mechanism, so `\` is an ordinary
    // character both inside and outside quotes.
    assert_eq!(parse_attribute(r"a\b").unwrap(), [r"a\b"]);
    assert_eq!(parse_attribute(r#"a."b\c""#).unwrap(), ["a", r"b\c"]);
  }

  #[test]
  fn parse_attribute_rejects_unclosed_quote() {
    assert!(parse_attribute(r#"foo."bar"#).is_err());
  }

  #[test]
  fn parse_flake_reference_without_attribute_is_empty_path() {
    assert_eq!(
      parse_flake_reference(".").unwrap(),
      (".".to_string(), vec![])
    );
  }

  #[test]
  fn parse_flake_reference_splits_on_first_hash() {
    let (reference, attribute) =
      parse_flake_reference("nixpkgs#legacyPackages.hello").unwrap();
    assert_eq!(reference, "nixpkgs");
    assert_eq!(attribute, ["legacyPackages", "hello"]);
  }

  #[test]
  fn parse_flake_reference_rejects_empty_input() {
    assert!(parse_flake_reference("").is_err());
  }

  #[test]
  fn parse_flake_reference_rejects_missing_reference() {
    assert!(parse_flake_reference("#hello").is_err());
  }

  #[test]
  fn render_attribute_round_trips_through_parse() {
    let path = ["foo", "bar.baz", r"has\slash", ""];
    let rendered = render_attribute(path).unwrap();
    assert_eq!(parse_attribute(&rendered).unwrap(), path);
  }

  #[test]
  fn render_attribute_rejects_unrepresentable_double_quote() {
    let err = render_attribute([r#"has"quote"#]).unwrap_err();
    assert!(matches!(err, RenderError::UnrepresentableAttribute(_)));
  }

  #[test]
  fn to_args_renders_flake_reference_and_attribute() {
    let installable = Installable::Flake {
      reference: "w".to_string(),
      attribute: ["x", "y.z"].into_iter().map(str::to_string).collect(),
    };
    assert_eq!(installable.to_args().unwrap(), [r#"w#x."y.z""#]);
  }

  #[test]
  fn to_args_renders_flake_without_attribute() {
    let installable = Installable::Flake {
      reference: ".".to_string(),
      attribute: vec![],
    };
    assert_eq!(installable.to_args().unwrap(), [".#"]);
  }

  #[test]
  fn to_args_renders_file_with_flags() {
    let installable = Installable::File {
      path:      PathBuf::from("w"),
      attribute: ["x", "y.z"].into_iter().map(str::to_string).collect(),
    };
    assert_eq!(installable.to_args().unwrap(), [
      "--file",
      "w",
      r#"x."y.z""#
    ]);
  }

  #[test]
  fn to_args_renders_expression_with_flags() {
    let installable = Installable::Expression {
      expression: "{ }".to_string(),
      attribute:  vec!["out".to_string()],
    };
    assert_eq!(installable.to_args().unwrap(), ["--expr", "{ }", "out"]);
  }

  #[test]
  fn to_args_renders_store_path_bare() {
    let installable = Installable::Store {
      path: PathBuf::from("/nix/store/abc-hello"),
    };
    assert_eq!(installable.to_args().unwrap(), ["/nix/store/abc-hello"]);
  }

  #[test]
  fn with_attribute_appends_to_flake_attribute() {
    let appended = Installable::Flake {
      reference: ".".to_string(),
      attribute: vec!["config".to_string()],
    }
    .with_attribute("drvPath")
    .expect("flake should accept an appended attribute");
    assert_eq!(appended.to_args().unwrap(), [".#config.drvPath"]);
  }

  #[test]
  fn with_attribute_returns_none_for_store_path() {
    let store = Installable::Store {
      path: PathBuf::from("/nix/store/abc"),
    };
    assert!(store.with_attribute("drvPath").is_none());
  }

  #[test]
  fn str_kind_names_each_variant() {
    assert_eq!(
      Installable::Flake {
        reference: String::new(),
        attribute: vec![],
      }
      .str_kind(),
      "flake"
    );
    assert_eq!(
      Installable::Store {
        path: PathBuf::new(),
      }
      .str_kind(),
      "store path"
    );
  }
}

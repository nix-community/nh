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
//! assert_eq!(installable.to_args(), ["github:NixOS/nixpkgs#hello"]);
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
/// double quotes to contain literal `.` characters, and inside quotes `\`
/// escapes the following character. An empty input yields an empty path.
///
/// # Errors
///
/// Returns an error when a quoted segment is left unclosed, or when a `\`
/// escape inside quotes has no following character.
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
/// assert!(parse_attribute(r#"pkgs."unterminated"#).is_err());
/// ```
pub fn parse_attribute(input: &str) -> Result<Vec<String>, ParseError> {
  let mut components = Vec::new();

  if input.is_empty() {
    return Ok(components);
  }

  let mut in_quote = false;
  let mut current = String::new();
  let mut chars = input.chars();

  while let Some(ch) = chars.next() {
    match ch {
      '.' if !in_quote => {
        components.push(std::mem::take(&mut current));
      },
      '"' => in_quote = !in_quote,
      '\\' if in_quote => {
        let escaped = chars
          .next()
          .ok_or("contains an incomplete quoted attribute escape")?;
        current.push(escaped);
      },
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

/// Renders an attribute path back into a single Nix-quoted string.
///
/// Components that are empty or contain `.`, `"`, or `\` are wrapped in double
/// quotes with `"` and `\` escaped, so the result round-trips through
/// [`parse_attribute`].
fn render_attribute<I>(attribute: I) -> String
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
    let needs_quoting =
      component.is_empty() || component.contains(['.', '"', '\\']);

    if needs_quoting {
      rendered.push('"');
      for ch in component.chars() {
        if matches!(ch, '"' | '\\') {
          rendered.push('\\');
        }
        rendered.push(ch);
      }
      rendered.push('"');
    } else {
      rendered.push_str(component);
    }
  }

  rendered
}

impl Installable {
  /// Renders the installable into the arguments Nix expects on its command
  /// line.
  ///
  /// `Flake` renders as a single `reference#attrpath` argument; `File` and
  /// `Expression` render as `--file`/`--expr` followed by their source and
  /// attribute path; `Store` renders as the bare path. A `File` or `Store`
  /// path that is not valid UTF-8 cannot be expressed as an argument and yields
  /// an empty vector.
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
  /// assert_eq!(installable.to_args(), [".#packages.x86_64-linux"]);
  /// ```
  #[must_use]
  pub fn to_args(&self) -> Vec<String> {
    match self {
      Self::Flake {
        reference,
        attribute,
      } => vec![format!("{reference}#{}", render_attribute(attribute))],
      Self::File { path, attribute } => {
        path.to_str().map_or_else(Vec::new, |path| {
          vec![
            String::from("--file"),
            path.to_string(),
            render_attribute(attribute),
          ]
        })
      },
      Self::Expression {
        expression,
        attribute,
      } => {
        vec![
          String::from("--expr"),
          expression.clone(),
          render_attribute(attribute),
        ]
      },
      Self::Store { path } => {
        path
          .to_str()
          .map_or_else(Vec::new, |path| vec![path.to_string()])
      },
    }
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
  fn parse_attribute_unquotes_escaped_characters() {
    assert_eq!(parse_attribute(r#"foo."bar\"baz"."bar\\baz""#).unwrap(), [
      "foo",
      r#"bar"baz"#,
      r"bar\baz"
    ]);
  }

  #[test]
  fn parse_attribute_rejects_unclosed_quote() {
    assert!(parse_attribute(r#"foo."bar"#).is_err());
  }

  #[test]
  fn parse_attribute_rejects_trailing_escape() {
    assert!(parse_attribute(r#"foo."bar\"#).is_err());
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
    let path = ["foo", "bar.baz", r#"has"quote"#, r"has\slash", ""];
    let rendered = render_attribute(path);
    assert_eq!(parse_attribute(&rendered).unwrap(), path);
  }

  #[test]
  fn to_args_renders_flake_reference_and_attribute() {
    let installable = Installable::Flake {
      reference: "w".to_string(),
      attribute: ["x", "y.z"].into_iter().map(str::to_string).collect(),
    };
    assert_eq!(installable.to_args(), [r#"w#x."y.z""#]);
  }

  #[test]
  fn to_args_renders_flake_without_attribute() {
    let installable = Installable::Flake {
      reference: ".".to_string(),
      attribute: vec![],
    };
    assert_eq!(installable.to_args(), [".#"]);
  }

  #[test]
  fn to_args_renders_file_with_flags() {
    let installable = Installable::File {
      path:      PathBuf::from("w"),
      attribute: ["x", "y.z"].into_iter().map(str::to_string).collect(),
    };
    assert_eq!(installable.to_args(), ["--file", "w", r#"x."y.z""#]);
  }

  #[test]
  fn to_args_renders_expression_with_flags() {
    let installable = Installable::Expression {
      expression: "{ }".to_string(),
      attribute:  vec!["out".to_string()],
    };
    assert_eq!(installable.to_args(), ["--expr", "{ }", "out"]);
  }

  #[test]
  fn to_args_renders_store_path_bare() {
    let installable = Installable::Store {
      path: PathBuf::from("/nix/store/abc-hello"),
    };
    assert_eq!(installable.to_args(), ["/nix/store/abc-hello"]);
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

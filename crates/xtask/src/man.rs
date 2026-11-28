use std::{io::Write, path::Path};

use nh_static::{Documented, EnvVars, NH_VERSION, Since, paths};
use pound::{CommandSpec, Parse};
use semver::Version;

type Entry = (&'static str, &'static str);

const EXIT_STATUSES: &[Entry] = &[
  ("0", "Successful program execution."),
  ("1", "Unsuccessful program execution."),
  ("101", "The program panicked."),
];

const EXAMPLES: &[Entry] = &[
  (
    "Switch to a new NixOS configuration",
    "nh os switch --hostname myhost --specialisation dev",
  ),
  (
    "Rollback to a previous NixOS generation",
    "nh os rollback --to 42",
  ),
  (
    "Switch to a home-manager configuration",
    "nh home switch --configuration alice@work",
  ),
  (
    "Build a home-manager configuration with backup",
    "nh home build --backup-extension .bak",
  ),
  (
    "Switch to a darwin configuration",
    "nh darwin switch --hostname mymac",
  ),
  ("Search for ripgrep", "nh search ripgrep"),
  (
    "Show supported platforms for a package",
    "nh search --platforms ripgrep",
  ),
  ("Clean all but keep 5 generations", "nh clean all --keep 5"),
  (
    "Clean a specific profile",
    "nh clean profile /nix/var/nix/profiles/system",
  ),
];

pub fn generate(out_dir: &str) -> Result<(), String> {
  let gen_dir = Path::new(out_dir);
  std::fs::create_dir_all(gen_dir).map_err(|e| {
    format!("Failed to create output directory '{out_dir}': {e}")
  })?;

  let man_path = gen_dir.join("nh.1");
  let mut buffer: Vec<u8> = Vec::new();

  for page in caress::man::pages(nh_cli::Main::SPEC, "") {
    let content = page
      .content
      .split(".Sh SEE ALSO")
      .next()
      .unwrap_or(&page.content);
    if page.name == "nh" {
      buffer.extend_from_slice(content.as_bytes());
    } else {
      let heading = format!(".Sh \"SUBCOMMAND {}\"\n", page.name);
      buffer.extend_from_slice(heading.as_bytes());
      for line in content.lines().skip(3) {
        let line = line
          .strip_prefix(".Sh ")
          .map_or_else(|| line.to_owned(), |heading| format!(".Ss {heading}"));
        buffer.extend_from_slice(line.as_bytes());
        buffer.push(b'\n');
      }
    }
  }
  check_env_documented(nh_cli::Main::SPEC)?;
  check_since_versions()?;

  let exit_statuses = EXIT_STATUSES
    .iter()
    .map(|&(label, description)| (label, vec![description.to_owned()]));
  render_indented_section("EXIT STATUS", exit_statuses, &mut buffer)?;
  render_environment(&mut buffer)?;
  render_indented_section(
    "FILES",
    paths::DOCUMENTED.iter().map(documented_entry),
    &mut buffer,
  )?;
  render_examples(EXAMPLES, &mut buffer)?;

  std::fs::write(&man_path, buffer).map_err(|e| {
    format!("Failed to write manpage to '{}': {e}", man_path.display())
  })?;

  println!("Generated manpage to {out_dir}");
  Ok(())
}

/// Fail if the CLI reads an environment variable that the man page does not
/// document.
fn check_env_documented(spec: &CommandSpec) -> Result<(), String> {
  for env in spec.args.iter().filter_map(|arg| arg.env) {
    if !EnvVars::DOCUMENTED.iter().any(|var| var.name == env) {
      return Err(format!(
        "`{env}` is read by the CLI but not documented in nh_static::EnvVars"
      ));
    }
  }
  for sub in spec.subs {
    check_env_documented(sub.spec)?;
  }
  for flattened in spec.flattened {
    check_env_documented(flattened)?;
  }
  Ok(())
}

/// Fail if a `#[since("x.y.z")]` is not valid semver or is newer than nh
/// itself, which means it should be `#[since(next)]`.
fn check_since_versions() -> Result<(), String> {
  let current = Version::parse(NH_VERSION)
    .map_err(|e| format!("invalid nh version `{NH_VERSION}`: {e}"))?;
  for var in EnvVars::DOCUMENTED {
    let Some(Since::Version(since)) = var.since else {
      continue;
    };
    let name = var.name;
    let version = Version::parse(since)
      .map_err(|e| format!("`{name}`: invalid #[since(\"{since}\")]: {e}"))?;
    if version > current {
      return Err(format!(
        "`{name}`: #[since(\"{since}\")] is newer than nh {current}; use \
         #[since(next)]"
      ));
    }
  }
  Ok(())
}

/// Turns `documented` into a man page entry.
fn documented_entry(documented: &Documented) -> (&'static str, Vec<String>) {
  let paragraphs = documented
    .paragraphs()
    .map(|paragraph| paragraph.replace('`', ""))
    .chain(documented.since.map(Since::sentence))
    .collect();
  (documented.name, paragraphs)
}

/// Render the ENVIRONMENT section, placing nh's own variables
/// before those of other tools, each section sorted alphabetically by name.
fn render_environment(buffer: &mut Vec<u8>) -> Result<(), String> {
  let (mut own, mut external): (Vec<_>, Vec<_>) = EnvVars::DOCUMENTED
    .iter()
    .partition(|var| var.name.starts_with("NH_"));
  // Sort `_` before letters, so `NIX_*` precedes `NIXOS_*`.
  let key = |var: &&Documented| var.name.replace('_', " ");
  own.sort_by_cached_key(key);
  external.sort_by_cached_key(key);

  let render = || -> std::io::Result<()> {
    writeln!(buffer, ".Sh ENVIRONMENT")?;
    writeln!(buffer, "nh reads the following environment variables:")?;
    render_list(own.into_iter().map(documented_entry), buffer)?;
    writeln!(buffer, ".Ss \"Externally defined variables\"")?;
    writeln!(
      buffer,
      "nh also reads or forwards the following variables, defined by other \
       tools:"
    )?;
    render_list(external.into_iter().map(documented_entry), buffer)
  };
  render().map_err(|e| format!("Failed to write ENVIRONMENT section: {e}"))
}

fn render_indented_section(
  title: &str,
  entries: impl IntoIterator<Item = (&'static str, Vec<String>)>,
  buffer: &mut Vec<u8>,
) -> Result<(), String> {
  let render = || -> std::io::Result<()> {
    writeln!(buffer, ".Sh \"{title}\"")?;
    render_list(entries, buffer)
  };
  render().map_err(|e| format!("Failed to write {title} section: {e}"))
}

/// Render `entries` as a tagged list, separating paragraphs with `.Pp`.
fn render_list(
  entries: impl IntoIterator<Item = (&'static str, Vec<String>)>,
  buffer: &mut Vec<u8>,
) -> std::io::Result<()> {
  writeln!(buffer, ".Bl -tag -width Ds")?;
  for (label, paragraphs) in entries {
    writeln!(buffer, ".It Sy \"{label}\"")?;
    writeln!(buffer, "{}", paragraphs.join("\n.Pp\n"))?;
  }
  writeln!(buffer, ".El")
}

fn render_examples(
  examples: &[Entry],
  buffer: &mut Vec<u8>,
) -> Result<(), String> {
  let mut render = || -> std::io::Result<()> {
    writeln!(buffer, ".Sh EXAMPLES")?;
    writeln!(buffer, ".Bl -tag -width Ds")?;
    for &(description, command) in examples {
      writeln!(buffer, ".It \"{description}\"")?;
      writeln!(buffer, ".Dl \"$ {command}\"")?;
    }
    writeln!(buffer, ".El")
  };
  render().map_err(|e| format!("Failed to write examples section: {e}"))
}

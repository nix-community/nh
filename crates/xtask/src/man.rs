use std::{io::Write, path::Path};

use nh_static::{Documented, EnvVars, paths};
use pound::{CommandSpec, Parse};

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

  let exit_statuses = EXIT_STATUSES
    .iter()
    .map(|&(label, description)| (label, vec![description.to_owned()]));
  render_indented_section("EXIT STATUS", exit_statuses, &mut buffer)?;
  render_indented_section(
    "ENVIRONMENT",
    EnvVars::DOCUMENTED.iter().map(documented_entry),
    &mut buffer,
  )?;
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
  let args = spec.args.iter();
  let flattened = spec.flattened.iter().flat_map(|spec| spec.args);
  for env in args.chain(flattened).filter_map(|arg| arg.env) {
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
    for sub in flattened.subs {
      check_env_documented(sub.spec)?;
    }
  }
  Ok(())
}

/// A man page entry: the label and the paragraphs of its description, with
/// Markdown code spans unwrapped.
fn documented_entry(documented: &Documented) -> (&'static str, Vec<String>) {
  let paragraphs = documented
    .paragraphs()
    .map(|paragraph| paragraph.replace('`', ""))
    .collect();
  (documented.name, paragraphs)
}

fn render_indented_section(
  title: &str,
  entries: impl IntoIterator<Item = (&'static str, Vec<String>)>,
  buffer: &mut Vec<u8>,
) -> Result<(), String> {
  let render = || -> std::io::Result<()> {
    writeln!(buffer, ".Sh \"{title}\"")?;
    writeln!(buffer, ".Bl -tag -width Ds")?;
    for (label, paragraphs) in entries {
      writeln!(buffer, ".It Sy \"{label}\"")?;
      writeln!(buffer, "{}", paragraphs.join("\n.Pp\n"))?;
    }
    writeln!(buffer, ".El")
  };
  render().map_err(|e| format!("Failed to write {title} section: {e}"))
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

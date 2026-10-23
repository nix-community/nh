use std::{io::Write, path::Path};

use pound::Parse;

type Entry = (&'static str, &'static str);

const EXIT_STATUSES: &[Entry] = &[
  ("0", "Successful program execution."),
  ("1", "Unsuccessful program execution."),
  ("101", "The program panicked."),
];

const ENVIRONMENT: &[Entry] = &[
  (
    "NH_NO_CHECKS",
    "When set (any non-empty value), skips startup checks such as Nix version \
     and experimental feature validation. Useful for generating completions \
     or running in constrained build environments.",
  ),
  (
    "NH_ASK",
    "When set to 1, true, or yes, asks for confirmation before applying \
     supported operations. 0, false, or no disable it. Other values are \
     rejected. Equivalent to --ask.",
  ),
  (
    "NH_FLAKE",
    "Preferred path/reference to a directory containing your flake.nix used \
     by NH when running flake-based commands. Historically FLAKE was used; NH \
     will migrate FLAKE into NH_FLAKE if present.",
  ),
  (
    "NH_OS_FLAKE",
    "Command-specific flake reference for nh os commands. Takes precedence \
     over NH_FLAKE.",
  ),
  (
    "NH_HOME_FLAKE",
    "Command-specific flake reference for nh home commands. Takes precedence \
     over NH_FLAKE.",
  ),
  (
    "NH_DARWIN_FLAKE",
    "Command-specific flake reference for nh darwin commands. Takes \
     precedence over NH_FLAKE.",
  ),
  (
    "NH_FILE",
    "Preferred path to a directory/file containing a Nix expression. Chosen \
     after os/home/darwin-specific env vars, but before NH_FLAKE.",
  ),
  (
    "NH_ATTRP",
    "Nix attribute path for an expression specified with NH_FILE. For \
     example, nixosConfigurations.<hostname>.",
  ),
  (
    "NH_SUDO_ASKPASS",
    "Path to a program used as SUDO_ASKPASS when NH self-elevates with sudo.",
  ),
  (
    "NH_PRESERVE_ENV",
    "Controls whether environment variables marked for preservation are \
     passed to elevated commands. Set to \"0\" to disable, \"1\" to force. If \
     unset, defaults to enabled.",
  ),
  (
    "NH_SHOW_ACTIVATION_LOGS",
    "Controls whether activation output is displayed. By default, activation \
     output is hidden. Setting to \"1\" shows full logs.",
  ),
  (
    "NH_LOG",
    "Sets the tracing/log filter for NH. Uses tracing_subscriber format \
     (e.g., nh=trace).",
  ),
  (
    "NH_ROM",
    "Controls whether rom shows build progress. Unset enables it only on a \
     terminal. Set to 1, true, yes, or on to force it on; set to 0, false, \
     no, or off to force it off. --no-rom takes precedence. NH_NOM is a \
     deprecated alias that will be removed in 5.0.",
  ),
  (
    "NH_ROM_LOG",
    "Path to a file that records the Nix log stream of builds shown by rom, \
     so it can be replayed through rom.",
  ),
  (
    "NH_SSHOPTS",
    "SSH options for remote operations. Takes precedence over NIX_SSHOPTS. \
     Accepts the same format as NIX_SSHOPTS.",
  ),
  (
    "NH_SUDOOPTS",
    "Extra arguments inserted into the sudo invocation when NH elevates \
     privileges. Takes precedence over NIX_SUDOOPTS.",
  ),
  (
    "NIXOS_INSTALL_BOOTLOADER",
    "Forwarded to switch-to-configuration. If true, forces bootloader \
     installation. Also available via --install-bootloader.",
  ),
  (
    "NIXOS_NO_CHECK",
    "Forwarded to switch-to-configuration during activation. Inhibits certain \
     NixOS service checks.",
  ),
  (
    "NIX_SSHOPTS",
    "SSH options passed to Nix commands for remote builds. NH_SSHOPTS takes \
     precedence when both are set.",
  ),
  (
    "NIX_SUDOOPTS",
    "Extra arguments inserted into the sudo invocation. Supported for \
     nixos-rebuild compatibility; NH_SUDOOPTS takes precedence.",
  ),
  (
    "NIX_CONFIG",
    "Nix configuration options passed to all Nix commands.",
  ),
  (
    "NIX_REMOTE",
    "Remote store configuration for Nix operations.",
  ),
  (
    "NIX_SSL_CERT_FILE",
    "SSL certificate file for Nix HTTPS operations.",
  ),
  ("NIX_USER_CONF_FILES", "User configuration files for Nix."),
];

const FILES: &[Entry] = &[
  (
    "/nix/var/nix/profiles/system",
    "System profile directory containing system generations.",
  ),
  (
    "/run/current-system",
    "Symlink to the currently active system profile.",
  ),
  (
    "/etc/specialisation",
    "NixOS specialisation detection. Contains the active specialisation name.",
  ),
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

  for page in caress::man::pages(nh::interface::Main::SPEC, "") {
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
  render_indented_section("EXIT STATUS", EXIT_STATUSES, &mut buffer)?;
  render_indented_section("ENVIRONMENT", ENVIRONMENT, &mut buffer)?;
  render_indented_section("FILES", FILES, &mut buffer)?;
  render_examples(EXAMPLES, &mut buffer)?;

  std::fs::write(&man_path, buffer).map_err(|e| {
    format!("Failed to write manpage to '{}': {e}", man_path.display())
  })?;

  println!("Generated manpage to {out_dir}");
  Ok(())
}

fn render_indented_section(
  title: &str,
  entries: &[Entry],
  buffer: &mut Vec<u8>,
) -> Result<(), String> {
  let mut render = || -> std::io::Result<()> {
    writeln!(buffer, ".Sh \"{title}\"")?;
    writeln!(buffer, ".Bl -tag -width Ds")?;
    for &(label, description) in entries {
      writeln!(buffer, ".It Sy \"{label}\"")?;
      writeln!(buffer, "{description}")?;
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

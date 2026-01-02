use std::path::Path;

use clap::CommandFactory;
use roff::{Roff, bold, roman};

pub fn generate(out_dir: &str) -> Result<(), String> {
  let gen_dir = Path::new(out_dir);
  if !gen_dir.exists() {
    std::fs::create_dir_all(gen_dir).map_err(|e| {
      format!("Failed to create output directory '{}': {}", out_dir, e)
    })?;
  }

  let man_path = gen_dir.join("nh.1");
  let mut buffer: Vec<u8> = Vec::new();

  let mut cmd = nh::interface::Main::command();
  let mut man = clap_mangen::Man::new(cmd.clone());
  man = man.manual("nh manual".to_string());
  man
    .render_title(&mut buffer)
    .map_err(|e| format!("Failed to render title: {}", e))?;
  man
    .render_name_section(&mut buffer)
    .map_err(|e| format!("Failed to render name section: {}", e))?;
  man
    .render_synopsis_section(&mut buffer)
    .map_err(|e| format!("Failed to render synopsis section: {}", e))?;
  man
    .render_description_section(&mut buffer)
    .map_err(|e| format!("Failed to render description section: {}", e))?;
  render_command_recursive(&mut cmd, 1, &mut buffer)?;

  // EXIT STATUS section
  let statuses = [
    ("0", "Successful program execution."),
    ("1", "Unsuccessful program execution."),
    ("101", "The program panicked."),
  ];
  let mut sect = Roff::new();
  sect.control("SH", ["EXIT STATUS"]);
  for (code, reason) in statuses {
    sect.control("IP", [code]).text([roman(reason)]);
  }
  sect
    .to_writer(&mut buffer)
    .map_err(|e| format!("Failed to write exit status section: {}", e))?;

  // ENVIRONMENT section
  let env_vars = [
    (
      "NH_NO_CHECKS",
      "When set (any non-empty value), skips startup checks such as Nix \
       version and experimental feature validation. Useful for generating \
       completions or running in constrained build environments.",
    ),
    (
      "NH_FLAKE",
      "Preferred path/reference to a directory containing your flake.nix used \
       by NH when running flake-based commands. Historically FLAKE was used; \
       NH will migrate FLAKE into NH_FLAKE if present.",
    ),
    (
      "NH_OS_FLAKE",
      "Command-specific flake reference for nh os commands. Takes precedence \
       over NH_FLAKE.",
    ),
    (
      "NH_HOME_FLAKE",
      "Command-specific flake reference for nh home commands. Takes \
       precedence over NH_FLAKE.",
    ),
    (
      "NH_DARWIN_FLAKE",
      "Command-specific flake reference for nh darwin commands. Takes \
       precedence over NH_FLAKE.",
    ),
    (
      "NH_SUDO_ASKPASS",
      "Path to a program used as SUDO_ASKPASS when NH self-elevates with sudo.",
    ),
    (
      "NH_PRESERVE_ENV",
      "Controls whether environment variables marked for preservation are \
       passed to elevated commands. Set to \"0\" to disable, \"1\" to force. \
       If unset, defaults to enabled.",
    ),
    (
      "NH_SHOW_ACTIVATION_LOGS",
      "Controls whether activation output is displayed. By default, \
       activation output is hidden. Setting to \"1\" shows full logs.",
    ),
    (
      "NH_LOG",
      "Sets the tracing/log filter for NH. Uses tracing_subscriber format \
       (e.g., nh=trace).",
    ),
    (
      "NH_NOM",
      "Control whether nix-output-monitor (nom) is enabled for build \
       processes. Equivalent of --no-nom.",
    ),
    (
      "NH_REMOTE_CLEANUP",
      "Whether to clean up remote processes on interrupt via pkill. Opt-in \
       due to fragile behavior.",
    ),
    (
      "NIXOS_INSTALL_BOOTLOADER",
      "Forwarded to switch-to-configuration. If true, forces bootloader \
       installation. Also available via --install-bootloader.",
    ),
    (
      "NIX_SSHOPTS",
      "SSH options passed to Nix commands for remote builds.",
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
  let mut sect = Roff::new();
  sect.control("SH", ["ENVIRONMENT"]);
  for (var, desc) in env_vars {
    sect.control("IP", [var]).text([roman(desc)]);
  }
  sect
    .to_writer(&mut buffer)
    .map_err(|e| format!("Failed to write environment section: {}", e))?;

  // FILES section
  let files = [
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
      "NixOS specialisation detection. Contains the active specialisation \
       name.",
    ),
  ];
  let mut sect = Roff::new();
  sect.control("SH", ["FILES"]);
  for (path, desc) in files {
    sect.control("IP", [path]).text([roman(desc)]);
  }
  sect
    .to_writer(&mut buffer)
    .map_err(|e| format!("Failed to write files section: {}", e))?;

  // EXAMPLES section
  let examples = [
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
  let mut sect = Roff::new();
  sect.control("SH", ["EXAMPLES"]);
  for (desc, command) in examples {
    sect
      .control("TP", [])
      .text([roman(desc)])
      .text([bold(format!("$ {}", command))])
      .control("br", []);
  }
  sect
    .to_writer(&mut buffer)
    .map_err(|e| format!("Failed to write examples section: {}", e))?;

  std::fs::write(&man_path, buffer).map_err(|e| {
    format!("Failed to write manpage to '{}': {}", man_path.display(), e)
  })?;

  println!("Generated manpage to {}", out_dir);
  Ok(())
}

fn render_command_recursive(
  cmd: &mut clap::Command,
  depth: usize,
  buffer: &mut Vec<u8>,
) -> Result<(), String> {
  let mut sect = Roff::new();

  // Section header
  let title = if depth == 1 { "OPTIONS" } else { "SUBCOMMAND" };
  sect.control("SH", [title]);

  // About/long_about/help
  if let Some(about) = cmd.get_long_about().or(cmd.get_about()) {
    sect.text([roman(about.to_string())]);
  }

  // Usage
  let usage = cmd.render_usage().to_string();
  sect.control("TP", []);
  sect.text([bold(usage)]);

  // Arguments/options
  for arg in cmd.get_arguments() {
    if arg.is_hide_set() {
      continue;
    }
    sect.control("TP", []);
    let mut opt = String::new();
    if let Some(short) = arg.get_short() {
      opt.push('-');
      opt.push(short);
      if arg.get_long().is_some() {
        opt.push_str(", ");
      }
    }
    if let Some(long) = arg.get_long() {
      opt.push_str("--");
      opt.push_str(long);
    }
    if !opt.is_empty() {
      sect.text([bold(opt)]);
    }
    if let Some(help) = arg.get_help().or(arg.get_long_help()) {
      sect.text([roman(help.to_string())]);
    }
    if let Some(env) = arg.get_env() {
      sect.text([roman(format!(" [env: {}]", env.to_string_lossy()))]);
    }
    let mut defaults_iter = arg.get_default_values().iter();
    if let Some(default) = defaults_iter.next() {
      sect.text([roman(format!(" [default: {}]", default.to_string_lossy()))]);
    }
    if arg.is_required_set() {
      sect.text([roman(" [required]")]);
    }
  }

  sect
    .to_writer(buffer)
    .map_err(|e| format!("Failed to render command section: {}", e))?;

  // Subcommands
  for sub in cmd.get_subcommands_mut() {
    render_command_recursive(sub, depth + 1, buffer)?;
  }

  Ok(())
}

use std::path::Path;

use clap::CommandFactory;
use roff::{Roff, bold, roman};

pub fn r#gen(out_dir: &str) {
  let gen_dir = Path::new(out_dir);
  if !gen_dir.exists() {
    std::fs::create_dir_all(gen_dir)
      .expect("failed to create output directory");
  }
  gen_man(gen_dir);
}

fn gen_man(base_dir: &Path) {
  let man_path = base_dir.join("nh.1");
  let mut buffer: Vec<u8> = Vec::new();

  let mut cmd = nh::interface::Main::command();
  let mut man = clap_mangen::Man::new(cmd.clone());
  man = man.manual("nh manual".to_string());
  man.render_title(&mut buffer).unwrap();
  man.render_name_section(&mut buffer).unwrap();
  man.render_synopsis_section(&mut buffer).unwrap();
  man.render_description_section(&mut buffer).unwrap();
  render_command_recursive(&mut cmd, 1, &mut buffer);

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
  sect.to_writer(&mut buffer).unwrap();

  // EXAMPLES section
  let examples = [
    (
      "Switch to a new NixOS configuration",
      "nh os switch --hostname myhost --specialisation dev",
      "",
    ),
    (
      "Rollback to a previous NixOS generation",
      "nh os rollback --to 42",
      "",
    ),
    (
      "Switch to a home-manager configuration",
      "nh home switch --configuration alice@work",
      "",
    ),
    (
      "Build a home-manager configuration with backup",
      "nh home build --backup-extension .bak",
      "",
    ),
    (
      "Switch to a darwin configuration",
      "nh darwin switch --hostname mymac",
      "",
    ),
    ("Search for ripgrep", "nh search ripgrep", ""),
    (
      "Show supported platforms for a package",
      "nh search --platforms ripgrep",
      "",
    ),
    (
      "Clean all but keep 5 generations",
      "nh clean all --keep 5",
      "",
    ),
    (
      "Clean a specific profile",
      "nh clean profile /nix/var/nix/profiles/system",
      "",
    ),
  ];
  let mut sect = Roff::new();
  sect.control("SH", ["EXAMPLES"]);
  for (desc, command, result) in examples {
    sect
      .control("TP", [])
      .text([roman(desc)])
      .text([bold(format!("$ {}", command))])
      .control("br", []);
    if !result.is_empty() {
      sect.text([roman(result)]);
    }
  }
  sect.to_writer(&mut buffer).unwrap();

  std::fs::write(man_path, buffer).expect("failed to write manpage");
}

fn render_command_recursive(
  cmd: &mut clap::Command,
  depth: usize,
  buffer: &mut Vec<u8>,
) {
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

  sect.to_writer(buffer).unwrap();

  // Subcommands
  for sub in cmd.get_subcommands_mut() {
    render_command_recursive(sub, depth + 1, buffer);
  }
}

<!-- markdownlint-disable no-inline-html first-line-heading line-length -->
<div align="center">
  <h1 id="header">nh</h1>
  <a alt="CI" href="https://github.com/nix-community/nh/actions">
    <img
      src="https://github.com/nix-community/nh/actions/workflows/build.yaml/badge.svg"
      alt="Build Status"
    />
  </a>
  <a alt="Deps" href="https://deps.rs/repo/github/nix-community/nh">
    <img
      src="https://deps.rs/repo/github/nix-community/nh/status.svg"
      alt="Dependency Status"
    />
  </a>
  <a alt="License" href="https://github.com/nix-community/nh/blob/master/LICENSE">
    <img
      src="https://img.shields.io/github/license/nix-community/nh?label=License"
      alt="License"
    />
  </a>
  <br/>
  <h6>Because the name "yet-another-<u>n</u>ix-<u>h</u>elper" was too long to type...</h1>
  <br/>
  <a href="#what-does-it-do">Synopsis</a><br/>
  <a href="#features">Features</a> | <a href="#usage">Usage</a><br/>
  <a href="#hacking">Contributing</a>
  <br/>
</div>

## What Does it Do?

NH is a modern helper utility that aims to consolidate and reimplement some of
the commands from various tools within the NixOS ecosystem. Our goal is to
provide a cohesive, easily-understandable interface with more features, better
ergonomics and at many times better _speed_. In addition to bringing together
relevant 3rd party projects, NH also acts a super-convenient all-in-one utility
that reimplements well known Nix commands.

## Features

- **Unified CLI**: Consistent, intuitive interface for NixOS, Home Manager, and
  Darwin workflows.
- **Rich Interface**: Each major function (`os`, `home`, `darwin`, `search`,
  `clean`) exposes granular subcommands and flags for fine-tuned control.
- **Eye Candy**: It looks great, without any compromise. I mean who does not
  love some cool looking UIs?
- **Enhanced Garbage Collection**: `nh clean` extends `nix-collect-garbage` with
  gcroot cleanup, profile targeting, and time-based retention.
- **Build-tree Visualization**: `nh os` and similar commands display build trees
  for clear dependency tracking.
- **Diff & Change Review**: Integrated, super-fast diffing of derivation changes
  before activation or switch.
- **Specialisation Support**: Easily select or ignore NixOS & Home-Manager
  specialisations via flags.
- **Generation Management**: Inspect, rollback, and manage system generations
  with explicit targeting.
- **Extensible & Futureproof**: Designed for seamless, rapid addition of new
  subcommands and flags.

## Status

[update request]: https://github.com/NixOS/nixpkgs/issues

<a href="https://repology.org/project/nh/versions">
    <img
      src="https://repology.org/badge/vertical-allrepos/nh.svg"
      alt="Packaging status"
      align="right"
      style="padding-left: 20px"
    >
</a>

NH is packaged in nixpkgs, and is available under both nixpkgs stable and
nixpkgs unstable. Outside of extreme circumstances, all updates will be
backported to the stable branch. Refer to the [installation](#installation)
section for more details. Make sure you submit an [update request] in Nixpkgs if
the package is outdated.

## Usage

One of the features and the core principles of NH is to provide a clean, uniform
and intuitive CLI for its users. The `nh` command offers several subcommands,
all with their extensive CLI flags for extensive configuration.

### Global Subcommands

- `nh search` - a super-fast package searching tool (powered by an Elasticsearch
  client) for Nix packages in supported Nixpkgs branches.
  <p align="center">
    <img
      alt="nh search showcase"
      src="./.github/nh_search_screenshot.png"
      width="750px"
    >
  </p>

- `nh clean` - a re-implementation of `nix-collect-garbage` that also collects
  gcroots.
  <p align="center">
    <img
      alt="nh clean showcase"
      src="./.github/nh_clean_screenshot.png"
      width="750px"
    >
  </p>

### Platform Specific Subcommands

- `nh os` - reimplements `nixos-rebuild`[^1] with the addition of
  - build-tree displays.
  - diff of changes.
  - confirmation.
  <p align="center">
    <img
      alt="nh os switch showcase"
      src="./.github/nh_switch_screenshot.png"
      width="750px"
    >
  </p>

- `nh home` - reimplements `home-manager`.
- `nh darwin` - reimplements `darwin-rebuild`.

[^1]: `nh os` does not yet provide full feature parity with `nixos-rebuild`.
    While a large collection of subcommands have been implemented, you might be
    missing some features. Please visit
    [#358](https://github.com/nix-community/nh/issues/358) for a roadmap.

> [!TIP]
> See the help page for individual subcommands, or `man 1 nh` for more
> information on each subcommand.

## Installation

The latest, tagged version is available in Nixpkgs as **NH stable**. This is
recommended for most users, as tagged releases will usually undergo more
testing.This repository also provides the latest development version of NH,
which you can get from the flake outputs.

```sh
nix shell nixpkgs#nh # stable
nix shell github:nix-community/nh # dev
```

You can try NH in a Nix shell today, no setup required!

### NixOS

We provide a NixOS module that integrates `nh clean` as a service. To enable it,
set the following configuration:

```nix
{
  programs.nh = {
    enable = true;
    clean.enable = true;
    clean.extraArgs = "--keep-since 4d --keep 3";
    flake = "/home/user/my-nixos-config"; # sets NH_OS_FLAKE variable for you
  };
}
```

> [!TIP]
> As of 4.0, NH fully supports both **Nix flakes** and classical NixOS
> configurations via channels or manual dependency pinning and the such. Please
> consider the new API mature, but somewhat experimental as it is a new
> addition. Remember to report any bugs!

- For flakes, the command is `nh os switch /path/to/flake`
- For a classical configuration:
  - `nh os switch -f '<nixpkgs/nixos>'`, or
  - `nh os switch -f '<nixpkgs/nixos>' -- -I
  nixos-config=/path/to/configuration.nix`
    if using a different location than the default.

You might want to check `nh os --help` for other values and the defaults from
environment variables.

#### Specialisations support

NH is capable of detecting which specialisation you are running, so it runs the
proper activation script. To do so, you need to give NH some information of the
spec that is currently running by writing its name to `/etc/specialisation`. The
config would look like this:

```nix
{config, pkgs, ...}: {
  specialisation."foo".configuration = {
    environment.etc."specialisation".text = "foo";
    # ..rest of config
  };

  specialisation."bar".configuration = {
    environment.etc."specialisation".text = "bar";
    # ..rest of config
  };
}
```

#### Home-Manager

Home specialisations are read from `~/.local/share/home-manager/specialisation`.
The config would look like this:

```nix
{config, pkgs, ...}: {
  specialisation."foo".configuration = {
    xdg.dataFile."home-manager/specialisation".text = "foo";
    # ..rest of config
  };

  specialisation."bar".configuration = {
    xdg.dataFile."home-manager/specialisation".text = "bar";
    # ..rest of config
  };
}
```

## Environment variables

NH supports several environment variables to control command behaviour. Some of
the common variables that you may encounter or choose to employ are as follows:

### Global

- `NIXOS_INSTALL_BOOTLOADER`
  - This is a variable accepted by `switch-to-configuration`, which handles the
    system switching behind the scenes. If `true`, `swith-to-configuration` will
    call the necessary script to force and installation of your bootloader. This
    behaviour can also be replicated by passing `--install-bootloader` to
    `nh os switch` and `nh os boot` commands.

### NH Specific

- `NH_NO_CHECKS`
  - When set (any non-empty value), skips startup checks such as Nix version and
    experimental feature validation. Useful for generating completions or
    running in constrained build environments. You can also consider this an
    "expert flag" that you can set for a non-zero performance benefit. It
    assumes you know what you are doing.

- `NH_FLAKE`
  - Preferred flake path/reference used by NH when running flake-based commands.
    Historically `FLAKE` was used; NH will migrate `FLAKE` into `NH_FLAKE` if
    present and the specific `NH_*_FLAKE` vars are not set.

- `NH_OS_FLAKE`, `NH_HOME_FLAKE`, `NH_DARWIN_FLAKE`
  - Command-specific flake references for `os`, `home`, and `darwin` commands
    respectively. If present they take precedence over `NH_FLAKE`.

- `NH_SUDO_ASKPASS`
  - Path to a program used as `SUDO_ASKPASS` when NH self-elevates with `sudo`.
    If set and `sudo` is used for elevation, NH will pass `-A` to `sudo` and set
    `SUDO_ASKPASS` accordingly.

- `NH_PRESERVE_ENV`
  - Controls whether environment variables marked for preservation are passed to
    elevated commands. Set to `"0"` to disable preservation, `"1"` to force
    preservation. If unset, preservation defaults to enabled.

- `NH_LOG`
  - Sets the tracing/log filter for NH. This uses the same format as
    `tracing_subscriber` env filters (for example: `nh=trace`).

- `NH_NOM`
  - Control whether `nom` (nix-output-monitor) should be enabled for the build
    processes. Equivalent of `--no-nom`.

### Notes

- Any environment variables prefixed with `NH_` are explicitly propagated by NH
  to commands when appropriate.
- For backwards compatibility, if `FLAKE` is present and none of the
  command-specific `NH_*_FLAKE` variables exist, NH will set `NH_FLAKE` from
  `FLAKE` and emit a warning recommending migration to `NH_FLAKE`. `FLAKE` will
  be removed in the future versions of NH.

## Hacking

Contributions are always welcome. To get started, just clone the repository and
run `nix develop`. We also provide a `.envrc` for Direnv users, who may use
`direnv allow` to enter a shell with the necessary dependencies.

### Structure

NH consists of two modules. The core of NH is found in the `src` directory, and
is separated into different modules. Some of the critical modules that you may
want to be aware of are `nh::commands` for command interfaces, `nh::checks` for
pre-startup checks and `nh::util` to store shared logic. Platform-specific logic
is placed in the appropriate platform module, such as `nh::nixos` or
`nh::darwin` with generic helpers placed in `nh::util`.

The `xtask` directory contains the cargo-xtask tasks used by NH, used to
generate manpages and possibly more in the future. Some of the

### Submitting Changes

Once your changes are complete, remember to run [fix.sh](./fix.sh) to apply
general formatter and linter rules that will be expected by the CI.

Lastly, update the [changelog](/CHANGELOG.md) and open your pull request.

## Attributions

[nix-output-monitor]: https://github.com/maralorn/nix-output-monitor
[dix]: https://github.com/bloxx12/dix

NH would not be possible without all the tools we run under the hood

- Tree of builds with [nix-output-monitor].
- Visualization of the upgrade diff with [dix].
- And of course, all the [crates](./Cargo.toml) we depend on.

Last but not least, thank you to those who contributed to NH or simply talked
about it on various channels. NH would not be where it is without you.

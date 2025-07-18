<!-- markdownlint-disable no-inline-html -->
<!-- markdownlint-disable first-line-heading -->
<div align="center">
  <h1>nh</h1>
  <!-- markdownlint-disable line-length -->
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
- **Extensible & Futureproof**: Designed for rapid addition of new subcommands
  and flags.

## Usage

One of the features and the core principles of NH is to provide a clean, uniform
and intuitive CLI for its users. The `nh` command offers several subcommands,
all with their extensive CLI flags for extensive configuration.

### Global Subcommands

<p align="center">
  <img
    alt="nh feature showcase"
    src="./.github/screenshot.png"
    width="800px"
  >
</p>

- `nh search` - a super-fast package searching tool (powered by a ElasticSearch
  client).
- `nh clean` - a re-implementation of `nix-collect-garbage` that also collects
  gcroots.

### Platform Specific Subcommands

- `nh os` - reimplements `nixos-rebuild`[^1] with the addition of
  - build-tree displays.
  - diff of changes.
  - confirmation.
- `nh home` - reimplements `home-manager`.
- `nh darwin` - which reimplements `darwin-rebuild`.

[^1]: `nh os` does not yet provide full feature parity with `nixos-rebuild`.
    While a large collection of subcommands have been implemented, you might be
    missing some features. Please visit
    [#254](https://github.com/nix-community/nh/issues/254) for a roadmap.

See the help page for individual subcommands, or `man 1 nh` for more information
on each subcommand.

## Installation

The latest, tagged version is available in Nixpkgs as **NH stable**. This is
recommended for most users, as tagged releases will usually undergo more
testing.This repository also provides the latest development version of NH,
which you can get from the flake outputs.

```sh
nix shell nixpkgs#nh # stable
nix shell github:nix-community/nh # dev
```

You can try NH today in a Nix shell today, no setup required!

### NixOS

We provide a NixOS module that integrates `nh clean` as a service. To enable it,
set the following configuration:

```nix
{ config, pkgs, ... }:
{
  programs.nh = {
    enable = true;
    clean.enable = true;
    clean.extraArgs = "--keep-since 4d --keep 3";
    flake = "/home/user/my-nixos-config";
  };
}
```

As of 4.0, NH supports both **Flakes** and classical NixOS configurations
(channels or manual pinning.)

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

## Hacking

Contributions are always welcome. To get started, just clone the repository and
run `nix develop`. We also provide a `.envrc` for Direnv users, who may use
`direnv allow` to enter a shell with the necessary dependencies.

### Structure

NH consists of two modules. The core of NH is found in the `src` directory, and
is separated into different modules. Some of the critical modules that you may
want to be aware of are `nh::commands` for command interfaces, `nh::checks` for
pre-startup checks and `nh::util` to store shared logic.

The `xtask` directory contains the cargo-xtask tasks used by NH, used to
generate manpages and possibly more in the future. Some of the

### Submitting Changes

Once your changes are complete, remember to run [fix.sh](./fix.sh) to apply
general formatter and linter rules that will be expected by the CI.

Lastly, update the [changelog](/CHANGELOG.md) and open your pull request.

## Attributions

[nix-output-monitor]: https://github.com/maralorn/nix-output-monitor
[dix]: https://github.com/bloxx12/dix

NH would not be possible without that nh runs under the hood:

- Tree of builds with [nix-output-monitor].
- Visualization of the upgrade diff with [dix].
- And of course, all the [crates](./Cargo.toml) we depend on.

Last but not least, thank you to those who contributed to NH or simply talked
about it on various channels. NH would not be where it is without you.

## Status

[![Dependency status](https://deps.rs/repo/github/nix-community/nh/status.svg)](https://deps.rs/repo/github/nix-community/nh)

[![Packaging status](https://repology.org/badge/vertical-allrepos/nh.svg)](https://repology.org/project/nh/versions)

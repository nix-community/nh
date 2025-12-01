<!-- markdownlint-disable MD033 MD041 -->
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
the commands and interfaces from various tools within the Nix/NixOS ecosystem.
Our goal is to provide a **cohesive**, **easily-understandable** interface with
more features, better ergonomics and at many times better _speed_. In addition
to acting as a super-convenient, all-in-one utility that reimplements well-known
and commonly used Nix commands, NH is a _pretty_ tool that brings together
relevant 3rd party projects that you might be familiar with.

To get started with NH, skip to the [Usage](#usage) section.

## Features

- **Unified CLI**: Consistent, intuitive interface for many **Nix**, **NixOS**,
  **Home Manager**, and **Darwin** workflows.
  - **Rich Interface**: Each major function (`os`, `home`, `darwin`, `search`,
    `clean`) exposes granular subcommands and flags for fine-tuned control.
  - **Enhanced Garbage Collection**: `nh clean` extends `nix-collect-garbage`
    with gcroot cleanup, profile targeting, and time-based retention.
  - **Faster Nix Search**: search Nixpkgs via Elasticsearch for faster results
- **Eye Candy**: It looks great, without any compromise. I mean who does not
  love some cool looking UIs?
  - **Build-tree Visualization**: `nh os` and similar commands display build
    trees for clear dependency tracking.
  - **Diff & Change Review**: Integrated, super-fast diffing of derivation
    changes before activation or switch.
- **Specialisation Support**: Easily select or ignore NixOS & Home-Manager
  specialisations via flags.
- **Generation Management**: Inspect, rollback, and manage system generations
  with explicit targeting.
- **Extensible & Futureproof**: Designed for seamless, rapid addition of new
  subcommands and flags.

### Design

[Discussions]: https://github.com/nix-community/nh/discussions
[Issues]: https://github.com/nix-community/nh/issues

NH is an _unified_ CLI, meaning it aims to bring together core platform support
into a single, convenient utility. For the time being, this appears in NH's
interface as support for **NixOS** (first-class), **Home Manager** and
**Nix-Darwin**. We hope to provide a familiar, convenient and _good looking_
interface for users of any and all of those projects.

The familiar interface of NH should not be seen as a weakness, however, as NH is
NOT a nixos-rebuild wrapper, and is not constrained by the limits of such tools.
Simply put, our goal is to implement _not only_ what is available but instead go
above and beyond to implement additional commands and flags to **improve the
experience provided by the default tools**. Which is to say _plumbing_ and _sane
defaults_ matter. You get to enjoy the very tools that you are accustomed to,
with the benefits of a faster and safer language.

> [!IMPORTANT]
> Future goals of NH include providing support for lower-levels tools, such as
> `nixos-install`, and `nixos-generate-config` to become a drop-in replacement
> for most critical tools with the benefits of Rust. If you would like to
> provide feedback and create feature requests, those are welcome over at the
> [Discussions] and [Issues] tabs respectively.

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

### Installation

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

## Usage

One of the features and the core principles of NH is to provide a clean, uniform
and intuitive CLI for its users. The `nh` command offers several subcommands,
all with their extensive CLI flags for extensive configuration.

Under the `nh` command, there are two types of commands that you'll be
interested in:

### Global Subcommands

Global subcommands implement functionality around core Nix commands. As it
stands, we provide a **better search** and **better garbage collection**.

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

Platform specific subcommands are those that implement CLI utilities for
**NixOS**, **Home Manager** and **Nix-Darwin**.

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

> [!TIP]
> NH supports various flags, [environment variables](#environment-variables) and
> setups to provide the best possible user experience. See the `--help` page for
> individual subcommands, or `man 1 nh` for more information on each subcommand
> with examples.

[^1]: `nh os` does not yet provide full feature parity with `nixos-rebuild`.
    While a large collection of subcommands have been implemented, you might be
    missing some features. Please visit
    [#358](https://github.com/nix-community/nh/issues/358) for a roadmap.

### Command Examples

<!-- markdownlint-disable MD013 -->

| Platform     | Old Command (Without NH)                 | New Command (With NH)          |
| ------------ | ---------------------------------------- | ------------------------------ |
| NixOS        | `nixos-rebuild switch --flake .#myHost`  | `nh os switch . -H myHost`     |
| Darwin       | `darwin-rebuild switch --flake .#myHost` | `nh darwin switch . -H myHost` |
| Home Manager | `home-manager switch --flake .#myHost`   | `nh home switch . -C myHome`   |

<!-- markdownlint-enable MD013 -->

If the `NH_FLAKE` variable is set, NH allows omitting the path to your flake.
This is done automatically in the modules for NH in the NixOS and Home-Manager
modules if `programs.nh.flake` is set.

NH also allows omitting the hostname (`-H`) for NixOS/Darwin and the
configuration (`-C`) parameters when it can be autodiscovered on the system. For
example, if `NH_FLAKE` or `NH_OS_FLAKE` is set you may simply run `nh os switch`
with no additional arguments, and it will automatically resolve
`nixosConfigurations.<myHost>`.

## Environment variables

NH supports several environment variables to control command behaviour. Some of
the common variables that you may encounter or choose to employ are as follows:

### Global

- `NIX_SSHOPTS`, `NIX_CONFIG`, `NIX_REMOTE`, `NIX_SSL_CERT_FILE` and
  `NIX_USER_CONF_FILES` are forwarded in all Nix commands with environment
  isolation.
- `NIXOS_INSTALL_BOOTLOADER`
  - This is a variable accepted by `switch-to-configuration`, which handles the
    system switching behind the scenes. If `true`, `switch-to-configuration`
    will call the necessary script to force and installation of your bootloader.
    This behaviour can also be replicated by passing `--install-bootloader` to
    `nh os switch` and `nh os boot` commands.

### NH Specific

- `NH_NO_CHECKS`
  - When set (any non-empty value), skips startup checks such as Nix version and
    experimental feature validation. Useful for generating completions or
    running in constrained build environments. You can also consider this an
    "expert flag" that you can set for a non-zero performance benefit. It
    assumes you know what you are doing.

- `NH_FLAKE`
  - Preferred path/reference to a directory containing your `flake.nix` used by
    NH when running flake-based commands. Historically `FLAKE` was used; NH will
    migrate `FLAKE` into `NH_FLAKE` if present and the specific `NH_*_FLAKE`
    vars are not set.

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

- `NH_SHOW_ACTIVATION_LOGS`
  - Controls whether activation output is displayed. By default, activation
    output is hidden. Setting this to `"1"` will show the full activation logs,
    which is useful for debugging activation failures. Supported on all
    platforms (NixOS, Home Manager, and Darwin).

- `NH_LOG`
  - Sets the tracing/log filter for NH. This uses the same format as
    `tracing_subscriber` env filters (for example: `nh=trace`).

- `NH_NOM`
  - Control whether `nom` (nix-output-monitor) should be enabled for the build
    processes. Equivalent of `--no-nom`.

### Notes

- Any environment variables prefixed with `NH_` are explicitly propagated by NH
  to commands when appropriate, i.e., in environment isolation.
- For backwards compatibility, if `FLAKE` is present and none of the
  command-specific `NH_*_FLAKE` variables exist, NH will set `NH_FLAKE` from
  `FLAKE` and emit a warning recommending migration to `NH_FLAKE`. `FLAKE` will
  be removed in the future versions of NH.

## Hacking

Contributions are always welcome. To get started, just clone the repository and
run `nix develop`. We also provide a `.envrc` for Direnv users, who may use
`direnv allow` to enter a shell with the necessary dependencies.

### Structure

[cargo-xtask]: https://github.com/matklad/cargo-xtask

NH is written in the Rust programming language, and consists of two modules. The
core of NH is found in the `src` directory, and is separated into different
modules. Some of the critical modules that you may want to be aware of are
`nh::commands` for command interfaces, `nh::checks` for pre-startup checks and
`nh::util` to store shared logic. Platform-specific logic is placed in the
appropriate platform module, such as `nh::nixos` or `nh::darwin` with generic
helpers placed in `nh::util`.

The `xtask` directory contains the [cargo-xtask] tasks used by NH, used to
generate manpages and possibly more in the future.

### Submitting Changes

Once your changes are complete, please remember to run the fixup script in
[fix.sh](./fix.sh) to apply general formatter and linter rules that will be
expected by the CI. This is optional, but some CI steps (such as formatting) is
required for a merge.

You will also want to update the [changelog](/CHANGELOG.md) with sufficient
amount of information to detail the new behaviour before you create your
changes.

This might seem daunting, but it isn't. Even if you _don't_ meet those
requirements, you'll be gently nudged to make your changes. Friendly
contributions are always welcome.

### Profiling Allocations and Timing

[Hotpath]: https://github.com/pawurb/hotpath

NH has an experimental (as in, we're still experimenting with the idea) of
[Hotpath] for profiling function execution timing and heap allocations. This
helps identify performance bottlenecks and track optimization progress.

Unfortunately, due to the nature of NH, it is a little difficult to keep track
of timings but allocation tracking is generally helpful in identifying possible
optimizations.

To profile allocations:

```bash
HOTPATH_JSON=true cargo run --features=hotpath,hotpath-alloc
```

To profile timing:

```bash
HOTPATH_JSON=true cargo run --features=hotpath
```

The JSON output can be analyzed with the `hotpath` CLI tool for detailed
performance metrics. On pull requests, GitHub Actions automatically profiles
both timing and allocations, posting comparison comments to help catch
performance regressions.

## Attributions

[faukah]: https://github.com/faukah
[ViperML]: https://github.com/viperML
[nvd]: https://sr.ht/~khumba/nvd/
[dix]: https://github.com/bloxx12/dix
[nix-output-monitor]: https://github.com/maralorn/nix-output-monitor
[crates]: ./Cargo.toml

NH has had a long history, and it has grown a lot over the years. I, NotAShelf,
would first like to extend my thanks to [ViperML] for his immense efforts as the
creator and the first maintainer of NH. I have recently taken over NH, but none
of this would be possible without him.

Next, I would like to thank the tools we run under the hood. The visualization
of upgrade diffs are provided by the [dix] crate, which is created by my good
friend [faukah]. Compared to the previous diffing utility, [nvd], dix is more
than twice as fast and has been a blessing to NH's diffing experience. Thank
you!

[nix-output-monitor], is also a very good utility worth a mention, which NH uses
under the hood for the pretty tree of builds. A big shoutout to
nix-output-monitor for providing many NH users such as myself with pretty build
visuals.

I also would like to extend my thanks to the many Rust [crates] that power NH
under the hood and give it its signature UX. Without the beautiful Rust
ecosystem, NH could not be where it is.

Last but not least I would like to thank... YOU! Thank you to everyone who has
contributed to NH, talked about NH or criticized NH to allow further
improvement. NH would not be where it is without you.

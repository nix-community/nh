<!-- markdownlint-disable no-duplicate-heading -->

# NH Changelog

<!--
This is the Nh changelog. It aims to describe changes that occurred within the
codebase, to the extent that concerns *both users and contributors*. If you are
a contributor, please add your changes under the "Unreleased" section as tags
will be created at the discretion of maintainers. If your changes fix an
existing bug, you must describe the new behaviour (ideally in comparison to the
old one) and put it under the "Fixed" subsection. Linking the relevant open
issue is not necessary, but good to have. Otherwise, general-purpose changes can
be put in the "Changed" section or, if it's just to remove code or
functionality, under the "Removed" section.
-->

## Unreleased

### Changed

- `nh os info` now support `--fields` to select which field(s) to display; also
  add a per-generation "Closure Size" coloumn.
  ([#375](https://github.com/nix-community/nh/issues/375))
- `nh os switch` and `nh os boot` now support the `--install-bootloader` flag,
  which will explicitly set `NIXOS_INSTALL_BOOTLOADER` for
  `switch-to-configuration`. Bootloader behaviour was previously supported by
  explicitly passing the variable to `nh` commands, which has now been made
  explicit through the `--install-bootloader` flag.
  ([#424](https://github.com/nix-community/nh/issues/424))
- A `--run` flag was added to `nh os build-vm`, which allows immediately
  starting a built VM after the build is complete. This can be chained with
  other `build-vm` flags such as `--with-bootloader`.
- Switched from owo-colors to Yansi as the internal coloring library. This
  should not affect end-users, but please create an issue if you notice anything
  different.

## 4.2.0

### Changed

- Nh checks are now more robust in the sense that unnecessary features will not
  be required when the underlying command does not depend on them.
- The `--update-input` flag now supports being specified multiple times.
- The `--update-input` flag no longer requires `--update` in order to take
  effect, and both flags are now considered mutually exclusive. If you specify
  the `--update` flag, all flake inputs will be updated. If you specify the
  `--update-input NAME` flag, only the specified flake(s) will be updated.
- `nh darwin switch` now shows the output from the `darwin-rebuild` activation.
  This allows you to see more details about the activation from `nix-darwin`, as
  well as `Home Manager`.
- `nvd` is replaced by `dix`, resulting in saner and faster diffing.
- Nh now supports a new `--diff` flag, which takes one of `auto` `always`
  `never` and toggles displaying the package diff after a build.
- Manpages have been added to nh, and will be available as `man 1 nh` if the
  package vendor provides them.
- `nh clean` will now skip directories that are checked and don't exist. Instead
  of throwing an error, it will print a warning about which directories were
  skipped.
- nh's verbosity flag can now be passed multiple times for more verbose debug
  output.
- `nh search` will now use the system trust store for it's HTTPS requests.
- Error handling has been improved across the board, with more contextful errors
  replacing direct error propagation or unwraps.
- The directory traversal during `nh clean` has been improved slightly and
  relevant bits of the clean module has been sped up.
  - It's roughly %4 faster according to testing, but IO is still a limiting
    factor and results may differ.
- Added more context to some minor debug messages across platform commands.
- Nh now supports alternative privilege escalation methods. Namely `doas`,
  `run0` and a fallback `pkexec` strategies will be attempted if the system does
  not use `sudo`.
- Nh will correctly prompt you for your `sudo` password while deploying
  remotely. This helps mitigate the need to allow password-less `sudo` on the
  target host to deploy remotely.

### Fixed

- Nh will now correctly detect non-semver version strings, such as `x.ygit`.
  Instead of failing the check, we now try to normalize the string and simply
  skip the check with a warning.
- In the case system switch is disabled (`system.switch enable = false;`) Nh
  will provide a more descriptive error message hinting at what might be the
  issue. ([#331](https://github.com/nix-community/nh/issues/331))
  - We cannot accurately guess what the issue is, but this should be more
    graceful than simply throwing an error about a missing path (what path?)
- Nh will now carefully pick environment variables passed to individual
  commands. This resolves the "`$HOME` is not owned by you!" error, but it's
  also a part of a larger refactor that involves only providing relevant
  variables to individual commands. This is an experimental change, please let
  us know if you face any new bugs.
  ([#314](https://github.com/nix-community/nh/issues/314))
- Fixed a tempdir race condition causing activation failures.
  [#386](https://github.com/nix-community/nh/pull/386)

## 4.1.2

### Changed

- The environment and Nix feature checks have been made more robust, which
  should allow false positives caused by the initial implementation
  - Version normalization for the Nix version is now much more robust. This gets
    rid of unexpected breakage when using, e.g., `pkgs.nixVersions.git`
- Support for additional Nix variants have been added. This allows for us to
  handle non-supported Nix variants gracefully, treating them as mainline Nix.
- Version check regex in checks module is now compiled only once, instead of in
  a loop.

## 4.1.1

### Changed

- Nh is now built on Cargo 2024 edition. This does not imply any changes for the
  users, but contributors might need to adapt.

- `nh os build` and `nh os build-vm` now default to placing the output at
  `./result` instead of a temp directory.

### Fixed

- The Elasticsearch backend version has been updated to v43, which fixes failing
  search commands ([#316](https://github.com/nix-community/nh/pull/316))

## 4.1.0

### Added

- A new `nh os rollback` subcommand has been added to allow rolling back a
  generation, or to a specific generation with the `--to` flag. See
  `nh os rollback --help` for more details on this subcommand.

- Nh now supports the `--build-host` and `--target-host` cli arguments

- Nh now checks if the current Nix implementation has necessary experimental
  features enabled. In mainline Nix (CppNix, etc.) we check for `nix-command`
  and `flakes` being set. In Lix, we also use `repl-flake` as it is still
  provided as an experimental feature in versions below 2.93.0.

- Nh will now check if you are using the latest stable, or "recommended,"
  version of Nix (or Lix.) This check has been placed to make it clear we do not
  support legacy/vulnerable versions of Nix, and encourage users to update if
  they have not yet done so.

- NixOS: Nh now accepts the subcommand `nh os build-vm`, which builds a virtual
  machine image activation script instead of a full system. This includes a new
  option `--with-bootloader/-B` that applies to just build-vm, to build a VM
  with a bootloader.

### Changed

- Darwin: Use `darwin-rebuild` directly for activation instead of old scripts
- Darwin: Future-proof handling of `activate-user` script removal
- Darwin: Improve compatibility with root-only activation in newer nix-darwin
  versions
- NixOS: Check if the target hostname matches the running system hostname before
  running `nvd` to compare them.

## 4.0.3

### Added

- Nh now supports specifying `NH_SUDO_ASKPASS` to pass a custom value to
  `SUDO_ASKPASS` in self-elevation. If specified, `sudo` will be called with
  `-A` and the `NH_SUDO_ASKPASS` will be `SUDO_ASKPASS` locally.

### Fixed

- Fix `--configuration` being ignored in `nh home switch`
  ([#262](https://github.com/nix-community/nh/issues/262))

## 4.0.2

### Added

- Add `--json` to `nh search`, which will return results in JSON format. Useful
  for parsing the output of `nh search` with, e.g., jq.

## 4.0.1

### Removed

- NixOS 24.05 is now marked as deprecated, and will emit an error if the search
  command attempts to use it for the channel. While the Elasticsearch backend
  still seems to support 24.05, it is deprecated in Nixpkgs and is actively
  discouraged. Please update your system at your earliest convenience.

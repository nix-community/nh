<!-- markdownlint-disable no-duplicate-heading -->

# NH Changelog

## Unreleased

### Changed

- Nh checks are now more robust in the sense that unnecessary features will not
  be required when the underlying command does not depend on them.

### Fixed

- Nh will now correctly detect non-semver version strings, such as `x.ygit`.
  Instead of failing the check, we now try to normalize the string and simply
  skip the check with a warning.

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

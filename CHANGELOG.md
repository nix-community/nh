<!-- markdownlint-disable no-duplicate-heading -->

# NH Changelog

<!--
This is the NH changelog. It aims to describe changes that occurred within the
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

### Added

### Changed

### Fixed

### 4.4.1

### Fixed

- `nh clean`'s auto-gcroot handling no longer subjects arbitrary indirect
  gcroots (e.g., home-manager's `current-home`, or any other tool's "current
  generation" symlink) to `--keep-since` age-based cleanup. Only roots that
  structurally look like ephemeral `nix build` result symlinks (`result`,
  `result-<name>`) are aged out; anything else resolving into `/nix/store` is
  left untouched unless it becomes orphaned (its target no longer exists),
  matching real Nix's indirect-gcroot semantics. Previously,
  `nh clean all --ask` could delete a live, in-use gcroot on confirmation,
  causing the next `nix store gc` to silently collect an active generation
  ([#722](https://github.com/nix-community/nh/issues/722)).
  - `nh clean` no longer aborts entirely when a gcroot target lives on a
    read-only-mounted Nix store or otherwise can't be write-checked for a known,
    expected reason (`EACCES`, `EROFS`, `EPERM`)..

### Removed

## 4.4.0

### Added

- `--option NAME VALUE` passthrough flag (repeatable) for setting arbitrary Nix
  configuration options per-invocation, e.g. `--option sandbox false`. Forwarded
  to all underlying `nix` commands.
- `--override-input INPUT FLAKE_URL` passthrough flag (repeatable) for
  overriding specific flake inputs without editing `flake.lock`.
- `NH_SSHOPTS` environment variable as the NH-native alias for `NIX_SSHOPTS`.
  `NH_SSHOPTS` takes precedence when both are set.
- `NH_SUDOOPTS` environment variable for passing extra arguments to the `sudo`
  invocation when NH elevates privileges. `NIX_SUDOOPTS` is also accepted for
  nixos-rebuild compatibility, with `NH_SUDOOPTS` taking precedence.
- `NIXOS_NO_CHECK` is now forwarded to `switch-to-configuration` during
  activation, matching nixos-rebuild behaviour.
- `nh search offline <query>` subcommand for offline search using
  [spam-db](https://github.com/feel-co/spam) databases. Requires `-D <path>` (or
  `NH_OFFLINE_DB`) pointing to the database directory.
- `nh search prs <query>` subcommand for searching recent Nixpkgs pull requests
  and showing which Nixpkgs branches merged PRs have reached. Numeric queries
  and `#<number>` fetch that pull request directly.
- `nh search issues <query>` subcommand for searching recent Nixpkgs issues
  while excluding pull requests.
- `nh search prs` reads GitHub authentication from `GH_TOKEN` first, then checks
  `$XDG_STATE_HOME/nh/github-token` for a token file, falling back to
  `~/.local/state/nh/github-token`.
- `nh search --default-search` global option to set default search mode
  (`packages` or `options`) when no subcommand is specified.
- `nh clean --keep-one` preserves all active direnv gcroots regardless of
  `--keep-since`, preventing projects from being collected when they haven't
  been activated recently. Orphaned and broken gcroots are still removed.
- `nh clean --cross-filesystems` / `-x` allows the gcroot scan to cross
  filesystem boundaries. By default the walk stays on the same filesystem as
  `/nix/var/nix/gcroots`.
- `--no-direnv` passed to `nh clean all` will now preserve `.direnv/` paths as
  well.
- Quoted attribute paths like `foo."bar.baz"` are now allowed. Malformed quoted
  attributes return validation errors instead of outright panicking.
- `nh os {boot,build,switch}` now supports diffing on remote machines.
- nh now has an in-house `nix-command` crate, which is used for building typed
  nixos commands. `nix-command` is also
  [published](https://crates.io/crates/nix-command) to crates.io.

### Changed

- **Breaking Change**: `nh search` CLI has been restructured to use subcommands.
  - `--options <query>` is now `nh search options <query>`
  - `--options` flag has been removed
  - `--json` is shared by all search modes, while `--channel`, `--limit`, and
    `--platforms` now appear only on search modes that use them
  - When no subcommand is specified, the default search mode is used
    (configurable via `--default-search`)
- `nh search` now uses search backend version 48 (previously 46) to track the
  current Elasticsearch endpoint.
- gcroot scanning now walks all of `/nix/var/nix/gcroots` recursively instead of
  only the flat `/nix/var/nix/gcroots/auto` directory.
- Orphaned gcroots where entries in `/nix/var/nix/gcroots` whose target symlink
  no longer exists on disk were silently skipped. They are now detected and
  tagged for removal.
- Broken gcroots where symlinks whose `/nix/store` target has already been
  collected were also silently skipped. They are now tagged for removal
  unconditionally, bypassing `--keep-since`.
- `RESULT_REGEX` (`.*result.*`) matched any path containing the word "result",
  including unrelated files. It is replaced by a structural check that only
  matches direct children of `/nix/store`.
- Explicit local flake refs are now checked by nh before Nix runs. Local flake
  refs like `.`, `./foo`, `/path/to/flake` or `path:/some/path` must point at a
  directory containing a `flake.nix`. Directories where a parent directory
  contains a flake are no longer valid.
- Empty/malformed flake refs are now rejected early. Exempli gratia,
  `NH_FLAKE="" nh os switch`, `nh os repl ''`, and `nh os switch '#attr'` no
  longer work.
- `NH_FLAKE` now also does flake feature detection. A command resolving its
  installable from `NH_FLAKE` now also checks for the required Nix experimental
  features.

### Fixed

- Local `run0` elevation now uses `--pty-late`, avoiding terminal ownership
  changes can break subsequent commands.
- `--no-build-output` / `-Q` now forwards `--quiet` to `nix build` instead of
  the unsupported `--no-build-output` flag.
- Generated Nushell completions now mark the `installable` argument as a path
  while keeping nh's own parser compatible with flake references.
- `nh os switch --target-host root@host` no longer wraps the activation in
  `sudo --prompt= --stdin` when the SSH user is already uid 0. The elevation
  decision now probes `id -u` over the established ControlMaster instead of
  reading the local process uid, matching `nixos-rebuild` behaviour and
  unblocking sudo shims that don't accept stdin passwords (e.g.
  `run0-sudo-shim`).
- The regression introduced by the `Subprocess` crate upgrade causing various
  event outputs to be printed incorrectly should now be resolved.
- `nh os info` now batches `nix path-info` calls into a single invocation
  instead of spawning one process per generation. On systems with hundreds of
  generations this reduces runtime from over a minute to a few seconds.
  ([#636](https://github.com/nix-community/nh/issues/636))
  - `nh os info` now uses a direct map lookup when extracting closure sizes from
    `nix path-info` JSON output, replacing an O(N) key scan with an O(1) lookup.
- The `indicatif` spinner shown during remote closure copying has been fixed so
  it renders correctly again. Hooray.
  ([#635](https://github.com/nix-community/nh/pull/635))

### Removed

- `nh` no longer supports `x86_64-darwin`, following Nixpkgs' decision to drop
  support for that platform.

## 4.3.2

### Fixed

- Bumped the version of the search backend from 44 to 46 to fix outdated
  elasticsearch credentials.

## 4.3.1

### Changed

- The host used to select the `nixosConfiguration` now defaults to the
  `--target-host` for remote deployments instead of the local hostname, unless
  the hostname is explicitly specified via the `-H|--hostname` flag.
- `nh os rollback` performance has been improved.

### Fixed

- Nushell completions now properly complete and expand the `installable`
  argument by treating it like a path instead of a string.
- Specialisations now get correctly installed when running nh os {switch, boot}
  with a specialisation selected.
- The environment variable `NH_SHOW_ACTIVATION_LOGS` now supports `1` (and
  more!) as a value again.

### Removed

## 4.3.0

### Changed

- Passing the `--bypass-root-check` flag now elevates if the user is not root
  instead of calling Nix as non-root user.
- A new `nh os build-image` subcommand for building a disk-image variant is now
  available. A variant can be selected using the `--image-variant` flag. All
  variants in the `config.system.build.images` attribute set are supported.
- `--elevation-program` flag was renamed to `--elevation-strategy` with support
  for `'none'` (no elevation) and `'passwordless'` (for remote hosts with
  `NOPASSWD` configured) values. The old flag name remains available as an alias
  for backward compatibility. It may be removed at a later version.
  ([#434](https://github.com/nix-community/nh/issues/434))
  - Multi-program remote elevation support: `sudo`, `doas`, `run0`, and `pkexec`
    are now supported with correct flags for each program
  - Environment variable `NH_ELEVATION_PROGRAM` is still supported for backward
    compatibility (falls back to `NH_ELEVATION_STRATEGY` if set)
- Platform commands (`nh os`, `nh home`, `nh darwin`) now support SSH-based
  remote builds via `--build-host`. The flag now uses proper remote build
  semantics: derivations are copied to the remote host via `nix copy`, built
  remotely, and results are transferred back. This matches `nixos-rebuild`
  behavior, and is significantly more robust than the previous implementation
  where `--build-host` would use Nix's `--builders` flag inefficiently.
  ([#428](https://github.com/nix-community/nh/issues/428),
  [#497](https://github.com/nix-community/nh/pull/497))
  - A new `--no-validate` flag skips pre-activation system validation checks.
    Can also be set via the `NH_NO_VALIDATE` environment variable.
  - Added `NH_REMOTE_CLEANUP` environment variable. When set, NH will attempt to
    terminate remote Nix processes on interrupt (Ctrl+C). Opt-in due to
    fragility.
- Shell argument splitting now uses `shlex` for proper quote handling in complex
  command arguments.
- `nh os info` now supports `--fields` to select which field(s) to display
  ([#375](https://github.com/nix-community/nh/issues/375)).
  - Empty columns are now hidden by default to avoid visual clutter.
  - A new, per-generation "Closure Size" column has been added
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
- `nh os info` now hides empty fields by default, they can be explicitly shown
  via the `--fields` flag.
- `nh completions` now supports [nushell](https://www.nushell.sh/)
- Platform commands (`nh os`, `nh home`, `nh darwin`) now accept a
  `--show-activation-logs` flag that displays activation output at the end.
  While activation output is now hidden to reduce noise _by default_, this flag
  can be used to replicate the activation behaviour from `nixos-rebuild` where
  failing units are displayed at the end.
  - **Breaking change**: Activation output is now hidden by default on Home
    Manager and Darwin. The logs were previously always visible.
  - The flag can be set globally via the `NH_SHOW_ACTIVATION_LOGS` environment
    variable.
  - `nh search` displays a link to the `package.nix` file on the nixpkgs GitHub,
    and also fixes the existing links so that they no longer brokenly point to a
    non-existent file path on Nix flake systems.

### Fixed

- `nh os info` now gracefully handles out-of-sync profiles. When a previous
  switch failed during activation (e.g., a Systemd service failed), the profile
  may be out of sync with `/run/current-system` while in "test mode" via
  `switch-to-configuration test`. NH now warns about this condition and displays
  version info from the running system instead of failing with an error.
- Fixed the whitespace splitting of self-elevated commands so spaces inside
  quotes don't get separated.
- Missing or "invalid" installable references are now handled more gracefully.
  - Previously the error emitted by NH was unhelpful and generic, which
    negatively affected user experience. NH will now _instead_ fall back to
    common installable locations _and_ tell you what exactly is missing in the
    error with instructions.
  - If the fallback directory is a symlink, NH will now resolve it to the
    canonical path for correct flake handling. If the fallback directory is real
    but `flake.nix` inside it is a symlink, NH will resolve the symlink and use
    its parent directory as the flake reference. This matches how
    `nixos-rebuild` handles symlinked flake files.
  - Permission errors and other I/O errors are now surfaced with clear,
    actionable messages.
  - All fallback errors now include a hint to check `man nh` or the GitHub repo
    for more details.
  - NH also refused to handle references that contained hostname as a part of
    the installable such as (`./flake.nix#myHost`) in the past and lead to
    confusing behaviour for those unfamiliar. Such arguments are now normalized
    with a warning if NH can parse them.
- Password caching now works across all remote operations.
- Empty password validation prevents invalid credential caching.
- Direnv caches in [alternative locations][direnv-alternative-caches] (e.g.,
  `$XDG_CACHE_DIR/direnv/layouts`) will now be detected during `nh clean`.
- Fixed `--use-substitutes` being incorrectly passed to `nix build`, causing
  "unrecognised flag" errors.
- nh now properly resolves installables, fixing issues when e.g. multiple
  `NH_{FLAKE,FILE,{OS,HOME_DARWIN}_FLAKE}` environment variables are set.
- For the `--keep-since` flag, the explanation linking to the documentation of
  the `humantime` crate is now shown.
- In the man page, print full available documentation details for each option.

[direnv-alternative-caches]: https://github.com/direnv/direnv/wiki/Customizing-cache-location

### Removed

- Shell completion generation has been moved OUT of the main NH CLI, and is now
  done via `cargo-xtask` in the packaging step. The `nh completions` command is
  now fully deprecated and shell completion can be done with
  `cargo xtask completions` or `cargo xtask dist`.

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

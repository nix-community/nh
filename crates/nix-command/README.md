# nix-command

`nix-command` provides a schema-driven builder API for constructing and running
`nix` subcommands. Each supported command (build, develop, eval, flake, run,
shell) carries predefined defaults via `CommandSpec`, so flags like
`--print-build-logs` and interactive stdio are set correctly without manual
wiring.

## Features

- Each `CommandKind` carries default flags (`--print-build-logs`, interactive
  mode) via `CommandSpec`.
- Chain `.arg()`, `.env()`, `.impure()`, `.interactive()`,
  `.print_build_logs()`, `.with_timeout()` to configure a `NixCommand` via the
  builder API.
- Stdout and stderr are forwarded as they arrive using streaming execution via
  `run_with_logs()`
- Captured execution via `output()`; stdout and stderr are collected into
  buffers.

## Quick start

```rust
use nix_command::{CommandKind, NixCommand};

// Build a command
let cmd = NixCommand::new(CommandKind::Build)
    .arg("nixpkgs#hello")
    .impure(true);

// Inspect the argv
assert_eq!(cmd.argv(), [
    "nix", "build", "--print-build-logs", "--impure", "nixpkgs#hello"
]);

// Run with streaming output
let status = cmd.run_with_logs()?;
assert!(status.success());

// Or capture output
let output = cmd.output()?;
let stdout = String::from_utf8_lossy(&output.stdout);
```

## Supported commands

| Command   | `--print-build-logs` | Interactive |
| --------- | -------------------- | ----------- |
| `build`   | yes                  | no          |
| `develop` | yes                  | yes         |
| `eval`    | no                   | no          |
| `flake`   | no                   | no          |
| `run`     | yes                  | yes         |
| `shell`   | yes                  | yes         |

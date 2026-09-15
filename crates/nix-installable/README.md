# nix-installable

`nix-installable` provides parsing and argument rendering for Nix installables.
It models flake references, Nix files, expressions, and store paths without
environment lookups or NH-specific configuration defaults.

## Features

- Split flake references and attribute paths with `parse_flake_reference()`.
  Query parameters before `#` are preserved in the reference.
- Parse dotted attribute paths with `parse_attribute()`, including quoted
  components containing literal dots. Backslashes are literal, matching Nix's
  attribute-path syntax.
- Render an `Installable` to command-line arguments with `.to_args()`. Non-UTF-8
  paths and attribute components containing double quotes return an error.
- Append an attribute component with `.with_attribute()`, which returns `None`
  for store paths.

## Quick start

```rust
use nix_installable::{Installable, parse_flake_reference};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (reference, attribute) =
        parse_flake_reference("github:NixOS/nixpkgs#hello")?;
    let installable = Installable::Flake { reference, attribute };

    assert_eq!(installable.to_args()?, ["github:NixOS/nixpkgs#hello"]);

    // Pass the arguments to a Nix command builder or std::process::Command.
    let mut command = std::process::Command::new("nix");
    command.arg("build").args(installable.to_args()?);
    Ok(())
}
```

## Supported installables

| Variant      | Arguments                     |
| ------------ | ----------------------------- |
| `Flake`      | `reference#attribute`         |
| `File`       | `--file path attribute`       |
| `Expression` | `--expr expression attribute` |
| `Store`      | `/nix/store/...`              |

Arguments are returned separately for direct use with a command builder; they
are not shell-quoted. Flake references are passed through for Nix to resolve.

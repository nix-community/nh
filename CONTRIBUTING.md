# Contributing to NH

<!--toc:start-->

- [Contributing to NH](#contributing-to-nh)
  - [Code of Conduct](#code-of-conduct)
  - [Making Changes](#making-changes)
    - [Writing Code](#writing-code)
      - [Pre-flight Checklist](#pre-flight-checklist)
  - [Testing](#testing)
    - [Running Tests](#running-tests)
    - [Platform-Specific Testing](#platform-specific-testing)
  - [Submitting Changes](#submitting-changes)
    - [Commit Messages](#commit-messages)
      - [Common Scopes](#common-scopes)
    - [Pull Request Process](#pull-request-process)
  - [Code Style](#code-style)
    - [Rust](#rust)
    - [Error Handling](#error-handling)
    - [Shell Command Quoting](#shell-command-quoting)
    - [Logging](#logging)
    - [Derive Macros](#derive-macros)
  - [AI Policy](#ai-policy)
    - [What This Means](#what-this-means)
  - [Getting Help](#getting-help)
  - [License](#license)

<!--toc:end-->

Thank you for your interest in contributing to NH! It is maintained by
volunteers at no cost to the users, and user contributions are the greatest form
of support we can receive. This document will streamline the contributing
process, provide information to help you contribute effectively, and establish
the guidelines you must be aware of.

## Code of Conduct

This project, and everyone participating within this project or the surrounding
spaces, are governed by our commitment to maintaining a welcoming and respectful
environment. We expect all contributors to:

- Be respectful and constructive in all interactions
- Accept constructive criticism gracefully
- Focus on what is best for the community and the project
- Show empathy towards others

## Making Changes

Before you start, it is _highly advisable_ that you check existing issues and
pull requests to avoid duplicate work. Sometimes certain issues take a little
too long to address while we discuss the specifics, and pull requests are not
immediately visible.

Likewise, for significant changes you should create either an issue or a
discussion to propose your idea. This is not critical, but it would help polish
a feature before it is implemented so that it fits the codebase.

Lastly, for bug fixes, consider referring to the issue number in the PR
discussion to make the issue author aware and help close it automatically once
the fix has been merged.

### Writing Code

[code style guidelines]: #code-style
[changelog]: ./CHANGELOG.md

1. Follow the [code style guidelines]
2. Prefer appropriate error handling patterns (e.g, `color_eyre::Result`) over
   `unwrap()` and `expect()`.
3. Write appropriate Rustdoc for new functions, but keep nested comments
   minimal. Code should be self-documenting where possible.
4. Update the [changelog] with your changes when your work is complete

#### Pre-flight Checklist

We use `just` to orchestrate some common maintenance tasks. The `just` tool is
provided by the default dev shell and may be invoked to either check for or
automatically fix common issues you might forget about. Before committing your
changes, consider running the "fix" task provided by the Justfile located in the
repository root. It will apply the necessary formatting and linting changes
enforced by `rustfmt` and `clippy`:

```bash
# Check for common issues
$ just check

# Fix issues that can be fixed automatically
$ just fix
```

The `fix` task will run:

- Run `cargo fix` for automatic fixes
- Run `cargo clippy --fix` for lint fixes
- Run `cargo fmt` for code formatting
- Run `taplo fmt` for TOML formatting

while the `check` task runs a "dry-run" of sorts to _check_ for issues without
modifying anything. If you wish, you may run those commands manually as well.

## Testing

NH features a _massive_ test suite to verify the accuracy for various bits and
pieces. This helps ensure we do not regress existing features while moving fast.
While contributing, you are expected to write appropriate tests and run the
existing test suite.

### Running Tests

For faster, parallel test execution we use `cargo-nextest`. It is included in
the default Nix devshell.

```bash
# Run all tests
$ cargo nextest run

# Or with plain cargo
$ cargo test

# Or with Just
$ just test
```

### Platform-Specific Testing

If your changes affect platform-specific functionality, please try to build your
project on the relevant platform(s). By design NH supports Linux and Darwin, but
various other Unixes might also work. The following are first-class targets in
NH:

- `x86_64-linux`
- `aarch64-linux`
- `x86_64-darwin`
- `aarch64-darwin`

Please try to test on _at least_ `x86_64-linux` and `aarch64-darwin`. The
`x86_64-darwin` target is set to be phased out with the 26.05 release, as
Nixpkgs aims to drop it. We may try to support it as long as we can if a Darwin
maintainer joins the NH team.

## Submitting Changes

### Commit Messages

Please use clear, descriptive commit messages that adhere to **scoped commits**
format. For example:

```commitmsg
nixos: don't validate `--profile` path locally when targeting remote
```

In this commit, the format is `scope: description`. Some common scopes include
`various`, `treewide`, `chore` or `meta` for common maintenance tasks. In most
cases, it will be `<module-name>: <description>`. You should also strive to make
your commits atomic, and focus each commit on one logical change.

You do not need to reference issues in commit bodies, but you _may_ add
something like `Fixes #XXX` to the long description.

#### Common Scopes

It is _usually_ safe to use the crate or module name for the scope. For example
if you are editing `crates/nh-core/args.rs` you may use `nh-core:` as the scope.
If you are editing multiple modules or crates, `various` or `treewide` may be
more suitable. We use `docs:`, `chore:` and `meta:` for changes around
documentation, basic maintenance (such as dependency updates) and permanent
repository changes respectively.

If in doubt, feel free to ask the maintainers for guidance.

### Pull Request Process

While creating a pull request, you will want to first update the CHANGELOG.md
document that keeps track of changes made to NH. Most changes should target the
"Unreleased" section, and this section will be renamed to the next tag before a
tagged release.

There exists a PR template that helps clarify what we are looking for in a PR.
Please fill it out completely.

After creating a PR, please ensure that all checks pass. At the very least you
**must** ensure that NH builds on supported platforms, and that
formatting/linting checks succeed. You will also want to make sure that there
are no new test failures.

Last but not least, you'll need to wait for maintainer review. If a review takes
too long, i.e., there has been no reviews after a long time you may give the
maintainers a gentle nudge by pinging them in the issue.

## Code Style

### Rust

[Cargo manifest]: ./Cargo.toml

NH uses Rust 2024 edition, with most formatting rules codified in
`.rustfmt.toml`. Naming is standard Rust, where we use:

- `snake_case` for functions and variables
- `PascalCase` for types and enums
- `SCREAMING_SNAKE_CASE` for constants

The various lints enforced by default can be found in the [Cargo manifest]. In
general we do not want lints suppressed, however, `pedantic` and `nursery`
groups for Clippy sometimes provide false positives. In this case you may
annotate your code with `#[expect]` with a `reason` parameter.

> [!TIP]
> `#[expect]` is preferred over `#[allow]` because it will warn if the lint no
> longer fires (e.g., if it was fixed or the code changed). This catches stale
> suppressions that would otherwise go unnoticed.

The lints and formatting rules are enforced by the CI, and can be checked easily
with `just check` as described in [pre-flight checklist](#pre-flight-checklist)
section.

### Error Handling

Use `color_eyre::Result` and `thiserror::Error`:

```rust
use color_eyre::Result;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MyError {
    #[error("something went wrong: {0}")]
    SomethingWentWrong(String),
}
```

Panics, i.e., `panic!`, `unwrap()` and `expect()` are banned throughout the
codebase but they may be used in tests. It is allowed to suppress them within
test fields. As described above, you must explicitly use `#[expect]` with the
`reason` parameter.

### Shell Command Quoting

In general you are expected to use "native" crates where possible rather than
calling shell commands. In some cases there is no alternative to using shell,
and you will need to spawn subprocesses to run shell commands. You must always
use `shlex::try_quote` when constructing shell commands:

```rust
use shlex::try_quote;
let safe_arg = try_quote(user_input)?;
```

### Logging

We use the `tracing` crate for logging throughout NH. Use the appropriate
macros, and ensure hotpaths provide appropriate debugging information via
`debug!`.

```rust
use tracing::{debug, info, warn};

debug!("Detailed debugging information");
info!("General progress information");
warn!("Potential issues that aren't errors");
```

Keep in mind to provide developer-friendly information in `debug!` and `trace!`
level logs, as those will come in handy for pinpointing code issues.

### Derive Macros

Use strong typing with appropriate derives:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MyType {
    // fields
}
```

## AI Policy

> [!IMPORTANT]
> Pull requests created or submitted by autonomous or supervised AI agents are
> explicitly prohibited, and will be immediately closed without a review. NH, as
> a codebase, does not welcome AI-generated contributions.

This policy exists for the following reasons:

1. **Quality Assurance**: AI-generated code often lacks the contextual
   understanding required for systems-level software that interfaces with
   critical system components.

2. **Legal and Licensing**: The project is licensed under EUPL-1.2, which
   requires clear authorship and accountability. AI-generated contributions
   create ambiguity around copyright and licensing obligations. Not to mention
   the ethical concerns.

3. **Security**: NH operates with elevated privileges and manages system
   configurations. Changes to such software require human judgment, security
   awareness, and accountability.

4. **Maintenance Burden**: AI-generated contributions often require
   disproportionate maintainer effort to review, correct, and integrate
   properly.

### What This Means

- **Prohibited**: Submitting PRs where an AI agent (autonomous or supervised)
  generated the code, commit messages, or PR description, regardless of whether
  a human clicked the "submit" button.

- **Prohibited**: Using AI agents to automatically fix issues, respond to review
  comments, or generate follow-up commits.

- **Allowed**: Using AI tools as aids while writing code, provided a human
  author thoroughly reviews, tests, and takes full responsibility for the
  submission. AI-assisted PRs require **FULL DISCLOSURE** and appropriate proof
  that the user thoroughly understands the code generated.

By submitting a pull request, you attest that:

1. You are a human contributor
2. You have personally authored or thoroughly reviewed and tested all changes
3. You take full legal and ethical responsibility for the contribution
4. No autonomous or supervised AI agent was used to create or submit the PR
5. You understand the consequences of violating above guidelines.

Violations of this policy may result in a permanent ban from contributing to the
project.

## Getting Help

[GitHub Discussions]: https://github.com/nix-community/nh/discussions
[GitHub Issues]: https://github.com/nix-community/nh/issues

- **Discussions**: For questions and ideas, use [GitHub Discussions].
- **Issues**: For bug reports and feature requests, use [GitHub Issues].
- **Matrix**: You may find the maintainers, `@NotAShelf` and `@faukah`, in
  various Nix/NixOS community spaces.

## License

[European Union Public License v. 1.2 (EUPL-1.2)]: ./LICENSE

By contributing to NH, you agree that your contributions will be immediately
licensed under [European Union Public License v. 1.2 (EUPL-1.2)] upon a merge.

---

Thank you for contributing to NH! Your efforts help make the Nix ecosystem
better for everyone.

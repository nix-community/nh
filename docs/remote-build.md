# Remote Deployment

NH supports remote deployments, as you might be familiar from `nixos-rebuild` or
similar tools, using the `--build-host` and `--target-host` options.

## Overview

Remote deployment has two independent concepts:

- **`--build-host`**: Where the configuration is **built** (via
  `nix-copy-closure` + `nix build`)
- **`--target-host`**: Where the result is **deployed** and activated

You can use either, both, or neither. Derivation evaluation always happens
locally.

| Flags used                       | Build location | Activation location |
| -------------------------------- | -------------- | ------------------- |
| none                             | localhost      | localhost           |
| `--build-host X`                 | X              | localhost           |
| `--target-host Y`                | localhost      | Y                   |
| `--build-host X --target-host Y` | X              | Y                   |
| `--build-host Y --target-host Y` | Y              | Y                   |

## Basic Usage

### Build Remotely, Deploy Locally

```bash
nh os switch --build-host user@buildserver
```

This builds the configuration on `buildserver`, then copies the result back to
the local machine and activates it.

### Build Locally, Deploy Remotely

```bash
nh os switch --target-host user@production
```

This builds the configuration locally, copies it to `production`, and activates
it there.

### Build on One Host, Deploy to Another

```bash
nh os switch --build-host user@buildserver --target-host user@production
```

This builds on `buildserver`, then copies the result directly to `production`
and activates it there.

### Build and Deploy to the Same Remote Host

```bash
nh os switch --build-host user@production --target-host user@production
```

This builds on `production` and deploys to `production`. The implementation
avoids unnecessary data transfers by detecting when both hosts are the same.

## Host Specification Format

Hosts can be specified in several formats:

- `hostname` - connects as the current user
- `user@hostname` - connects as the specified user
- `ssh://hostname` or `ssh://user@hostname` - URI format (scheme is stripped)
- `ssh-ng://hostname` or `ssh-ng://user@hostname` - Nix store URI format (scheme
  is stripped)
- IPv6 addresses must use bracketed notation: `[2001:db8::1]` or
  `user@[2001:db8::1]`

> [!NOTE]
> Due to restrictions of Nix's SSH remote handling, ports cannot be specified in
> the host string. Use `NIX_SSHOPTS="-p 2222"` or configure ports in
> `~/.ssh/config` for the host you are building on/deploying to.

## SSH Configuration

### Authentication

Remote deployment connects via SSH as the user running `nh`, not as `root`. This
means:

- Your SSH keys and agent are used
- You can use keys with passphrases (via ssh-agent)
- You can use interactive authentication if needed

This differs from `nix build --builders`, which connects via the `nix-daemon`
running as `root`.

### Connection Multiplexing

`nh` uses SSH's `ControlMaster` feature to share connections:

```plaintext
ControlMaster=auto
ControlPath=<runtime-dir>/nh-ssh-%n
ControlPersist=60
```

This reduces overhead when multiple SSH operations are performed. The first SSH
connection to a host creates a master connection, and subsequent operations to
the same host reuse it, avoiding repeated authentication.

SSH control connections are automatically cleaned up when `nh` completes,
ensuring no lingering SSH processes remain.

### Custom SSH Options

Use the `NIX_SSHOPTS` environment variable to pass additional SSH options:

```bash
NIX_SSHOPTS="-p 2222 -i ~/.ssh/custom_key" nh os switch --build-host user@host
```

Options in `NIX_SSHOPTS` are merged with the default options. For persistent
configuration, use `~/.ssh/config`:

```plaintext
Host buildserver
    HostName 192.168.1.100
    Port 2222
    User builder
    IdentityFile ~/.ssh/builder_key
```

Then simply use:

```bash
nh os switch --build-host buildserver
```

## Environment Variables

### NH_REMOTE_CLEANUP

When set, nh will attempt to terminate remote Nix processes when you press
Ctrl+C during a remote build. This uses `pkill` on the remote host to clean up
the build process.

```bash
export NH_REMOTE_CLEANUP=1
nh os switch --build-host user@buildserver
```

Valid values: `1`, `true`, `yes` (case-insensitive).

This feature is **opt-in** because it is inherently fragile - remote process
cleanup depends on SSH still being functional and `pkill` being available. You
may still see zombie processes on the remote host if the connection drops before
cleanup can complete.

### NH_NO_VALIDATE

When set, skips pre-activation system validation checks. Useful when the target
host's store path isn't accessible from the local machine (e.g., building
remotely and deploying to a different target).

```bash
export NH_NO_VALIDATE=1
nh os switch --build-host user@buildserver --target-host user@production
```

## How Remote Builds Work

When you use `--build-host`, `nh` follows this process:

1. **Evaluate** the derivation path locally using
   `nix eval --raw <flake>.drvPath`
2. **Copy derivation** to the build host using `nix-copy-closure --to`
3. **Build remotely** by running `nix build <drv>^* --print-out-paths` on the
   build host
4. **Copy result** back based on the deployment scenario (see below)

### Copy Optimization

To avoid unnecessary network transfers, `nh` optimizes copies based on your
configuration:

<!-- markdownlint-disable MD013 -->

| Scenario                                     | Copy Path                                          |
| -------------------------------------------- | -------------------------------------------------- |
| Build remote, no target                      | `build -> local`                                   |
| Build remote, target = different host        | `build -> target`, `build -> local` (for out-link) |
| Build remote, target = build host            | `(nothing)` (already on target)                    |
| Build remote, target = build host + out-link | `build -> local` (only for out-link)               |

<!-- markdownlint-enable MD013 -->

If `--build-host` and `--target-host` differ, NH will attempt a quick connection
from the build host to the target host to see if it can handle the copy directly
without relaying over localhost. This operation **will not fail the remote build
process**, and NH will simply relay over the orchestrator, i.e., the host you
have ran `nh os build` on. This is implemented as a minor convenience function,
and has zero negative effect over your builds. Instead, it may optimize the
number of connections when all hosts are connected over Tailscale, for example.

When `--build-host` and `--target-host` point to the same machine, the result
stays on that machine unless you need a local out-link (symlink to the build
result).

For security, you are _encouraged to be explicit_ in your hostnames and not
trust the DNS blindly.

## Substitutes

Use `--use-substitutes` to allow remote hosts to fetch pre-built binaries from
binary caches instead of building everything:

```bash
nh os switch --build-host buildserver --use-substitutes
```

This passes:

- `--use-substitutes` to `nix-copy-closure`
- `--substitute-on-destination` to `nix copy` (when copying between two remote
  hosts)

## Build Output

### nix-output-monitor

By default, build output is shown directly. While the NH package is wrapped with
nix-output-monitor, you will need `nix-output-monitor` available on the build
host if you want NH to be able to use it.

If `nix-output-monitor` creates issues for whatever reason, you may disable it
with `--no-nom`.

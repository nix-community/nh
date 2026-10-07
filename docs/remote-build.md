# Remote Deployment

NH supports remote deployments, as you might be familiar from `nixos-rebuild` or
similar tools, using the `--build-host` and `--target-host` options.

## Overview

Remote deployment has two independent concepts:

- **`--build-host`**: Where the configuration is **built** (via
  `nix build --store`)
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

NH supports a few workflows, indicated by a permutation of the flags described
above. If `--target-host` is provided but `--hostname` or `-H` isn't, hostname
will _instead default to the value provided in `--target-host`_. For mDNS
targets, the `.local` domain suffix is stripped when selecting the NixOS
configuration.

For example, if your flake contains a `nixosConfiguration` named `host`, you can
deploy to `user@host.local` without explicitly specifying `--hostname host`.

Below sections describe common workflows.

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

- `hostname` - connects as the current user and uses `ssh-ng://` for Nix store
  copies
- `user@hostname` - connects as the specified user and uses `ssh-ng://` for Nix
  store copies
- `ssh-ng://hostname` or `ssh-ng://user@hostname` - uses the Nix SSH-NG store
  protocol
- `ssh://hostname` or `ssh://user@hostname` - uses the legacy Nix SSH store
  protocol
- IPv6 addresses must use bracketed notation: `[2001:db8::1]` or
  `user@[2001:db8::1]`

> [!NOTE]
> Due to restrictions of Nix's SSH remote handling, ports cannot be specified in
> the host string. Use `NH_SSHOPTS="-p 2222"` (`NIX_SSHOPTS` is also supported
> for nixos-rebuild compatibility) or configure ports in `~/.ssh/config` for the
> host you are building on/deploying to.

## Diffing Remote Systems

When deploying to a `--target-host`, nh diffs the target's current system
against the new one. Closures that are already in the local store are read
locally. Otherwise, nh runs `dix snapshot` on the target host, which requires
[dix](https://github.com/faukah/dix) >= 3.0.0 there:

```nix
environment.systemPackages = [ pkgs.dix ];
```

If dix is missing or too old, nh warns and continues without a diff. Pass
`--diff always` to make this an error, or `--diff never` to skip diffing.

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

Use `NH_SSHOPTS` to pass additional SSH options (or `NIX_SSHOPTS` for
compatibility with nixos-rebuild):

```bash
NH_SSHOPTS="-p 2222 -i ~/.ssh/custom_key" nh os switch --build-host user@host
```

`NH_SSHOPTS` takes precedence over `NIX_SSHOPTS` when both are set. Options are
merged with the default options. For persistent configuration, use
`~/.ssh/config`:

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

1. **Build remotely** by running
   `nix build --store <build-host> --eval-store auto <installable> --print-out-paths`
   locally. Nix evaluates the configuration locally, copies the derivation to
   the build host, builds it there, and streams the logs back. Pressing Ctrl+C
   cancels the remote build.
2. **Copy result** back based on the deployment scenario (see below)

### Copy Optimization

`nh` copies the result to the host that will activate it, and nowhere else:

<!-- markdownlint-disable MD013 -->

| Scenario                              | Copy Path                                        |
| ------------------------------------- | ------------------------------------------------ |
| Build local, target = remote host     | `local -> target`                                |
| Build remote, no target               | `build -> local`                                 |
| Build remote, target = different host | `build -> target`, or `build -> local -> target` |
| Build remote, target = build host     | `(nothing)` (already on target)                  |

<!-- markdownlint-enable MD013 -->

The copy happens right after the build, before `--ask` and the diff, so the diff
can compare against the target host's current system. The local out-link is only
created when the result reaches the local store.

If `--build-host` and `--target-host` differ, NH first tries to copy directly
from the build host to the target host. If that fails, for example because the
hosts can't reach each other, NH relays the copy over the orchestrator, i.e.,
the host you ran `nh os` on.

Remote build products are usually not signed by a binary cache key, so NH passes
`--no-check-sigs` to its `nix copy` invocations. This preserves the behavior
expected from an interactive remote build rather than treating the build host as
a public substituter.

For security, you are _encouraged to be explicit_ in your hostnames and not
trust the DNS blindly.

## Substitutes

Use `--use-substitutes` to allow remote hosts to fetch pre-built binaries from
binary caches instead of building everything:

```bash
nh os switch --build-host buildserver --use-substitutes
```

This passes `--substitute-on-destination` to `nix copy` when the destination is
a remote store, so the remote host can fetch missing paths from its configured
substituters.

## Build Output

### nix-output-monitor

By default, build output is shown directly. While the NH package is wrapped with
nix-output-monitor, you will need `nix-output-monitor` available on the build
host if you want NH to be able to use it.

If `nix-output-monitor` creates issues for whatever reason, you may disable it
with `--no-nom`.

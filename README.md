# cargo-device

A Cargo subcommand for cross-compiling, deploying, and running Rust binaries on embedded Linux devices.

Inspired by Qt Creator's Kit system and Flutter's `--device` flag. Define your target devices once, then switch between desktop and hardware with a single command.

---

## Problem

There is no unified workflow in Rust for:

- Cross-compiling for a target device (Raspberry Pi, industrial boards, and similar SBCs)
- Deploying the compiled binary to the device via SSH/rsync
- Running it with local stdout/stderr
- Syncing additional artifacts (models, configs, assets)
- Sourcing a runtime environment on the device before the binary starts (e.g. ROS 2 `setup.bash`, Yocto SDK activation, or any shell environment that sets `LD_LIBRARY_PATH` / `PATH`)

Developers currently juggle Python scripts, shell scripts, and manual combinations of `cross`, `scp`, and `ssh`. This is error-prone, not reusable, and hard for agents to automate.

---

## System dependencies

`cargo-device` orchestrates existing tools — it does not bundle SSH or file-transfer protocols.

| Tool | Required for | Notes |
|------|-------------|-------|
| `ssh` (OpenSSH) | `run`, `deploy`, `sync` on remote devices | Pre-installed on macOS and most Linux distros |
| `rsync` | `deploy`, `sync` on remote devices | `apt install rsync` / `brew install rsync` |
| `cross` | Build when `cross = true` is set | `cargo install cross` (requires Docker) |
| Cross-linker (e.g. `gcc-aarch64-linux-gnu`) | Build when `linker` is set | Recommended over `cross` — no Docker needed |

`ssh` and `rsync` are detected at runtime; `cargo-device` reports a clear error if a required tool is missing. `cross` (Docker) is opt-in via `cross = true` — the default for a configured `target` is a plain `cargo build` using your local toolchain (`linker`/`sysroot`/`rustflags`/`env`).

## Installation

```bash
cargo install cargo-device
```

## Agent Setup Prompt

Paste this into Cursor, Claude Code, Codex, or another coding agent from the root of the Rust project you want to run on a device:

```text
Set up cargo-device for this Rust project.

Work interactively: ask me one question at a time for any value you cannot infer safely. At minimum, confirm:
- device name (example: raspi)
- Rust target triple (example: aarch64-unknown-linux-gnu)
- build backend: local linker, cross/Docker, or SDK script
- SSH host/user for my machine (keep this out of git)
- deploy path on the device
- binary or package name, if this is a workspace or has multiple binaries

Then do the setup:
1. Install cargo-device with `cargo install cargo-device` if `cargo device --help` is not available.
2. Install or tell me the missing system dependencies for my OS: `ssh`, `rsync`, the Rust target via `rustup target add <target>`, and either a cross-linker such as `gcc-aarch64-linux-gnu`, `cross`, or the SDK path I gave you.
3. Create or update `.cargo/config.toml` with a committed `[device.<name>]` config containing portable values only: target, linker/cross/sdk/sysroot/rustflags/env as needed, deploy_path, sync_dirs if useful, package/binary if needed, and optional run_source.
4. Create or update `.cargo/device.local.toml` with my private machine values such as `ssh_host`, `ssh_key`, local sdk/sysroot paths, or local run_source overrides.
5. Ensure `.cargo/device.local.toml` is listed in `.gitignore`.
6. Show me the final config files with secrets redacted, then run `cargo device list` and the safest verification command: `cargo device build <name>` first, then ask before running `cargo device run <name>`.

Keep the setup simple. Prefer a local linker on Linux when available; use `cross = true` only when a local linker/SDK is not practical. Do not hardcode real IPs, hostnames, SSH keys, or private paths into committed files.
```

---

## Quick Start

```bash
# Build for a device
cargo device build raspi
cargo device build raspi --release
cargo device build raspi --release --bin myapp

# Build, deploy, and run (stdout/stderr streamed locally)
cargo device run raspi
cargo device run raspi --release

# Pass arguments to the binary on the device (after --)
cargo device run raspi --release -- doctor --verbose

# Build and deploy only — no run
cargo device deploy raspi
cargo device deploy raspi --release

# Sync directories only — no build, no run
cargo device sync raspi

# List all configured devices
cargo device list

# Desktop (plain cargo build/run, no deploy)
cargo device build desktop
cargo device run desktop

# Show debug-level log output (SSH command, rsync invocations, etc.)
cargo device -v run raspi
```

Unknown flags and arguments are forwarded directly to `cargo build` / `cargo run`.

For devices that need a runtime environment (e.g. ROS 2), add `run_source` to the device config — `cargo device run` will source those scripts on the device before starting the binary. See [Run-time environment](#run-time-environment).

---

## Configuration

Define devices in `.cargo/config.toml` (committed to git):

```toml
[device.desktop]
# No target = native compilation, no deploy
target = "x86_64-unknown-linux-gnu"

[device.raspi]
target = "aarch64-unknown-linux-gnu" # Raspberry Pi OS 64-bit
linker = "aarch64-linux-gnu-gcc"     # optional: local cross-linker installed on host
# sdk = "/opt/poky/env-setup-..."    # optional: Yocto/Buildroot SDK env script
# sysroot = "~/sysroots/raspi"       # optional: link against this sysroot (--sysroot)
# rustflags = ["-C", "link-arg=-Wl,--allow-shlib-undefined"]  # optional: extra rustflags
# cross = true                       # optional: build via cross (Docker) instead of cargo
ssh_host = "pi@192.168.1.42"         # override this in device.local.toml
ssh_key = "~/.ssh/id_ed25519"
deploy_path = "/tmp/myapp"
sync_dirs = ["models/", "config/"]   # optional: rsync these directories too
package = "myapp"                  # optional: workspace crate for `cargo -p`
binary  = "myapp"                  # optional: deployed binary name (defaults to package)
# optional: source these scripts on the run host before the binary starts (run-time only)
# run_source = ["~/ros2_humble/install/setup.bash", "~/ldlidar_ros2_ws/install/setup.bash"]

# optional: build-time environment variables (visible to build scripts)
[device.raspi.env]
# AMENT_PREFIX_PATH = "~/ros2_libs"
# ROS_DISTRO = "humble"
```

### Build configuration fields

These fields compose — set as many as your toolchain needs:

| Field | Effect |
|-------|--------|
| `target` | Rust target triple; its presence makes the build a cross-compile |
| `linker` | sets `CARGO_TARGET_<T>_LINKER` (the cross-linker binary) |
| `sysroot` | adds `-C link-arg=--sysroot=<path>` (link against the target's libs/glibc, not the host's) |
| `rustflags` | appended to the target-scoped `CARGO_TARGET_<T>_RUSTFLAGS` (does not affect host build scripts) |
| `env` | environment variables for the build process and its build scripts (`[device.<name>.env]` table) |
| `sdk` | source a Yocto/Buildroot `environment-setup` script before building (composes with the fields above) |
| `cross` | `true` → build in Docker via `cross` instead of the local toolchain |

### Run-time environment

| Field | Effect |
|-------|--------|
| `run_source` | scripts to `source` on the **run host** immediately before the binary starts, in order — e.g. `["~/ros2_humble/install/setup.bash", "~/ldlidar_ros2_ws/install/setup.bash"]` |

`run_source` is the run-time counterpart to the build-time `env`/`sdk` fields: those set up the *build*, `run_source` sets up the *run*. The remote command becomes:
```sh
cd <deploy_path> && . <script1> && . <script2> && exec ./<binary> [args]
```

For the `desktop` device the same scripts are sourced in a local shell. Paths are resolved on the **run host**, so a leading `~/` expands to the device user's home — not yours. List them as they exist on the target.

This is the standard approach for embedded Linux runtimes that require environment setup before anything works (ROS 2, Yocto SDK activation, conda environments). Tested on a Radxa Rock 5C (Debian 12) with a ROS 2 Humble binary that links against `rclrs`: without sourcing the binary exits immediately with a dynamic-linker error; with `run_source` it starts clean and all expected env vars (`ROS_DISTRO`, `AMENT_PREFIX_PATH`, `LD_LIBRARY_PATH`) are set.

### Cargo workspaces

For workspace roots without a top-level `[package]`, set `package` (and optionally `binary`) on the device. `cargo device` injects `-p <package>` when you do not pass `-p` yourself.

### Remote binary arguments (`run` only)

With `cargo device run`, pass cargo flags first, then `--`, then arguments for the binary on the device:

```bash
cargo device run raspi --release -- status -v
```

Override per-machine settings in `.cargo/device.local.toml` (add to `.gitignore`):

```toml
# Only the fields listed here override — everything else comes from config.toml
[device.raspi]
ssh_host = "192.168.1.99"
sdk = "/opt/poky/3.4/environment-setup-cortexa72-poky-linux"
```

### .gitignore

```
.cargo/device.local.toml
```

---

## Build Backend

`cargo-device` selects the build backend based on device config, evaluated in order:

| Configuration | Backend |
|---|---|
| `desktop` or no `target` | Plain `cargo build` |
| `cross = true` | `cross build` (Docker); `env`/rustflags via `Cross.toml` `env.passthrough` |
| `sdk` defined | Source SDK env script, then `cargo build` (composing `linker`/`sysroot`/`rustflags`/`env`) |
| `target` defined | `cargo build --target`, applying `linker`, `sysroot`, `rustflags`, `env` |

> **Breaking change (vs. ≤ 0.1):** a configured `target` with no `linker`/`sdk` no longer
> silently falls back to `cross`/Docker. It now runs a plain `cargo build --target` using your
> local toolchain. To keep using Docker, set `cross = true` on the device.

### Recommended: local linker

The fastest and simplest setup — no Docker, no VM, just a cross-linker on your host:

```bash
# Debian / Ubuntu
sudo apt install gcc-aarch64-linux-gnu
rustup target add aarch64-unknown-linux-gnu
```

```toml
[device.raspi]
target = "aarch64-unknown-linux-gnu"
linker = "aarch64-linux-gnu-gcc"
```

`cargo-device` derives `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc` automatically — nothing to configure in `.cargo/config.toml` beyond the two lines above.

`cross` (Docker) is opt-in via `cross = true`, for cases where a host toolchain isn't available.

### Matching the target's glibc (avoid `GLIBC_x.yz not found`)

glibc is **backward- but not forward-compatible**: a binary linked against a newer glibc than
the device has will fail at startup with e.g. `GLIBC_2.39 not found`. A modern host's
`aarch64-linux-gnu-gcc` (Ubuntu 24.04 → glibc 2.39) overshoots an older device (e.g. Debian 12
→ glibc 2.36). Two Docker-free fixes, both expressed with the fields above:

**A — Link against a sysroot copied from the device** (exact glibc; also resolves any C/system
libraries the device has, e.g. ROS 2):

```toml
[device.edge]
target  = "aarch64-unknown-linux-gnu"
linker  = "aarch64-linux-gnu-gcc"       # host apt package
sysroot = "/abs/path/to/edge-sysroot"   # rsync'd from the device (see examples/cross-sysroot)
rustflags = [
    "-C", "link-arg=-Wl,--allow-shlib-undefined",
    # Multiarch host gcc only: put the sysroot's libc first, else --sysroot is overridden by the
    # toolchain's built-in -L and you get GLIBC_x.yz-not-found at runtime. See examples/cross-sysroot.
    "-C", "link-arg=-L/abs/path/to/edge-sysroot/usr/lib/aarch64-linux-gnu",
    "-C", "link-arg=-Wl,-rpath-link,/abs/path/to/edge-sysroot/usr/lib/aarch64-linux-gnu",
]
```

**B — Use a prebuilt toolchain whose bundled glibc is ≤ the device's** (e.g. a
[Bootlin](https://toolchains.bootlin.com/) SDK):

```toml
[device.edge-bootlin]
target  = "aarch64-unknown-linux-gnu"
linker  = "/opt/aarch64--glibc--stable-2021.11-1/bin/aarch64-buildroot-linux-gnu-gcc"
sysroot = "/opt/aarch64--glibc--stable-2021.11-1/aarch64-buildroot-linux-gnu/sysroot"
rustflags = ["-C", "link-arg=-Wl,--allow-shlib-undefined"]
```

See [`examples/cross-sysroot`](examples/cross-sysroot) for a complete, commented walkthrough
(including linking external C libraries such as ROS 2 and verifying the result with `readelf`).
See [Tested configurations](#tested-configurations) for which host/target combinations have been verified.

---

## Tested configurations

The table below shows which host/target/approach combinations have been verified and which
are expected to work but not yet tested. A binary must require glibc ≤ the device's version;
verify with `readelf -V <binary> | grep GLIBC_ | sort -uV | tail`.

| Host OS | Target | Approach / fields used | Status |
|---|---|---|---|
| Ubuntu 24.04 (gcc 13, glibc 2.39) | aarch64 — Debian 12 (glibc 2.36) | `linker` (host `aarch64-linux-gnu-gcc`) + `sysroot` (rsync'd from device) | ✅ Hardware tested |
| Ubuntu 24.04 (gcc 13, glibc 2.39) | aarch64 — Debian 12 (glibc 2.36) | `linker` + `sysroot` (Bootlin SDK `2021.11`, glibc 2.34) | ✅ Hardware tested |
| Ubuntu 24.04 | aarch64 | `cross = true` (Docker) | ✅ Hardware tested |
| Ubuntu 24.04 | x86_64 (same as host) | `desktop` — native build, no target set | ✅ Tested |
| Ubuntu/Debian | aarch64 | `linker` only, **no sysroot** | ⚠️ glibc mismatch if host glibc > device's |
| macOS | aarch64 | `cross = true` (Docker) | 🔲 Expected to work, not tested |
| Any Linux | armv7 / armhf | `cross = true` (Docker) | 🔲 Expected to work, not tested |
| Any Linux | armv7 / armhf | `linker` (`arm-linux-gnueabihf-gcc`) + `sysroot` | 🔲 Expected to work, not tested |

---

## Config Merge

`device.local.toml` overrides `config.toml` field by field. A missing `device.local.toml` is not an error.

Most fields replace wholesale when overridden (`target`, `linker`, `sysroot`, `run_source`, `sync_dirs`, `features`, …). The `env` table is the exception: it merges **per key**, so `config.toml` can hold portable variables (e.g. `ROS_DISTRO`) while `device.local.toml` adds machine-specific ones (e.g. `AMENT_PREFIX_PATH`) without dropping the base.

`run_source` replaces wholesale — if `device.local.toml` defines it, the entire list from `config.toml` is replaced. This is intentional: different machines may need different sourcing paths.

If `ssh_host` is set in `config.toml` but no `device.local.toml` exists, `cargo device` warns:

```
warning: ssh_host defined in .cargo/config.toml — consider creating .cargo/device.local.toml to override for your machine
```

---

## Roadmap

| Milestone | Goal | Status |
|---|---|---|
| M0 — Scaffold | Project structure, agent context, module stubs | done |
| M1 — Core MVP | Config parsing, build backends, deploy, SSH run | done |
| M2 — Polish | `list`, `desktop run`, verbosity control, `run_source` | done |
| M3 — Reliability | Integration tests, GitHub Actions CI, `cross` fallback | planned |

---

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

---

## Contributing

PRs welcome. This project uses [task-master-ai](https://github.com/eyaltoledano/claude-task-master) for task management. See [AGENTS.md](AGENTS.md) for the conventions used by AI coding assistants in this repository.

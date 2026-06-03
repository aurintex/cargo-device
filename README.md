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

Developers currently juggle Python scripts, shell scripts, and manual combinations of `cross`, `scp`, and `ssh`. This is error-prone, not reusable, and hard for agents to automate.

---

## Installation

```bash
cargo install cargo-device
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

# Sync directories only — no build, no run
cargo device sync raspi

# Desktop (plain cargo build/run, no deploy)
cargo device build desktop
cargo device run desktop
```

Unknown flags and arguments are forwarded directly to `cargo build` / `cargo run`.

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
ssh_host = "pi@192.168.1.42"         # override this in device.local.toml
ssh_key = "~/.ssh/id_ed25519"
deploy_path = "/tmp/myapp"
sync_dirs = ["models/", "config/"]   # optional: rsync these directories too
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

`cargo-device` selects the build backend based on device config:

| Configuration | Backend |
|---|---|
| `sdk` defined | Source SDK env script, then `cargo build` |
| `linker` defined (no sdk) | Set `CARGO_TARGET_*_LINKER`, then `cargo build` |
| Neither `sdk` nor `linker` | `cross build` as fallback (requires Docker) |
| `desktop` or no target | Plain `cargo build` |

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

`cross` (Docker) is the fallback for cases where the host toolchain isn't available. It is not the default assumption.

---

## Config Merge

`device.local.toml` overrides `config.toml` field by field. A missing `device.local.toml` is not an error.

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
| M2 — Polish | `list`, `desktop run`, verbosity control | in progress |
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

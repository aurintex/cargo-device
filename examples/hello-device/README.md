# hello-device

Minimal example for [cargo-device](../../README.md).

Prints the architecture, OS, and hostname — so you can verify the binary actually ran on the target hardware.

## Tested on

| Device | SoC | Target |
|---|---|---|
| Radxa Rock 5C | RK3588S (Cortex-A76, aarch64) | `aarch64-unknown-linux-gnu` |

## Prerequisites

Install the cross-linker (Debian/Ubuntu):

```bash
sudo apt install gcc-aarch64-linux-gnu
rustup target add aarch64-unknown-linux-gnu
```

Install `cargo-device`:

```bash
cargo install cargo-device
```

## Setup

1. Copy and edit the local override file (add it to your `.gitignore`):

```bash
cp .cargo/config.toml .cargo/device.local.toml
# Edit device.local.toml — set ssh_host and ssh_key for your machine
```

`.cargo/device.local.toml` example:

```toml
[device.radxa]
ssh_host = "radxa@192.168.1.42"
ssh_key  = "~/.ssh/id_ed25519_radxa"
```

2. Build, deploy, and run in one step:

```bash
cargo device run radxa --release
```

Expected output on a Rock 5C:

```
Hello from cargo-device!
arch:     aarch64
os:       linux
hostname: rock-5c
cpu model	: Rockchip RK3588S
```

## What this tests

- Cross-compilation with a local linker (`aarch64-linux-gnu-gcc`) via `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER`
- Binary deployment via `rsync` over SSH
- Remote execution with streamed stdout

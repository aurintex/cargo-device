# sdk-yocto — Yocto/Buildroot SDK example

Shows how to use the `sdk` field to cross-compile via a Yocto or Buildroot SDK
`environment-setup` script, without Docker.

> **Status:** documented and expected to work; not hardware-tested.
> See [`examples/cross-sysroot`](../cross-sysroot) for verified, hardware-tested configs.

---

## When to use `sdk`

Use `sdk` when your project already uses a Yocto or Buildroot SDK — it is the standard
embedded Linux workflow: run `bitbake populate_sdk` (Yocto) or `make sdk` (Buildroot),
extract the tarball, run the installer, point `sdk` at the resulting `environment-setup-*`
script. The SDK bundles a cross-compiler, sysroot, and all exported variables in one script.

Use `sysroot` + `linker` directly (see [`examples/cross-sysroot`](../cross-sysroot)) when
you do not have a full Yocto/Buildroot SDK and only need to match the device's glibc — for
example by rsyncing the device's `/usr/lib` + `/usr/include`, or using a prebuilt Bootlin SDK.

---

## How `sdk` works

`cargo-device` runs:

```sh
. /path/to/environment-setup-* && cargo build --target <T> ...
```

Everything the SDK exports (`CC`, `AR`, `CFLAGS`, `LDFLAGS`, `PKG_CONFIG_SYSROOT_DIR`, …)
is visible to `cargo build` and to all build scripts (`build.rs`). The `linker`, `sysroot`,
`rustflags`, and `env` fields in the device profile compose on top — they are applied after
the script is sourced.

---

## Setup

1. **Build or install the SDK** from your Yocto/Buildroot project. The script will be at a
   path like:
   - Yocto: `/opt/poky/5.0/environment-setup-cortexa72-poky-linux`
   - Buildroot: `<build>/host/environment-setup`

2. **Add `rustup target`:**
   ```bash
   rustup target add aarch64-unknown-linux-gnu
   ```

3. **Create `.cargo/device.local.toml`** (gitignored):
   ```toml
   [device.edge-yocto]
   sdk      = "/opt/poky/5.0/environment-setup-cortexa72-poky-linux"
   ssh_host = "user@192.168.1.50"
   ssh_key  = "~/.ssh/id_ed25519"
   ```

4. **Build and deploy:**
   ```bash
   cargo device build edge-yocto
   cargo device run edge-yocto
   ```

---

## Verifying glibc compatibility

After building, check the maximum glibc version required:

```bash
readelf -V target/aarch64-unknown-linux-gnu/debug/cross-sysroot \
  | grep GLIBC_ | sort -uV | tail
```

The highest `GLIBC_x.y` must be ≤ the device's glibc version (`ldd --version` on the device).
See the [cargo-device README](../../README.md#tested-configurations) for the full compatibility matrix.

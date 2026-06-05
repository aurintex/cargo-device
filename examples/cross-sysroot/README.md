# Example: Docker-free cross-compilation with a matched glibc

This example documents how to cross-compile for an aarch64 Linux device whose **glibc is older
than your host's**, without `cross`/Docker — including when the binary links external C
libraries such as ROS 2. It is a configuration walkthrough: the actual build needs a real device
(for the sysroot) or a downloaded toolchain, so it is not built in CI.

See [`.cargo/config.toml`](.cargo/config.toml) for the device profiles referenced below.

## The problem: glibc is not forward-compatible

glibc keeps **backward** compatibility (a binary built against glibc 2.34 runs on 2.36, 2.39, …)
but never **forward** compatibility. Link against a newer glibc than the device has and it fails
at startup:

```text
./app: /lib/aarch64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found (required by ./app)
```

A modern host overshoots: Ubuntu 24.04's `aarch64-linux-gnu-gcc` links against **glibc 2.39**,
while a Debian 12 ("bookworm") device has **glibc 2.36**. The fix is to link against a sysroot /
toolchain whose glibc is **≤ the device's**. `cargo-device` expresses this with composable
fields — `linker`, `sysroot`, `rustflags`, `env` — that work for any toolchain.

> Tip: check what a built binary requires with
> `readelf -V target/aarch64-unknown-linux-gnu/debug/app | grep GLIBC` (or
> `objdump -T … | grep GLIBC_`). The highest `GLIBC_x.yz` listed must be ≤ the device's.

---

## Approach A — sysroot copied from the device (`[device.edge]`)

Uses the device's *own* libraries, so the glibc matches exactly and any C libraries already
installed there (ROS 2, OpenCV, …) are available to the linker.

```bash
# 1. Host toolchain (the linker binary only — glibc comes from the sysroot)
sudo apt install gcc-aarch64-linux-gnu rsync
rustup target add aarch64-unknown-linux-gnu

# 2. Build a sysroot from the device. The device needs its -dev packages installed
#    (libc6-dev etc.) so the sysroot has headers, crt*.o and the libc.so linker script.
mkdir -p ~/sysroots/edge
rsync -aL --info=progress2 \
    user@device.local:/lib \
    user@device.local:/usr/lib \
    user@device.local:/usr/include \
    ~/sysroots/edge/
#    (rsync -L dereferences symlinks, sidestepping the broken-absolute-symlink trap. Without
#     -L, relativize absolute symlinks afterwards, e.g. with `symlinks -cr ~/sysroots/edge`.)

# 3. Point the device at it (per-machine path → device.local.toml)
cat >> .cargo/device.local.toml <<'EOF'
[device.edge]
sysroot = "/home/you/sysroots/edge"
# See the caveat below: a multiarch host gcc needs the sysroot's lib dir on the search path first.
rustflags = [
    "-C", "link-arg=-Wl,--allow-shlib-undefined",
    "-C", "link-arg=-L/home/you/sysroots/edge/usr/lib/aarch64-linux-gnu",
    "-C", "link-arg=-Wl,-rpath-link,/home/you/sysroots/edge/usr/lib/aarch64-linux-gnu",
]
ssh_host = "user@device.local"
EOF

# 4. Build — no Docker. ALWAYS verify the linked glibc is <= the device's:
cargo device build edge
readelf -V target/aarch64-unknown-linux-gnu/debug/<bin> | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -3
```

> **Multiarch host-gcc caveat (important).** Debian/Ubuntu's `aarch64-linux-gnu-gcc` has a
> built-in `-L /usr/aarch64-linux-gnu/lib` pointing at the **host's** glibc, and `--sysroot`
> does *not* override it. Without putting the sysroot's own lib dir first via `-L` (and
> `-rpath-link` for transitive deps), the linker silently picks the host glibc and you get
> `GLIBC_2.xx not found` only at runtime on the device. Use absolute paths — `rustflags` reach
> the linker via the target-scoped `CARGO_TARGET_<T>_RUSTFLAGS`, and relative paths would
> resolve against the per-crate compile dir, not your project root. Approach B (a self-contained
> toolchain) sidesteps this entirely.

---

## Approach B — prebuilt toolchain with a fixed, older glibc (`[device.edge-bootlin]`)

When you cannot or do not want to harvest a device sysroot, use a prebuilt toolchain whose
bundled glibc is ≤ the device's. [Bootlin](https://toolchains.bootlin.com/) "stable" SDKs are a
clean, self-contained choice (gcc + binutils + a full sysroot).

`aarch64--glibc--stable-2021.11-1` ships **glibc 2.34** (GCC 10.3) — older than Debian
bookworm's 2.36, so its binaries run on the device. (Bootlin never shipped an aarch64 2.36;
later stable releases jump to 2.35/2.37. The Arm GNU Toolchain bundles 2.37+/2.38 → too new.)

```bash
cd /opt
wget https://toolchains.bootlin.com/downloads/releases/toolchains/aarch64/tarballs/aarch64--glibc--stable-2021.11-1.tar.bz2
tar xjf aarch64--glibc--stable-2021.11-1.tar.bz2
cd aarch64--glibc--stable-2021.11-1 && ./relocate-sdk.sh   # fixes hardcoded paths

# Point the device at the toolchain (per-machine → device.local.toml)
cat >> /path/to/project/.cargo/device.local.toml <<'EOF'
[device.edge-bootlin]
linker  = "/opt/aarch64--glibc--stable-2021.11-1/bin/aarch64-buildroot-linux-gnu-gcc"
sysroot = "/opt/aarch64--glibc--stable-2021.11-1/aarch64-buildroot-linux-gnu/sysroot"
ssh_host = "user@device.local"
EOF

cargo device run edge-bootlin
```

To link external C libraries the SDK does not ship (e.g. ROS 2), add their `-L`/`-rpath-link`
paths to `rustflags` and set any build-script `env` (`AMENT_PREFIX_PATH`, `ROS_DISTRO`).

---

## Approach C — `cross` / Docker fallback (`[device.edge-cross]`)

`cross = true` builds in the cross container, which carries its own old sysroot. Build-script
env vars must be forwarded via `Cross.toml`:

```toml
# Cross.toml
[target.aarch64-unknown-linux-gnu]
env.passthrough = ["AMENT_PREFIX_PATH", "ROS_DISTRO", "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS"]
```

```bash
cargo device build edge-cross
```

---

## Why `--allow-shlib-undefined`?

When you link against a shared library that itself pulls in more shared libraries (e.g.
`librcl.so` → `libtracetools.so` → …), the linker may not be able to resolve every transitive
symbol at link time even though the dynamic loader resolves them fine at runtime on the device.
`-Wl,--allow-shlib-undefined` tells the linker not to fail on those — standard practice when
linking against a plugin-style C library set like ROS 2. Verify the result actually runs on the
device (the env-var forwarding and `-L` paths must be correct).

## Picking an approach

| | Glibc match | Setup | External C libs |
|---|---|---|---|
| **A — device sysroot** | exact | rsync from device | already present in the sysroot |
| **B — Bootlin SDK** | fixed (≤ device) | one download | add `-L`/`-rpath-link` + `env` |
| **C — cross/Docker** | container's | needs Docker | via `Cross.toml` passthrough |

Approach A is the most precise; B is the cleanest when you can't harvest a sysroot; C is the
fallback when no host toolchain is available.

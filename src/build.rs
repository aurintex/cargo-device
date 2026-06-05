//! Build backend selection and invocation.
//!
//! The build is configured by orthogonal, composable fields rather than mutually
//! exclusive modes. Backend priority (evaluated in order):
//! 1. Desktop → plain `cargo build` (no target/deploy needed)
//! 2. `cross = true` → `cross build` (Docker), with `env` and rustflags passed through
//! 3. `sdk` defined → source SDK env script, then `cargo build` (composing linker/rustflags/env)
//! 4. `target` defined → `cargo build --target`, applying `linker`, `sysroot`, `rustflags`, `env`
//! 5. No target → plain `cargo build` (native fallback)
//!
//! Steps 3 and 4 set the target-scoped `CARGO_TARGET_<T>_RUSTFLAGS` (so `--sysroot` does not
//! leak into host build-script/proc-macro compilation) and `CARGO_TARGET_<T>_LINKER`, and apply
//! the `env` map (e.g. `AMENT_PREFIX_PATH`, `ROS_DISTRO`) to the build process and its build scripts.

use crate::device::Device;
use crate::error::ExitStatusExt;
use anyhow::{Context, Result};
use std::process::Command;
use which::which;

/// Run the appropriate build backend for the given device.
pub fn run(device: &Device, extra_args: &[String]) -> Result<()> {
    if device.is_desktop() {
        return run_native(extra_args);
    }
    // `cross`/`sdk` are checked before the no-target fallback so a misconfigured device
    // (e.g. cross set but target missing) gets an error rather than a silent native build.
    if device.cross {
        return run_with_cross(device, extra_args);
    }
    if let Some(sdk) = &device.sdk {
        return run_with_sdk(sdk, device, extra_args);
    }
    if device.build_target().is_none() {
        return run_native(extra_args);
    }
    run_with_cargo(device, extra_args)
}

fn run_native(extra_args: &[String]) -> Result<()> {
    tracing::info!("building (native)");
    Command::new("cargo")
        .arg("build")
        .args(extra_args)
        .status()
        .require_success("cargo build")
}

fn run_with_sdk(sdk: &str, device: &Device, extra_args: &[String]) -> Result<()> {
    let target = device
        .build_target()
        .context("sdk build requires a target to be set in device config")?;
    if !std::path::Path::new(sdk).exists() {
        tracing::warn!(sdk, "SDK env script not found — sourcing may fail");
    }
    tracing::info!(%target, sdk, "building with SDK");
    // Source the SDK env script and build in a single shell invocation so env vars
    // set by the script are visible to cargo. Our composable linker/rustflags/env are
    // applied on top via the process environment (visible after the script is sourced).
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(". \"$SDK_SCRIPT\" && exec cargo build --target \"$TARGET\" \"$@\"")
        .arg("cargo-device-sdk-build")
        .args(extra_args)
        .env("SDK_SCRIPT", sdk)
        .env("TARGET", target);
    apply_build_env(&mut cmd, device, target, IncludeSysroot::Yes);
    cmd.status().require_success("sh -c (sdk build)")
}

fn run_with_cargo(device: &Device, extra_args: &[String]) -> Result<()> {
    let target = device
        .build_target()
        .context("a target is required to cross-compile for this device")?;
    if let Some(linker) = &device.linker {
        which(linker).with_context(|| {
            format!("linker '{linker}' not found in PATH — install the cross-compilation toolchain")
        })?;
    }
    tracing::info!(%target, linker = ?device.linker, "building with cargo (local toolchain)");
    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("--target")
        .arg(target)
        .args(extra_args);
    apply_build_env(&mut cmd, device, target, IncludeSysroot::Yes);
    cmd.status().require_success("cargo build")
}

/// Whether to fold `device.sysroot` into the target rustflags. Off for `cross`, whose
/// container has its own sysroot and where a host path would be meaningless.
#[derive(Clone, Copy, PartialEq)]
enum IncludeSysroot {
    Yes,
    No,
}

/// Apply the composable build configuration (linker, target rustflags, env) to a command.
fn apply_build_env(cmd: &mut Command, device: &Device, target: &str, sysroot: IncludeSysroot) {
    if let Some(linker) = &device.linker {
        cmd.env(linker_env_var(target), linker);
    }
    let sysroot = match sysroot {
        IncludeSysroot::Yes => device.sysroot.as_deref(),
        IncludeSysroot::No => None,
    };
    if let Some(flags) = target_rustflags(sysroot, &device.rustflags) {
        cmd.env(rustflags_env_var(target), flags);
    }
    for (key, value) in &device.env {
        cmd.env(key, value);
    }
}

fn linker_env_var(target: &str) -> String {
    format!(
        "CARGO_TARGET_{}_LINKER",
        target.replace('-', "_").to_uppercase()
    )
}

fn rustflags_env_var(target: &str) -> String {
    format!(
        "CARGO_TARGET_{}_RUSTFLAGS",
        target.replace('-', "_").to_uppercase()
    )
}

/// Build the space-separated value for `CARGO_TARGET_<T>_RUSTFLAGS` from an optional sysroot
/// and extra rustflags. Returns `None` when neither is set. The sysroot is prepended as a
/// `--sysroot` link arg. Cargo splits this env var on spaces, so individual flags must be
/// space-free (true for the link args we emit and for typical device paths).
fn target_rustflags(sysroot: Option<&str>, rustflags: &[String]) -> Option<String> {
    let mut flags: Vec<String> = Vec::new();
    if let Some(sysroot) = sysroot {
        flags.push("-C".to_owned());
        flags.push(format!("link-arg=--sysroot={sysroot}"));
    }
    flags.extend(rustflags.iter().cloned());
    if flags.is_empty() {
        None
    } else {
        Some(flags.join(" "))
    }
}

fn run_with_cross(device: &Device, extra_args: &[String]) -> Result<()> {
    let target = device
        .build_target()
        .context("cross build requires a target to be set in device config")?;
    which("cross").context(
        "`cross = true` is set but `cross` is not installed.\n\
         Install cross: cargo install cross --git https://github.com/cross-rs/cross",
    )?;
    tracing::info!(%target, "building with cross (Docker)");
    let mut cmd = Command::new("cross");
    cmd.arg("build")
        .arg("--target")
        .arg(target)
        .args(extra_args);
    // The container provides its own sysroot, so a host sysroot path is omitted. `env`
    // values and rustflags are set on the host; list them in `Cross.toml` env.passthrough
    // (e.g. `AMENT_PREFIX_PATH`, `ROS_DISTRO`, `CARGO_TARGET_<T>_RUSTFLAGS`) to reach the build.
    apply_build_env(&mut cmd, device, target, IncludeSysroot::No);
    cmd.status().require_success("cross build")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linker_env_var_aarch64() {
        assert_eq!(
            linker_env_var("aarch64-unknown-linux-gnu"),
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER"
        );
    }

    #[test]
    fn linker_env_var_arm_gnueabihf() {
        assert_eq!(
            linker_env_var("arm-unknown-linux-gnueabihf"),
            "CARGO_TARGET_ARM_UNKNOWN_LINUX_GNUEABIHF_LINKER"
        );
    }

    #[test]
    fn linker_env_var_x86_64() {
        assert_eq!(
            linker_env_var("x86_64-unknown-linux-gnu"),
            "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER"
        );
    }

    #[test]
    fn rustflags_env_var_aarch64() {
        assert_eq!(
            rustflags_env_var("aarch64-unknown-linux-gnu"),
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS"
        );
    }

    #[test]
    fn target_rustflags_none_when_empty() {
        assert_eq!(target_rustflags(None, &[]), None);
    }

    #[test]
    fn target_rustflags_sysroot_only() {
        let flags = target_rustflags(Some("/srv/sysroot"), &[]).unwrap();
        assert_eq!(flags, "-C link-arg=--sysroot=/srv/sysroot");
    }

    #[test]
    fn target_rustflags_extra_only() {
        let extra = vec![
            "-C".to_owned(),
            "link-arg=-Wl,--allow-shlib-undefined".to_owned(),
        ];
        let flags = target_rustflags(None, &extra).unwrap();
        assert_eq!(flags, "-C link-arg=-Wl,--allow-shlib-undefined");
    }

    #[test]
    fn target_rustflags_sysroot_prepended_to_extra() {
        let extra = vec![
            "-C".to_owned(),
            "link-arg=-Wl,--allow-shlib-undefined".to_owned(),
        ];
        let flags = target_rustflags(Some("/srv/sysroot"), &extra).unwrap();
        assert_eq!(
            flags,
            "-C link-arg=--sysroot=/srv/sysroot -C link-arg=-Wl,--allow-shlib-undefined"
        );
    }
}

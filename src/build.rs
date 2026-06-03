//! Build backend selection and invocation.
//!
//! Backend priority (evaluated in order):
//! 1. Desktop → plain `cargo build` (no target/deploy needed)
//! 2. `sdk` defined → source SDK env script, then `cargo build`
//! 3. `linker` defined → set `CARGO_TARGET_*_LINKER`, then `cargo build`
//! 4. No target, no sdk, no linker → plain `cargo build` (native fallback)
//! 5. Target set, no sdk/linker → `cross build` (requires Docker)

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
    // sdk/linker checked before the no-target fallback so a misconfigured device
    // (sdk set but target missing) gets an error rather than a silent native build.
    if let Some(sdk) = &device.sdk {
        return run_with_sdk(sdk, device, extra_args);
    }
    if let Some(linker) = &device.linker {
        return run_with_linker(linker, device, extra_args);
    }
    if device.build_target().is_none() {
        return run_native(extra_args);
    }
    run_with_cross(device, extra_args)
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
    // set by the script are visible to cargo.
    let args_str = extra_args.join(" ");
    let sh_cmd = format!(". {sdk} && cargo build --target {target} {args_str}");
    Command::new("sh")
        .arg("-c")
        .arg(&sh_cmd)
        .status()
        .require_success("sh -c (sdk build)")
}

fn run_with_linker(linker: &str, device: &Device, extra_args: &[String]) -> Result<()> {
    let target = device
        .build_target()
        .context("linker build requires a target to be set in device config")?;
    let env_var = format!(
        "CARGO_TARGET_{}_LINKER",
        target.replace('-', "_").to_uppercase()
    );
    tracing::info!(%target, %linker, %env_var, "building with local linker");
    Command::new("cargo")
        .arg("build")
        .arg("--target")
        .arg(target)
        .args(extra_args)
        .env(&env_var, linker)
        .status()
        .require_success("cargo build")
}

fn run_with_cross(device: &Device, extra_args: &[String]) -> Result<()> {
    let target = device
        .build_target()
        .context("cross build requires a target to be set in device config")?;
    which("cross").context(
        "neither sdk nor linker is configured, and `cross` is not installed.\n\
         Install cross: cargo install cross --git https://github.com/cross-rs/cross",
    )?;
    tracing::info!(%target, "building with cross (Docker)");
    Command::new("cross")
        .arg("build")
        .arg("--target")
        .arg(target)
        .args(extra_args)
        .status()
        .require_success("cross build")
}

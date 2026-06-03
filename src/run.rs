//! Remote execution via SSH — streams stdout and stderr locally and forwards the exit code.

use crate::device::Device;
use anyhow::{Context, Result};
use std::process::Command;

/// Execute the binary on the device, streaming its output locally and forwarding the exit code.
/// For desktop devices, re-invokes `cargo run` locally (idempotent with the prior build step).
pub fn execute(device: &Device, binary_name: &str, cargo_args: &[String]) -> Result<()> {
    if device.is_desktop() {
        tracing::info!("running locally (desktop)");
        let mut cmd = Command::new("cargo");
        cmd.args(["run"]).args(cargo_args);
        let status = cmd.status().context("failed to invoke cargo run")?;
        if !status.success() {
            if let Some(code) = status.code() {
                std::process::exit(code);
            }
            anyhow::bail!("local process terminated by signal");
        }
        return Ok(());
    }
    let (host, deploy_path) = device.require_ssh()?;
    tracing::info!(%host, %deploy_path, %binary_name, "executing on device via SSH");
    let mut cmd = Command::new("ssh");
    if let Some(key) = &device.ssh_key {
        cmd.arg("-i").arg(key);
    }
    cmd.arg(host).arg(format!("{deploy_path}/{binary_name}"));
    let status = cmd.status().context("failed to invoke ssh")?;
    if !status.success() {
        if let Some(code) = status.code() {
            std::process::exit(code);
        }
        anyhow::bail!("remote process terminated by signal");
    }
    Ok(())
}

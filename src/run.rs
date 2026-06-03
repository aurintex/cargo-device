//! Remote execution via SSH — streams stdout and stderr locally and forwards the exit code.

use crate::device::Device;
use anyhow::{Context, Result};
use std::process::Command;

/// Execute the deployed binary on the device over SSH and stream its output locally.
/// For desktop devices, local execution is not yet implemented (M1).
pub fn execute(device: &Device, binary_name: &str) -> Result<()> {
    if device.is_desktop() {
        anyhow::bail!("local execution for desktop devices is not yet implemented (M1)");
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

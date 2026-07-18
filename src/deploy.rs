//! Deploy a compiled binary and optional sync directories to a remote device.
//!
//! Uses `rsync` for file transfer via `std::process::Command`.

use crate::device::Device;
use crate::error::ExitStatusExt;
use anyhow::{Context, Result};
use std::process::Command;
use which::which;

/// Copy the compiled binary to the device and sync any configured `sync_dirs`.
/// No-op for desktop devices.
pub fn run(device: &Device, release: bool, binary_name: &str) -> Result<()> {
    if device.is_desktop() {
        return Ok(());
    }
    copy_binary(device, release, binary_name)?;
    sync_dirs(device)
}

/// Copy the compiled binary to `ssh_host:deploy_path` via rsync.
pub fn copy_binary(device: &Device, release: bool, binary_name: &str) -> Result<()> {
    if device.is_desktop() {
        return Ok(());
    }
    ensure_deploy_path(device)?;
    let (host, deploy_path) = device.require_ssh()?;
    which("rsync").context("rsync is not installed — install rsync to use deploy")?;
    let profile = if release { "release" } else { "debug" };
    let local_bin = match &device.target {
        Some(target) => format!("target/{target}/{profile}/{binary_name}"),
        None => format!("target/{profile}/{binary_name}"),
    };
    tracing::info!(binary = %local_bin, %host, %deploy_path, "deploying binary");
    let mut cmd = Command::new("rsync");
    cmd.arg("-avz");
    if let Some(key) = &device.ssh_key {
        cmd.arg("-e").arg(format!("ssh -i {key}"));
    }
    cmd.arg(&local_bin).arg(format!("{host}:{deploy_path}/"));
    cmd.status().require_success("rsync")
}

/// Rsync all `sync_dirs` to the device (if any are configured).
pub fn sync_dirs(device: &Device) -> Result<()> {
    if device.sync_dirs.is_empty() {
        return Ok(());
    }
    ensure_deploy_path(device)?;
    let (host, deploy_path) = device.require_ssh()?;
    which("rsync").context("rsync is not installed — install rsync to use sync")?;
    for dir in &device.sync_dirs {
        let dir_name = dir.trim_end_matches('/');
        let src = format!("{dir_name}/");
        let dst = format!("{host}:{deploy_path}/{dir_name}");
        tracing::info!(%src, %dst, "syncing directory");
        let mut cmd = Command::new("rsync");
        cmd.arg("-avz").arg("--mkpath");
        if let Some(key) = &device.ssh_key {
            cmd.arg("-e").arg(format!("ssh -i {key}"));
        }
        cmd.arg(&src).arg(&dst);
        cmd.status().require_success("rsync")?;
    }
    Ok(())
}

/// Create `deploy_path` on the device when missing (needed before `sync` without a prior binary deploy).
fn ensure_deploy_path(device: &Device) -> Result<()> {
    if device.is_desktop() {
        return Ok(());
    }
    which("ssh").context("ssh is not installed — install OpenSSH to use deploy/run")?;
    let (host, deploy_path) = device.require_ssh()?;
    let mut cmd = Command::new("ssh");
    if let Some(key) = &device.ssh_key {
        cmd.arg("-i").arg(key);
    }
    cmd.arg(host).arg("--").args(["mkdir", "-p", deploy_path]);
    cmd.status().require_success("ssh mkdir")?;
    Ok(())
}

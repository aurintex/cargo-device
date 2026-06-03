//! Resolved device configuration — all fields expanded and validated for a given operation.

use crate::config::Config;
use anyhow::{Context, Result};

/// A fully resolved device, ready to use for build, deploy, or run operations.
#[derive(Debug, Clone)]
pub struct Device {
    pub name: String,
    pub target: Option<String>,
    pub linker: Option<String>,
    /// Path to a Yocto/Buildroot SDK environment-setup script.
    pub sdk: Option<String>,
    pub ssh_host: Option<String>,
    /// Path to SSH private key, with `~` expanded.
    pub ssh_key: Option<String>,
    pub deploy_path: Option<String>,
    pub sync_dirs: Vec<String>,
}

/// Resolve a device by name from the loaded config, expanding paths and validating required fields.
pub fn resolve(cfg: &Config, name: &str) -> Result<Device> {
    let raw = cfg
        .device
        .get(name)
        .with_context(|| format!("device '{name}' not found in .cargo/config.toml"))?;

    Ok(Device {
        name: name.to_owned(),
        target: raw.target.clone(),
        linker: raw.linker.clone(),
        sdk: raw.sdk.clone(),
        ssh_host: raw.ssh_host.clone(),
        ssh_key: raw.ssh_key.as_deref().map(expand_tilde),
        deploy_path: raw.deploy_path.clone(),
        sync_dirs: raw.sync_dirs.clone().unwrap_or_default(),
    })
}

impl Device {
    /// Returns `true` if this is the special `desktop` device (no deployment).
    pub fn is_desktop(&self) -> bool {
        self.name == "desktop"
    }

    /// Validates that fields required for SSH operations are present.
    pub fn require_ssh(&self) -> Result<(&str, &str)> {
        let host = self
            .ssh_host
            .as_deref()
            .context("ssh_host is required for this operation — set it in .cargo/config.toml")?;
        let path = self
            .deploy_path
            .as_deref()
            .context("deploy_path is required for this operation — set it in .cargo/config.toml")?;
        Ok((host, path))
    }

    /// Returns the build target, if set.
    pub fn build_target(&self) -> Option<&str> {
        self.target.as_deref()
    }
}

fn expand_tilde(path: &str) -> String {
    shellexpand::tilde(path).into_owned()
}

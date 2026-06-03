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
    pub package: Option<String>,
    pub binary: Option<String>,
    pub ssh_host: Option<String>,
    /// Path to SSH private key, with `~` expanded.
    pub ssh_key: Option<String>,
    /// Remote deploy path, with `~` expanded.
    pub deploy_path: Option<String>,
    pub sync_dirs: Vec<String>,
}

/// Resolve a device by name from the loaded config, expanding paths and validating required fields.
pub fn resolve(cfg: &Config, name: &str) -> Result<Device> {
    let raw = cfg.device.get(name).with_context(|| {
        format!(
            "device '{name}' not found — add a [device.{name}] table to \
             .cargo/config.toml or .cargo/device.local.toml \
             (run 'cargo device list' to see configured devices)"
        )
    })?;

    Ok(Device {
        name: name.to_owned(),
        target: raw.target.clone(),
        linker: raw.linker.clone(),
        sdk: raw.sdk.clone(),
        package: raw.package.clone(),
        binary: raw.binary.clone(),
        ssh_host: raw.ssh_host.clone(),
        ssh_key: raw.ssh_key.as_deref().map(expand_tilde),
        deploy_path: raw.deploy_path.as_deref().map(expand_tilde),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceConfig};
    use std::collections::HashMap;

    fn make_config(name: &str, dev: DeviceConfig) -> Config {
        let mut map = HashMap::new();
        map.insert(name.to_owned(), dev);
        Config { device: map }
    }

    fn raspi_config() -> Config {
        make_config(
            "raspi",
            DeviceConfig {
                target: Some("aarch64-unknown-linux-gnu".into()),
                ssh_host: Some("raspi.local".into()),
                deploy_path: Some("/home/pi".into()),
                ssh_key: Some("~/.ssh/id_rsa".into()),
                ..Default::default()
            },
        )
    }

    #[test]
    fn resolve_known_device() {
        let cfg = raspi_config();
        let dev = resolve(&cfg, "raspi").unwrap();
        assert_eq!(dev.name, "raspi");
        assert_eq!(dev.target.as_deref(), Some("aarch64-unknown-linux-gnu"));
        assert_eq!(dev.ssh_host.as_deref(), Some("raspi.local"));
    }

    #[test]
    fn resolve_unknown_device_errors_with_name() {
        let cfg = raspi_config();
        let err = resolve(&cfg, "nonexistent").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("nonexistent"),
            "error should mention the device name: {msg}"
        );
    }

    #[test]
    fn is_desktop_true_for_desktop() {
        let cfg = make_config("desktop", DeviceConfig::default());
        let dev = resolve(&cfg, "desktop").unwrap();
        assert!(dev.is_desktop());
    }

    #[test]
    fn is_desktop_false_for_non_desktop() {
        let cfg = raspi_config();
        let dev = resolve(&cfg, "raspi").unwrap();
        assert!(!dev.is_desktop());
    }

    #[test]
    fn require_ssh_ok_when_both_fields_set() {
        let cfg = raspi_config();
        let dev = resolve(&cfg, "raspi").unwrap();
        let (host, path) = dev.require_ssh().unwrap();
        assert_eq!(host, "raspi.local");
        assert_eq!(path, "/home/pi");
    }

    #[test]
    fn require_ssh_errors_when_host_missing() {
        let cfg = make_config(
            "nhost",
            DeviceConfig {
                deploy_path: Some("/home/pi".into()),
                ..Default::default()
            },
        );
        let dev = resolve(&cfg, "nhost").unwrap();
        assert!(dev.require_ssh().is_err());
    }

    #[test]
    fn require_ssh_errors_when_deploy_path_missing() {
        let cfg = make_config(
            "npath",
            DeviceConfig {
                ssh_host: Some("raspi.local".into()),
                ..Default::default()
            },
        );
        let dev = resolve(&cfg, "npath").unwrap();
        assert!(dev.require_ssh().is_err());
    }

    #[test]
    fn tilde_in_ssh_key_is_expanded() {
        let cfg = raspi_config();
        let dev = resolve(&cfg, "raspi").unwrap();
        let key = dev.ssh_key.unwrap();
        assert!(!key.contains('~'), "tilde should be expanded, got: {key}");
        assert!(key.contains("/.ssh/id_rsa"));
    }

    #[test]
    fn tilde_in_deploy_path_is_expanded() {
        let cfg = make_config(
            "raspi",
            DeviceConfig {
                ssh_host: Some("raspi.local".into()),
                deploy_path: Some("~/myapp".into()),
                ..Default::default()
            },
        );
        let dev = resolve(&cfg, "raspi").unwrap();
        let path = dev.deploy_path.unwrap();
        assert!(!path.contains('~'), "tilde should be expanded, got: {path}");
        assert!(path.ends_with("/myapp"));
    }
}

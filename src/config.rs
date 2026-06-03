//! Load and merge device configuration from `.cargo/config.toml` and `.cargo/device.local.toml`.

use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Top-level config file structure, matching `[device.*]` tables.
#[derive(Debug, Deserialize, Default)]
pub struct Config {
    pub device: HashMap<String, DeviceConfig>,
}

/// Per-device configuration as written in the TOML files.
/// All fields are optional to support partial overrides in `device.local.toml`.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct DeviceConfig {
    pub target: Option<String>,
    pub linker: Option<String>,
    pub sdk: Option<String>,
    pub ssh_host: Option<String>,
    pub ssh_key: Option<String>,
    pub deploy_path: Option<String>,
    pub sync_dirs: Option<Vec<String>>,
}

impl DeviceConfig {
    /// Merge `other` on top of `self`, with `other` taking precedence for any defined field.
    pub fn merge(self, other: DeviceConfig) -> DeviceConfig {
        DeviceConfig {
            target: other.target.or(self.target),
            linker: other.linker.or(self.linker),
            sdk: other.sdk.or(self.sdk),
            ssh_host: other.ssh_host.or(self.ssh_host),
            ssh_key: other.ssh_key.or(self.ssh_key),
            deploy_path: other.deploy_path.or(self.deploy_path),
            sync_dirs: other.sync_dirs.or(self.sync_dirs),
        }
    }
}

/// Load config from `.cargo/config.toml`, merge overrides from `.cargo/device.local.toml`.
/// Searches for the `.cargo/` directory starting from the current directory and walking up.
pub fn load() -> Result<Config> {
    let cargo_dir = find_cargo_dir()?;
    let base = load_file(cargo_dir.join("config.toml"))?.unwrap_or_default();
    let local = load_file(cargo_dir.join("device.local.toml"))?;
    let local_present = local.is_some();
    warn_if_ssh_in_base_without_local(&base, local_present);
    Ok(merge_configs(base, local.unwrap_or_default()))
}

/// Read and parse a TOML file as `Config`. Returns `None` if the file does not exist;
/// propagates IO and parse errors for files that do exist.
fn load_file(path: impl AsRef<Path>) -> Result<Option<Config>> {
    let path = path.as_ref();
    match std::fs::read_to_string(path) {
        Ok(content) => toml::from_str(&content)
            .map(Some)
            .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow::anyhow!("failed to read {}: {e}", path.display())),
    }
}

fn merge_configs(base: Config, local: Config) -> Config {
    // Exhaustive destructure: adding a field to Config requires updating this merge.
    let Config {
        device: base_device,
    } = base;
    let Config {
        device: local_device,
    } = local;
    let mut merged = base_device;
    for (name, local_dev) in local_device {
        let entry = merged.entry(name).or_default();
        *entry = std::mem::take(entry).merge(local_dev);
    }
    Config { device: merged }
}

fn warn_if_ssh_in_base_without_local(base: &Config, local_present: bool) {
    if local_present {
        return;
    }
    let has_ssh = base.device.values().any(|d| d.ssh_host.is_some());
    if has_ssh {
        tracing::warn!(
            "ssh_host defined in .cargo/config.toml — consider creating \
             .cargo/device.local.toml to override for your machine"
        );
    }
}

fn find_cargo_dir() -> Result<std::path::PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        let candidate = dir.join(".cargo");
        if candidate.is_dir() {
            return Ok(candidate);
        }
        if !dir.pop() {
            anyhow::bail!("could not find .cargo/ directory in current or any parent directory");
        }
    }
}

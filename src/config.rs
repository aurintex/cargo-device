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
    /// Workspace member crate for `cargo -p` (also default binary name when `binary` is unset).
    pub package: Option<String>,
    /// Deployed binary file name (defaults to `package` when unset).
    pub binary: Option<String>,
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
            package: other.package.or(self.package),
            binary: other.binary.or(self.binary),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_config(target: &str) -> String {
        format!("[device.raspi]\ntarget = \"{target}\"\n")
    }

    #[test]
    fn merge_other_wins_when_set() {
        let base = DeviceConfig {
            ssh_host: Some("base-host".into()),
            target: Some("base-target".into()),
            ..Default::default()
        };
        let other = DeviceConfig {
            ssh_host: Some("other-host".into()),
            ..Default::default()
        };
        let merged = base.merge(other);
        assert_eq!(merged.ssh_host.as_deref(), Some("other-host"));
        assert_eq!(merged.target.as_deref(), Some("base-target"));
    }

    #[test]
    fn merge_base_wins_when_other_is_none() {
        let base = DeviceConfig {
            ssh_host: Some("base-host".into()),
            ..Default::default()
        };
        let other = DeviceConfig::default();
        let merged = base.merge(other);
        assert_eq!(merged.ssh_host.as_deref(), Some("base-host"));
    }

    #[test]
    fn merge_both_none_stays_none() {
        let merged = DeviceConfig::default().merge(DeviceConfig::default());
        assert!(merged.ssh_host.is_none());
        assert!(merged.target.is_none());
    }

    #[test]
    fn merge_package_and_binary() {
        let base = DeviceConfig {
            package: Some("base-pkg".into()),
            binary: Some("base-bin".into()),
            ..Default::default()
        };
        let other = DeviceConfig {
            binary: Some("other-bin".into()),
            ..Default::default()
        };
        let merged = base.merge(other);
        assert_eq!(merged.package.as_deref(), Some("base-pkg"));
        assert_eq!(merged.binary.as_deref(), Some("other-bin"));
    }

    #[test]
    fn load_file_returns_none_for_missing_file() {
        let result = load_file("/this/path/does/not/exist/config.toml").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_file_parses_valid_toml() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "{}", make_config("aarch64-unknown-linux-gnu")).unwrap();
        let config = load_file(f.path()).unwrap().unwrap();
        assert_eq!(
            config.device["raspi"].target.as_deref(),
            Some("aarch64-unknown-linux-gnu")
        );
    }

    #[test]
    fn load_file_errors_on_invalid_toml() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(f, "not valid toml ][").unwrap();
        assert!(load_file(f.path()).is_err());
    }

    #[test]
    fn merge_configs_local_overrides_base() {
        let mut base_map = std::collections::HashMap::new();
        base_map.insert(
            "raspi".to_owned(),
            DeviceConfig {
                ssh_host: Some("base-host".into()),
                target: Some("aarch64-unknown-linux-gnu".into()),
                ..Default::default()
            },
        );
        let mut local_map = std::collections::HashMap::new();
        local_map.insert(
            "raspi".to_owned(),
            DeviceConfig {
                ssh_host: Some("local-host".into()),
                ..Default::default()
            },
        );
        let merged = merge_configs(Config { device: base_map }, Config { device: local_map });
        let dev = &merged.device["raspi"];
        assert_eq!(dev.ssh_host.as_deref(), Some("local-host"));
        assert_eq!(dev.target.as_deref(), Some("aarch64-unknown-linux-gnu"));
    }

    #[test]
    fn merge_configs_local_only_device_is_added() {
        let base = Config::default();
        let mut local_map = std::collections::HashMap::new();
        local_map.insert(
            "extra".to_owned(),
            DeviceConfig {
                ssh_host: Some("extra-host".into()),
                ..Default::default()
            },
        );
        let merged = merge_configs(base, Config { device: local_map });
        assert!(merged.device.contains_key("extra"));
    }
}

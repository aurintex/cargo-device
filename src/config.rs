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
    /// Sysroot to link against (with `~` expanded). Added as `--sysroot=<abs>` to the
    /// target's rustflags so link-time glibc/system libs come from this tree rather than
    /// the host toolchain's bundled sysroot. Key to matching an older target glibc.
    pub sysroot: Option<String>,
    /// Extra rustflags appended to `CARGO_TARGET_<T>_RUSTFLAGS` (target-scoped, so they do
    /// not leak into host build-script/proc-macro compilation). Each entry is one argument,
    /// e.g. `["-C", "link-arg=-Wl,--allow-shlib-undefined"]`.
    pub rustflags: Option<Vec<String>>,
    /// Build-time environment variables (values have `~` expanded). Applied to the build
    /// command and visible to build scripts — e.g. `AMENT_PREFIX_PATH`, `ROS_DISTRO`.
    pub env: Option<HashMap<String, String>>,
    /// Opt in to building via `cross` (Docker). When unset/false, a configured `target`
    /// builds with plain `cargo` using `linker`/`sysroot`/`rustflags`/`env`.
    pub cross: Option<bool>,
    /// Workspace member crate for `cargo -p` (also default binary name when `binary` is unset).
    pub package: Option<String>,
    /// Deployed binary file name (defaults to `package` when unset).
    pub binary: Option<String>,
    pub ssh_host: Option<String>,
    pub ssh_key: Option<String>,
    pub deploy_path: Option<String>,
    pub sync_dirs: Option<Vec<String>>,
    /// Scripts to `source` on the run host immediately before exec'ing the binary
    /// (run-time only — orthogonal to the build-time `env`/`sdk` fields). Paths are
    /// interpreted on the *run host* (the device for SSH runs, the local machine for the
    /// `desktop` device), so `~` is left for that shell to expand and is NOT expanded
    /// locally at resolve time. Listed in source order, e.g. underlay before overlay:
    /// `["~/ros2_humble/install/setup.bash", "~/ldlidar_ros2_ws/install/setup.bash"]`.
    pub run_source: Option<Vec<String>>,
    /// When true, prepend `--no-default-features` unless the flag is already present.
    pub no_default_features: Option<bool>,
    /// Default `--features` list when the CLI omits `--features`.
    pub features: Option<Vec<String>>,
}

impl DeviceConfig {
    /// Merge `other` on top of `self`, with `other` taking precedence for any defined field.
    pub fn merge(self, other: DeviceConfig) -> DeviceConfig {
        DeviceConfig {
            target: other.target.or(self.target),
            linker: other.linker.or(self.linker),
            sdk: other.sdk.or(self.sdk),
            sysroot: other.sysroot.or(self.sysroot),
            rustflags: other.rustflags.or(self.rustflags),
            // `env` merges per key (unlike other fields, which replace wholesale): the base
            // table sets portable vars (e.g. ROS_DISTRO) and the local override adds
            // machine-specific ones (e.g. AMENT_PREFIX_PATH) without dropping the base.
            env: merge_env(self.env, other.env),
            cross: other.cross.or(self.cross),
            package: other.package.or(self.package),
            binary: other.binary.or(self.binary),
            ssh_host: other.ssh_host.or(self.ssh_host),
            ssh_key: other.ssh_key.or(self.ssh_key),
            deploy_path: other.deploy_path.or(self.deploy_path),
            sync_dirs: other.sync_dirs.or(self.sync_dirs),
            run_source: other.run_source.or(self.run_source),
            no_default_features: other.no_default_features.or(self.no_default_features),
            features: other.features.or(self.features),
        }
    }
}

/// Merge two optional env tables per key, with `other` winning on key collisions.
fn merge_env(
    base: Option<HashMap<String, String>>,
    other: Option<HashMap<String, String>>,
) -> Option<HashMap<String, String>> {
    match (base, other) {
        (Some(mut base), Some(other)) => {
            base.extend(other);
            Some(base)
        }
        (base, other) => other.or(base),
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
    fn merge_cross_toolchain_fields() {
        let base = DeviceConfig {
            sysroot: Some("/base/sysroot".into()),
            rustflags: Some(vec!["-C".into(), "base-flag".into()]),
            cross: Some(true),
            ..Default::default()
        };
        let other = DeviceConfig {
            sysroot: Some("/local/sysroot".into()),
            // rustflags + cross unset in `other` → base wins
            ..Default::default()
        };
        let merged = base.merge(other);
        assert_eq!(merged.sysroot.as_deref(), Some("/local/sysroot"));
        assert_eq!(
            merged.rustflags.as_deref(),
            Some(["-C".to_owned(), "base-flag".to_owned()].as_slice())
        );
        assert_eq!(merged.cross, Some(true));
    }

    #[test]
    fn merge_env_combines_per_key() {
        // env merges per key: local adds/overrides keys without dropping base-only keys.
        let mut base_env = HashMap::new();
        base_env.insert("ROS_DISTRO".to_owned(), "humble".to_owned());
        base_env.insert("AMENT_PREFIX_PATH".to_owned(), "/base".to_owned());
        let mut local_env = HashMap::new();
        local_env.insert("AMENT_PREFIX_PATH".to_owned(), "/local".to_owned());
        let base = DeviceConfig {
            env: Some(base_env),
            ..Default::default()
        };
        let other = DeviceConfig {
            env: Some(local_env),
            ..Default::default()
        };
        let merged = base.merge(other).env.unwrap();
        assert_eq!(
            merged.get("AMENT_PREFIX_PATH").map(String::as_str),
            Some("/local"),
            "local overrides on key collision"
        );
        assert_eq!(
            merged.get("ROS_DISTRO").map(String::as_str),
            Some("humble"),
            "base-only keys are preserved"
        );
    }

    #[test]
    fn merge_env_keeps_base_when_local_absent() {
        let mut base_env = HashMap::new();
        base_env.insert("ROS_DISTRO".to_owned(), "humble".to_owned());
        let base = DeviceConfig {
            env: Some(base_env),
            ..Default::default()
        };
        let merged = base.merge(DeviceConfig::default()).env.unwrap();
        assert_eq!(merged.get("ROS_DISTRO").map(String::as_str), Some("humble"));
    }

    #[test]
    fn parse_cross_toolchain_fields_from_toml() {
        let toml = r#"
[device.radxa]
target = "aarch64-unknown-linux-gnu"
linker = "aarch64-linux-gnu-gcc"
sysroot = "~/sysroots/radxa"
rustflags = ["-C", "link-arg=-Wl,--allow-shlib-undefined"]
cross = false

[device.radxa.env]
ROS_DISTRO = "humble"
AMENT_PREFIX_PATH = "~/ros2_libs"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let dev = &cfg.device["radxa"];
        assert_eq!(dev.sysroot.as_deref(), Some("~/sysroots/radxa"));
        assert_eq!(dev.rustflags.as_ref().unwrap().len(), 2);
        assert_eq!(dev.cross, Some(false));
        assert_eq!(
            dev.env
                .as_ref()
                .unwrap()
                .get("ROS_DISTRO")
                .map(String::as_str),
            Some("humble")
        );
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

    #[test]
    fn merge_features_array_is_replaced_not_merged() {
        // `features` (like all Vec fields except `env`) replaces wholesale — only `env` merges per key.
        let base = DeviceConfig {
            features: Some(vec!["base-feat".into(), "shared".into()]),
            ..Default::default()
        };
        let other = DeviceConfig {
            features: Some(vec!["other-feat".into()]),
            ..Default::default()
        };
        let merged = base.merge(other);
        assert_eq!(
            merged.features.as_deref(),
            Some(["other-feat".to_owned()].as_slice()),
            "override features replace base entirely (not appended)"
        );
    }

    #[test]
    fn merge_no_default_features_override() {
        let base = DeviceConfig {
            no_default_features: Some(true),
            ..Default::default()
        };
        let other = DeviceConfig {
            no_default_features: Some(false),
            ..Default::default()
        };
        let merged = base.merge(other);
        assert_eq!(merged.no_default_features, Some(false));
    }

    #[test]
    fn parse_run_source_from_toml() {
        let toml = r#"
[device.radxa]
run_source = ["~/ros2_humble/install/setup.bash", "~/ldlidar_ros2_ws/install/setup.bash"]
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let dev = &cfg.device["radxa"];
        let src = dev.run_source.as_deref().unwrap();
        assert_eq!(src.len(), 2);
        assert_eq!(src[0], "~/ros2_humble/install/setup.bash");
        assert_eq!(src[1], "~/ldlidar_ros2_ws/install/setup.bash");
    }

    #[test]
    fn merge_run_source_replaced_by_local() {
        // run_source, like features/sync_dirs, replaces wholesale when local overrides it.
        let base = DeviceConfig {
            run_source: Some(vec!["~/base/setup.bash".into()]),
            ..Default::default()
        };
        let local = DeviceConfig {
            run_source: Some(vec!["~/local/setup.bash".into(), "~/local/ws.bash".into()]),
            ..Default::default()
        };
        let merged = base.merge(local);
        assert_eq!(
            merged.run_source.as_deref(),
            Some(
                [
                    "~/local/setup.bash".to_owned(),
                    "~/local/ws.bash".to_owned()
                ]
                .as_slice()
            ),
        );
    }

    #[test]
    fn merge_run_source_base_kept_when_local_absent() {
        let base = DeviceConfig {
            run_source: Some(vec!["~/base/setup.bash".into()]),
            ..Default::default()
        };
        let merged = base.merge(DeviceConfig::default());
        assert_eq!(
            merged.run_source.as_deref(),
            Some(["~/base/setup.bash".to_owned()].as_slice()),
        );
    }

    #[test]
    fn parse_empty_env_table() {
        // An empty `[device.x.env]` table must parse as `Some(HashMap::new())`, not `None`,
        // so that merge_env treats it as "explicitly set but empty" rather than "absent".
        let toml = "[device.x]\n[device.x.env]\n";
        let cfg: Config = toml::from_str(toml).unwrap();
        let dev = &cfg.device["x"];
        assert!(
            dev.env.is_some(),
            "empty [device.x.env] should produce Some(empty map), not None"
        );
        assert!(dev.env.as_ref().unwrap().is_empty());
    }
}

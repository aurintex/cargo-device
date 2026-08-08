//! Resolved device configuration — all fields expanded and validated for a given operation.

use crate::config::Config;
use crate::deploy::TransportChoice;
use anyhow::{Context, Result};
use std::collections::HashMap;

/// A fully resolved device, ready to use for build, deploy, or run operations.
#[derive(Debug, Clone)]
pub struct Device {
    pub name: String,
    pub target: Option<String>,
    pub linker: Option<String>,
    /// Path to a Yocto/Buildroot SDK environment-setup script.
    pub sdk: Option<String>,
    /// Sysroot to link against, with `~` expanded.
    pub sysroot: Option<String>,
    /// Extra rustflags appended to the target's `CARGO_TARGET_<T>_RUSTFLAGS`.
    pub rustflags: Vec<String>,
    /// Build-time environment variables, with `~` expanded in values.
    pub env: HashMap<String, String>,
    /// When true, build via `cross` (Docker) instead of plain `cargo`.
    pub cross: bool,
    pub package: Option<String>,
    pub binary: Option<String>,
    pub ssh_host: Option<String>,
    /// Path to SSH private key, with `~` expanded.
    pub ssh_key: Option<String>,
    /// Deploy directory on the device. Deliberately NOT tilde-expanded (unlike the local
    /// `sysroot`/`ssh_key` paths): `~` here is the *device* user's home, expanded by the
    /// remote shell. Expanding it locally would send the host's home path — on Windows
    /// even a `C:\…` path — to the device.
    pub deploy_path: Option<String>,
    pub sync_dirs: Vec<String>,
    /// File-transfer backend preference for deploy/sync.
    pub transport: TransportChoice,
    /// Override for the `ssh` executable, with `~` expanded (a host path).
    pub ssh_program: Option<String>,
    /// Override for the `sftp` executable, with `~` expanded (a host path).
    pub sftp_program: Option<String>,
    /// Scripts to `source` on the run host before exec'ing the binary (run-time only).
    /// Deliberately NOT tilde-expanded (unlike `sysroot`/`ssh_key`/`deploy_path`): these
    /// scripts live on the *run host's* filesystem, where `~` is the remote user's home
    /// (e.g. `/home/radxa`), not the local one. Expansion is left to the run-host shell.
    pub run_source: Vec<String>,
    pub no_default_features: bool,
    pub features: Vec<String>,
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
        sysroot: raw.sysroot.as_deref().map(expand_tilde),
        rustflags: raw.rustflags.clone().unwrap_or_default(),
        env: raw
            .env
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, expand_tilde(&v)))
            .collect(),
        cross: raw.cross.unwrap_or(false),
        package: raw.package.clone(),
        binary: raw.binary.clone(),
        ssh_host: raw.ssh_host.clone(),
        ssh_key: raw.ssh_key.as_deref().map(expand_tilde),
        // Not tilde-expanded: deploy_path and run_source are resolved on the run host.
        deploy_path: raw.deploy_path.clone(),
        sync_dirs: raw.sync_dirs.clone().unwrap_or_default(),
        transport: raw
            .transport
            .as_deref()
            .map(TransportChoice::parse)
            .transpose()
            .with_context(|| format!("invalid transport for device '{name}'"))?
            .unwrap_or_default(),
        ssh_program: raw.ssh_program.as_deref().map(expand_tilde),
        sftp_program: raw.sftp_program.as_deref().map(expand_tilde),
        run_source: raw.run_source.clone().unwrap_or_default(),
        no_default_features: raw.no_default_features.unwrap_or(false),
        features: raw.features.clone().unwrap_or_default(),
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

    /// The `ssh` executable to invoke — the `ssh_program` override, or `ssh` from `PATH`.
    pub fn ssh_cmd(&self) -> &str {
        self.ssh_program.as_deref().unwrap_or("ssh")
    }

    /// The `sftp` executable to invoke — the `sftp_program` override, or `sftp` from `PATH`.
    pub fn sftp_cmd(&self) -> &str {
        self.sftp_program.as_deref().unwrap_or("sftp")
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
    fn tilde_in_sysroot_is_expanded() {
        let cfg = make_config(
            "radxa",
            DeviceConfig {
                sysroot: Some("~/sysroots/radxa".into()),
                ..Default::default()
            },
        );
        let dev = resolve(&cfg, "radxa").unwrap();
        let sysroot = dev.sysroot.unwrap();
        assert!(
            !sysroot.contains('~'),
            "tilde should be expanded: {sysroot}"
        );
        assert!(sysroot.ends_with("/sysroots/radxa"));
    }

    #[test]
    fn tilde_in_env_values_is_expanded_and_keys_preserved() {
        let mut env = HashMap::new();
        env.insert("AMENT_PREFIX_PATH".to_owned(), "~/ros2_libs".to_owned());
        env.insert("ROS_DISTRO".to_owned(), "humble".to_owned());
        let cfg = make_config(
            "radxa",
            DeviceConfig {
                env: Some(env),
                ..Default::default()
            },
        );
        let dev = resolve(&cfg, "radxa").unwrap();
        let ament = &dev.env["AMENT_PREFIX_PATH"];
        assert!(!ament.contains('~'), "tilde should be expanded: {ament}");
        assert!(ament.ends_with("/ros2_libs"));
        assert_eq!(dev.env["ROS_DISTRO"], "humble");
    }

    #[test]
    fn cross_defaults_to_false_and_rustflags_to_empty() {
        let cfg = raspi_config();
        let dev = resolve(&cfg, "raspi").unwrap();
        assert!(!dev.cross);
        assert!(dev.rustflags.is_empty());
        assert!(dev.env.is_empty());
        assert!(dev.sysroot.is_none());
    }

    #[test]
    fn tilde_in_deploy_path_is_not_expanded_locally() {
        // deploy_path lives on the device: `~` must survive resolve() so the remote shell
        // expands it against the device user's home. Expanding it here would ship the
        // host's home directory (on Windows, a `C:\…` path) as a remote path.
        let cfg = make_config(
            "raspi",
            DeviceConfig {
                ssh_host: Some("raspi.local".into()),
                deploy_path: Some("~/myapp".into()),
                ..Default::default()
            },
        );
        let dev = resolve(&cfg, "raspi").unwrap();
        assert_eq!(dev.deploy_path.as_deref(), Some("~/myapp"));
    }

    #[test]
    fn transport_defaults_to_auto_and_parses_from_config() {
        let cfg = raspi_config();
        assert_eq!(
            resolve(&cfg, "raspi").unwrap().transport,
            TransportChoice::Auto
        );

        let cfg = make_config(
            "win",
            DeviceConfig {
                transport: Some("sftp".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            resolve(&cfg, "win").unwrap().transport,
            TransportChoice::Sftp
        );
    }

    #[test]
    fn ssh_and_sftp_programs_default_to_path_lookup() {
        let dev = resolve(&raspi_config(), "raspi").unwrap();
        assert_eq!(dev.ssh_cmd(), "ssh");
        assert_eq!(dev.sftp_cmd(), "sftp");
    }

    #[test]
    fn ssh_and_sftp_program_overrides_are_used_and_tilde_expanded() {
        let cfg = make_config(
            "win",
            DeviceConfig {
                ssh_program: Some("C:/Windows/System32/OpenSSH/ssh.exe".into()),
                sftp_program: Some("~/tools/sftp".into()),
                ..Default::default()
            },
        );
        let dev = resolve(&cfg, "win").unwrap();
        assert_eq!(dev.ssh_cmd(), "C:/Windows/System32/OpenSSH/ssh.exe");
        assert!(
            !dev.sftp_cmd().contains('~'),
            "host paths are tilde-expanded: {}",
            dev.sftp_cmd()
        );
    }

    #[test]
    fn invalid_transport_error_names_the_device() {
        let cfg = make_config(
            "win",
            DeviceConfig {
                transport: Some("carrier-pigeon".into()),
                ..Default::default()
            },
        );
        let err = resolve(&cfg, "win").unwrap_err().to_string();
        assert!(err.contains("win"), "error should name the device: {err}");
    }

    #[test]
    fn run_source_is_not_tilde_expanded() {
        // run_source paths are resolved on the run host, so `~` must survive resolve()
        // untouched (expanding it locally would point at the wrong, local, home).
        let cfg = make_config(
            "radxa",
            DeviceConfig {
                run_source: Some(vec![
                    "~/ros2_humble/install/setup.bash".into(),
                    "/opt/ros/humble/setup.bash".into(),
                ]),
                ..Default::default()
            },
        );
        let dev = resolve(&cfg, "radxa").unwrap();
        assert_eq!(dev.run_source[0], "~/ros2_humble/install/setup.bash");
        assert!(
            dev.run_source[0].contains('~'),
            "tilde must NOT be expanded for run_source: {}",
            dev.run_source[0]
        );
        assert_eq!(dev.run_source[1], "/opt/ros/humble/setup.bash");
    }

    #[test]
    fn resolve_cross_true_from_config() {
        let cfg = make_config(
            "edge",
            DeviceConfig {
                target: Some("aarch64-unknown-linux-gnu".into()),
                cross: Some(true),
                ..Default::default()
            },
        );
        let dev = resolve(&cfg, "edge").unwrap();
        assert!(
            dev.cross,
            "cross: Some(true) in config must resolve to Device.cross == true"
        );
    }
}

//! Binary name resolution, workspace `-p` injection, and cargo vs. remote arg splitting.

use crate::device::Device;
use anyhow::{Context, Result};

/// Split trailing args at the first standalone `--` (cargo args vs. remote binary args).
pub fn split_cargo_and_remote(args: &[String]) -> (Vec<String>, Vec<String>) {
    if let Some(pos) = args.iter().position(|a| a == "--") {
        let cargo = args[..pos].to_vec();
        let remote = args[pos + 1..].to_vec();
        (cargo, remote)
    } else {
        (args.to_vec(), Vec::new())
    }
}

/// Returns true if `cargo_args` already contains `-p` or `--package`.
pub fn has_cargo_package_flag(cargo_args: &[String]) -> bool {
    cargo_args
        .iter()
        .any(|a| a == "-p" || a == "--package" || a.starts_with("--package="))
}

/// Prepend `-p <package>` when the device defines `package` and the flag is absent.
pub fn with_package_flag(device: &Device, cargo_args: Vec<String>) -> Vec<String> {
    let Some(pkg) = device.package.as_deref() else {
        return cargo_args;
    };
    if has_cargo_package_flag(&cargo_args) {
        return cargo_args;
    }
    let mut out = vec!["-p".to_owned(), pkg.to_owned()];
    out.extend(cargo_args);
    out
}

/// Resolve the deployed binary name: `binary` → `package` → root `Cargo.toml` `[package].name`.
pub fn resolve_binary_name(device: &Device) -> Result<String> {
    if let Some(bin) = &device.binary {
        return Ok(bin.clone());
    }
    if let Some(pkg) = &device.package {
        return Ok(pkg.clone());
    }
    read_root_package_name()
}

fn read_root_package_name() -> Result<String> {
    #[derive(serde::Deserialize)]
    struct CargoToml {
        package: Option<Package>,
    }
    #[derive(serde::Deserialize)]
    struct Package {
        name: String,
    }
    let content = std::fs::read_to_string("Cargo.toml")
        .context("failed to read Cargo.toml — run cargo device from the project root")?;
    let parsed: CargoToml = toml::from_str(&content).context("failed to parse Cargo.toml")?;
    parsed.package.map(|p| p.name).context(
        "no binary name: set `binary` or `package` in [device.*] for workspace roots, \
             or run from a single-crate project with [package].name in Cargo.toml",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config::DeviceConfig;
    use crate::device::{resolve, Device};
    use std::collections::HashMap;

    fn device_with_package(pkg: &str, bin: Option<&str>) -> Device {
        let mut map = HashMap::new();
        map.insert(
            "edge".to_owned(),
            DeviceConfig {
                package: Some(pkg.to_owned()),
                binary: bin.map(str::to_owned),
                ..Default::default()
            },
        );
        resolve(&Config { device: map }, "edge").unwrap()
    }

    #[test]
    fn split_at_first_double_dash() {
        let args = vec![
            "-p".into(),
            "myapp".into(),
            "--release".into(),
            "--".into(),
            "status".into(),
            "-v".into(),
        ];
        let (cargo, remote) = split_cargo_and_remote(&args);
        assert_eq!(cargo, vec!["-p", "myapp", "--release"]);
        assert_eq!(remote, vec!["status", "-v"]);
    }

    #[test]
    fn split_without_separator() {
        let args = vec!["--release".into()];
        let (cargo, remote) = split_cargo_and_remote(&args);
        assert_eq!(cargo, vec!["--release"]);
        assert!(remote.is_empty());
    }

    #[test]
    fn package_flag_detected_with_equals_form() {
        assert!(has_cargo_package_flag(&["--package=myapp".into()]));
        assert!(has_cargo_package_flag(&["-p".into(), "myapp".into()]));
        assert!(has_cargo_package_flag(&[
            "--package".into(),
            "myapp".into()
        ]));
        assert!(!has_cargo_package_flag(&["--release".into()]));
    }

    #[test]
    fn inject_package_skipped_for_equals_form() {
        let dev = device_with_package("device-pkg", None);
        let out = with_package_flag(&dev, vec!["--package=myapp".into()]);
        assert_eq!(out, vec!["--package=myapp"], "must not inject duplicate -p");
    }

    #[test]
    fn inject_package_when_missing() {
        let dev = device_with_package("myapp", None);
        let out = with_package_flag(&dev, vec!["--release".into()]);
        assert_eq!(out, vec!["-p", "myapp", "--release"]);
    }

    #[test]
    fn inject_package_skipped_when_present() {
        let dev = device_with_package("myapp", None);
        let out = with_package_flag(&dev, vec!["-p".into(), "other".into()]);
        assert_eq!(out, vec!["-p", "other"]);
    }

    #[test]
    fn resolve_binary_prefers_binary_over_package() {
        let dev = device_with_package("myapp", Some("myapp-bin"));
        assert_eq!(resolve_binary_name(&dev).unwrap(), "myapp-bin");
    }

    #[test]
    fn resolve_binary_falls_back_to_package() {
        let dev = device_with_package("myapp", None);
        assert_eq!(resolve_binary_name(&dev).unwrap(), "myapp");
    }
}

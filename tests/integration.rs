use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn bin() -> Command {
    // Cargo builds the binary before running integration tests;
    // the debug build is always present under CARGO_MANIFEST_DIR.
    Command::new(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/cargo-device"))
}

fn make_cargo_dir(dir: &TempDir) -> std::path::PathBuf {
    let cargo = dir.path().join(".cargo");
    fs::create_dir_all(&cargo).unwrap();
    cargo
}

// ── Help & basic invocation ──────────────────────────────────────────────────

#[test]
fn help_exits_successfully() {
    let status = bin().arg("--help").status().unwrap();
    assert!(status.success());
}

// ── Config loading errors ────────────────────────────────────────────────────

#[test]
fn no_cargo_dir_gives_clear_error() {
    let dir = TempDir::new().unwrap();
    let output = bin()
        .args(["build", "raspi"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(".cargo"),
        "expected .cargo mention in error, got: {stderr}"
    );
}

#[test]
fn invalid_toml_gives_parse_error() {
    let dir = TempDir::new().unwrap();
    let cargo = make_cargo_dir(&dir);
    fs::write(cargo.join("config.toml"), "not valid toml ][").unwrap();

    let output = bin()
        .args(["build", "raspi"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("parse") || stderr.contains("config.toml"),
        "expected parse error mention, got: {stderr}"
    );
}

// ── Device resolution errors ─────────────────────────────────────────────────

#[test]
fn unknown_device_names_it_in_error() {
    let dir = TempDir::new().unwrap();
    let cargo = make_cargo_dir(&dir);
    fs::write(
        cargo.join("config.toml"),
        "[device.raspi]\ntarget = \"aarch64-unknown-linux-gnu\"\n",
    )
    .unwrap();

    let output = bin()
        .args(["build", "nonexistent"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nonexistent"),
        "expected device name in error, got: {stderr}"
    );
}

#[test]
fn missing_device_error_mentions_config_file() {
    let dir = TempDir::new().unwrap();
    let cargo = make_cargo_dir(&dir);
    fs::write(cargo.join("config.toml"), "[device.raspi]\n").unwrap();

    let output = bin()
        .args(["build", "nope"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("config.toml") || stderr.contains("device.local.toml"),
        "expected config file mentioned in error, got: {stderr}"
    );
}

// ── local.toml override does not conflict with missing file ──────────────────

#[test]
fn missing_local_toml_is_not_an_error() {
    let dir = TempDir::new().unwrap();
    let cargo = make_cargo_dir(&dir);
    // Only base config, no device.local.toml — should still load fine
    // (build will fail because raspi has no linker/sdk, not because of missing local file)
    fs::write(cargo.join("config.toml"), "[device.raspi]\n").unwrap();

    let output = bin()
        .args(["build", "raspi"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Will fail (no Cargo.toml to build), but NOT because of missing device.local.toml
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("device.local.toml") || !stderr.contains("not found"),
        "missing device.local.toml should not cause an error, got: {stderr}"
    );
}

//! Deploy a compiled binary and optional sync directories to a remote device.
//!
//! Two file-transfer backends are supported, selected per device with `transport`:
//!
//! - **`rsync`** — delta transfer, the fastest option for large or repeated syncs.
//!   Needs the `rsync` binary on the host (standard on Linux/macOS, absent on Windows).
//! - **`sftp`** — batch mode (`sftp -b -`) over the same SSH connection. `sftp` ships
//!   with the OpenSSH suite, so this works out of the box on Windows.
//!
//! The default, `auto`, prefers `rsync` when it is on `PATH` and falls back to `sftp`.
//! This mirrors how Qt Creator deploys to remote Linux devices: SFTP as the portable
//! baseline, rsync as the opt-in accelerator.
//!
//! Remote directories are created up-front with a single `ssh … mkdir -p` rather than
//! rsync's `--mkpath` (which requires rsync ≥ 3.2.3 and has no sftp equivalent).

use crate::device::Device;
use crate::error::ExitStatusExt;
use crate::remote::remote_path_token;
use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use which::which;

/// The concrete file-transfer backend used for a deploy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// `rsync` over SSH.
    Rsync,
    /// `sftp` in batch mode — part of OpenSSH, available on Windows.
    Sftp,
}

/// Configured transport preference (`transport = "auto" | "rsync" | "sftp"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportChoice {
    /// Prefer `rsync`, fall back to `sftp` when `rsync` is not installed.
    #[default]
    Auto,
    /// Always use `rsync`; error if it is missing.
    Rsync,
    /// Always use `sftp`; error if it is missing.
    Sftp,
}

impl TransportChoice {
    /// Parse the `transport` config value. Unknown values are an error naming the valid ones.
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "rsync" => Ok(Self::Rsync),
            "sftp" => Ok(Self::Sftp),
            other => anyhow::bail!(
                "unknown transport '{other}' — valid values are \"auto\", \"rsync\", \"sftp\""
            ),
        }
    }

    /// Resolve to a concrete [`Transport`], verifying the required tool is available.
    /// `sftp_cmd` is the device's `sftp` executable (see `Device::sftp_cmd`).
    pub fn select(self, sftp_cmd: &str) -> Result<Transport> {
        let transport = match self {
            Self::Rsync => {
                which("rsync").context(
                    "transport = \"rsync\" is set but `rsync` is not installed — \
                     install rsync, or use transport = \"sftp\" (ships with OpenSSH)",
                )?;
                Transport::Rsync
            }
            Self::Sftp => {
                which(sftp_cmd).with_context(|| {
                    format!(
                        "transport = \"sftp\" is set but `{sftp_cmd}` was not found — \
                         install the OpenSSH client suite, or set sftp_program"
                    )
                })?;
                Transport::Sftp
            }
            Self::Auto if which("rsync").is_ok() => Transport::Rsync,
            Self::Auto => {
                which(sftp_cmd).with_context(|| {
                    format!(
                        "neither `rsync` nor `{sftp_cmd}` found — install one of them \
                         (`sftp` is part of the OpenSSH client suite)"
                    )
                })?;
                Transport::Sftp
            }
        };
        tracing::debug!(?transport, "selected file transport");
        Ok(transport)
    }
}

/// Copy the compiled binary to the device and sync any configured `sync_dirs`.
/// No-op for desktop devices.
pub fn run(device: &Device, release: bool, binary_name: &str) -> Result<()> {
    if device.is_desktop() {
        return Ok(());
    }
    let transport = device.transport.select(device.sftp_cmd())?;
    let (host, deploy_path) = device.require_ssh()?;
    let plan = plan_transfer(device, deploy_path, transport)?;
    ensure_remote_dirs(device, &plan.dirs)?;
    copy_binary(device, transport, release, binary_name)?;
    transfer_sync_dirs(device, transport, host, deploy_path, &plan)
}

/// Sync all `sync_dirs` to the device (if any are configured), without building or deploying.
pub fn sync_dirs(device: &Device) -> Result<()> {
    if device.sync_dirs.is_empty() {
        return Ok(());
    }
    let transport = device.transport.select(device.sftp_cmd())?;
    let (host, deploy_path) = device.require_ssh()?;
    let plan = plan_transfer(device, deploy_path, transport)?;
    ensure_remote_dirs(device, &plan.dirs)?;
    transfer_sync_dirs(device, transport, host, deploy_path, &plan)
}

/// Copy the compiled binary into `deploy_path` on the device.
fn copy_binary(
    device: &Device,
    transport: Transport,
    release: bool,
    binary_name: &str,
) -> Result<()> {
    let (host, deploy_path) = device.require_ssh()?;
    let local_bin = local_binary_path(device.target.as_deref(), release, binary_name);
    tracing::info!(binary = %local_bin, %host, %deploy_path, ?transport, "deploying binary");
    match transport {
        Transport::Rsync => rsync(device, &local_bin, &format!("{host}:{deploy_path}/")),
        Transport::Sftp => {
            let remote = format!("{}/{binary_name}", sftp_path(deploy_path));
            sftp_batch(device, host, &sftp_binary_lines(&local_bin, &remote))
        }
    }
}

/// sftp batch for the binary: upload, then restore the executable bit.
///
/// Unlike `rsync -a`, the SFTP protocol's `put` does not carry permissions over — the file
/// lands with the server's default mode and the device refuses to exec it. `rsync`'s
/// archive mode already preserves the mode, so this is sftp-only.
fn sftp_binary_lines(local: &str, remote: &str) -> Vec<String> {
    vec![
        format!("put {} {}", sftp_quote(local), sftp_quote(remote)),
        format!("chmod 755 {}", sftp_quote(remote)),
    ]
}

/// Path of the built binary inside `target/`, relative to the project root.
fn local_binary_path(target: Option<&str>, release: bool, binary_name: &str) -> String {
    let profile = if release { "release" } else { "debug" };
    match target {
        Some(target) => format!("target/{target}/{profile}/{binary_name}"),
        None => format!("target/{profile}/{binary_name}"),
    }
}

/// What a deploy has to create and upload, resolved before anything touches the device.
#[derive(Debug, Default, PartialEq, Eq)]
struct TransferPlan {
    /// Remote directories to create, parents before children.
    dirs: Vec<String>,
    /// `(local, remote)` file pairs. Only filled for the sftp transport, which uploads
    /// file by file; rsync walks the tree itself.
    files: Vec<(String, String)>,
}

/// Resolve the transfer plan: the deploy path, every `sync_dirs` root, and — for sftp —
/// the full recursive contents of those roots.
///
/// Collecting *all* directories here means one `ssh … mkdir -p` creates them, so the sftp
/// batch is pure `put`. Letting sftp create them instead would print a `remote mkdir …:
/// Failure` line for every directory that already exists on a re-deploy.
fn plan_transfer(device: &Device, deploy_path: &str, transport: Transport) -> Result<TransferPlan> {
    let mut plan = TransferPlan {
        dirs: vec![deploy_path.to_owned()],
        files: Vec::new(),
    };
    for dir in &device.sync_dirs {
        let dir_name = dir.trim_end_matches('/');
        let remote = format!("{deploy_path}/{dir_name}");
        plan.dirs.push(remote.clone());
        if transport == Transport::Sftp {
            collect_entries(Path::new(dir_name), &remote, &mut plan)?;
        }
    }
    Ok(plan)
}

/// Walk `local` and record its subdirectories and files against the remote path `remote`.
/// This mirrors `rsync -a <dir>/ <remote>`: the *contents* land in `remote`.
fn collect_entries(local: &Path, remote: &str, plan: &mut TransferPlan) -> Result<()> {
    let read_dir = std::fs::read_dir(local)
        .with_context(|| format!("failed to read sync directory {}", local.display()))?;
    let mut entries = read_dir
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to list {}", local.display()))?;
    // Deterministic order: parents before children, stable across platforms.
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_remote = format!("{remote}/{name}");
        let path = entry.path();
        if path.is_dir() {
            plan.dirs.push(child_remote.clone());
            collect_entries(&path, &child_remote, plan)?;
        } else {
            plan.files.push((local_path_arg(&path), child_remote));
        }
    }
    Ok(())
}

/// Transfer every configured `sync_dirs` entry into `deploy_path`.
fn transfer_sync_dirs(
    device: &Device,
    transport: Transport,
    host: &str,
    deploy_path: &str,
    plan: &TransferPlan,
) -> Result<()> {
    match transport {
        Transport::Rsync => {
            for dir in &device.sync_dirs {
                let dir_name = dir.trim_end_matches('/');
                let dst = format!("{deploy_path}/{dir_name}");
                tracing::info!(src = %dir_name, %dst, "syncing directory (rsync)");
                // Trailing slash on the source: copy the *contents* into dst.
                rsync(device, &format!("{dir_name}/"), &format!("{host}:{dst}"))?;
            }
        }
        Transport::Sftp => {
            if plan.files.is_empty() {
                return Ok(());
            }
            tracing::info!(files = plan.files.len(), "syncing directories (sftp)");
            sftp_batch(device, host, &sftp_put_lines(plan))?;
        }
    }
    Ok(())
}

/// Create the given directories on the device with a single `ssh … mkdir -p`.
fn ensure_remote_dirs(device: &Device, dirs: &[String]) -> Result<()> {
    if dirs.is_empty() {
        return Ok(());
    }
    let ssh = device.ssh_cmd();
    which(ssh).with_context(|| {
        format!("`{ssh}` not found — install the OpenSSH client, or set ssh_program")
    })?;
    let (host, _) = device.require_ssh()?;
    let mut cmd = Command::new(ssh);
    if let Some(key) = &device.ssh_key {
        cmd.arg("-i").arg(key);
    }
    cmd.arg(host).arg("--").arg(mkdir_command(dirs));
    cmd.status().require_success("ssh mkdir")
}

/// Build the remote `mkdir -p …` command line. Paths are tokenised for the remote shell,
/// so a leading `~/` expands against the *device* user's home.
fn mkdir_command(dirs: &[String]) -> String {
    let args = dirs
        .iter()
        .map(|dir| remote_path_token(dir))
        .collect::<Vec<_>>()
        .join(" ");
    format!("mkdir -p {args}")
}

// ── rsync backend ────────────────────────────────────────────────────────────────────

fn rsync(device: &Device, src: &str, dst: &str) -> Result<()> {
    let mut cmd = Command::new("rsync");
    cmd.arg("-avz");
    if let Some(rsh) = rsync_rsh(device.ssh_cmd(), device.ssh_key.as_deref()) {
        cmd.arg("-e").arg(rsh);
    }
    cmd.arg(src).arg(dst);
    cmd.status().require_success("rsync")
}

/// Build the `-e` (remote shell) value for rsync, or `None` when rsync's own default
/// (`ssh` from `PATH`, no key) already does the right thing.
fn rsync_rsh(ssh: &str, key: Option<&str>) -> Option<String> {
    if ssh == "ssh" && key.is_none() {
        return None;
    }
    let mut parts = vec![rsh_word(ssh)];
    if let Some(key) = key {
        parts.push("-i".to_owned());
        parts.push(rsh_word(key));
    }
    Some(parts.join(" "))
}

/// Normalise one word of the `-e` string. rsync re-tokenises the value itself, so Windows
/// paths need their backslashes (escape characters to rsync) turned into forward slashes,
/// and anything containing a space has to be quoted. Space-free POSIX paths pass through
/// unchanged.
fn rsh_word(word: &str) -> String {
    let word = word.replace('\\', "/");
    if word.contains(' ') {
        format!("\"{word}\"")
    } else {
        word
    }
}

// ── sftp backend ─────────────────────────────────────────────────────────────────────

/// Run a batch of sftp commands over one connection (`sftp -b -`).
fn sftp_batch(device: &Device, host: &str, lines: &[String]) -> Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let sftp = device.sftp_cmd();
    which(sftp).with_context(|| {
        format!("`{sftp}` not found — install the OpenSSH client suite, or set sftp_program")
    })?;
    let mut cmd = Command::new(sftp);
    cmd.arg("-b").arg("-");
    if let Some(key) = &device.ssh_key {
        cmd.arg("-i").arg(key);
    }
    cmd.arg(host).stdin(Stdio::piped());
    let script = format!("{}\n", lines.join("\n"));
    tracing::debug!(commands = lines.len(), "running sftp batch");
    tracing::trace!(%script, "sftp batch script");

    let mut child = cmd.spawn().context("failed to run `sftp`")?;
    // Taking stdin here also closes it at the end of this statement, which sftp needs
    // as the end-of-batch signal before it will exit.
    child
        .stdin
        .take()
        .context("failed to open stdin for `sftp`")?
        .write_all(script.as_bytes())
        .context("failed to send the batch script to `sftp`")?;
    let status = child.wait().context("failed to wait for `sftp`")?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("`sftp` exited with {status}"))
    }
}

/// One `put` per planned file. Uploading file by file (rather than `put -r`) keeps
/// rsync's trailing-slash "contents into dst" semantics, which `put -r` does not share.
/// The directories are already in place — see [`plan_transfer`].
fn sftp_put_lines(plan: &TransferPlan) -> Vec<String> {
    plan.files
        .iter()
        .map(|(local, remote)| {
            format!(
                "put {} {}",
                sftp_quote(local),
                sftp_quote(&sftp_path(remote))
            )
        })
        .collect()
}

/// Render a local path for an sftp `put`. sftp's own tokeniser treats `\` as an escape,
/// so Windows separators have to become forward slashes.
fn local_path_arg(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Quote a path for sftp's batch parser.
fn sftp_quote(path: &str) -> String {
    format!("\"{}\"", path.replace('\\', "/").replace('"', "\\\""))
}

/// Adapt a remote path for sftp, which has no shell to expand `~`. An sftp session
/// starts in the login user's home, so a leading `~/` becomes a plain relative path.
fn sftp_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        rest.to_owned()
    } else if path == "~" {
        ".".to_owned()
    } else {
        path.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, DeviceConfig};
    use std::collections::HashMap;

    fn device_with(sync_dirs: &[&str], deploy_path: &str) -> Device {
        let mut map = HashMap::new();
        map.insert(
            "edge".to_owned(),
            DeviceConfig {
                ssh_host: Some("user@host".into()),
                deploy_path: Some(deploy_path.to_owned()),
                sync_dirs: Some(sync_dirs.iter().map(|s| (*s).to_owned()).collect()),
                ..Default::default()
            },
        );
        crate::device::resolve(&Config { device: map }, "edge").unwrap()
    }

    // ── transport parsing / selection ────────────────────────────────────────────

    #[test]
    fn transport_choice_parses_known_values() {
        assert_eq!(
            TransportChoice::parse("auto").unwrap(),
            TransportChoice::Auto
        );
        assert_eq!(
            TransportChoice::parse("rsync").unwrap(),
            TransportChoice::Rsync
        );
        assert_eq!(
            TransportChoice::parse("sftp").unwrap(),
            TransportChoice::Sftp
        );
    }

    #[test]
    fn transport_choice_rejects_unknown_value_and_lists_valid_ones() {
        let err = TransportChoice::parse("scp").unwrap_err().to_string();
        assert!(
            err.contains("scp"),
            "error should quote the bad value: {err}"
        );
        assert!(err.contains("rsync") && err.contains("sftp"), "{err}");
    }

    #[test]
    fn transport_choice_defaults_to_auto() {
        assert_eq!(TransportChoice::default(), TransportChoice::Auto);
    }

    // ── remote directory preparation ─────────────────────────────────────────────

    #[test]
    fn rsync_plan_covers_deploy_path_and_each_sync_dir_without_walking() {
        // rsync recurses itself, so the plan must not touch the filesystem — these
        // directories do not exist.
        let dev = device_with(&["models/", "config"], "/opt/app");
        let plan = plan_transfer(&dev, "/opt/app", Transport::Rsync).unwrap();
        assert_eq!(
            plan.dirs,
            ["/opt/app", "/opt/app/models", "/opt/app/config"]
        );
        assert!(plan.files.is_empty());
    }

    #[test]
    fn sftp_plan_collects_nested_dirs_and_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("b.txt"), "b").unwrap();
        std::fs::create_dir(root.path().join("nested")).unwrap();
        std::fs::write(root.path().join("nested/a.txt"), "a").unwrap();

        let mut plan = TransferPlan::default();
        collect_entries(root.path(), "/opt/app/data", &mut plan).unwrap();

        // Nested directories are planned for the ssh mkdir, not left to sftp.
        assert_eq!(plan.dirs, ["/opt/app/data/nested"]);
        let remotes: Vec<_> = plan.files.iter().map(|(_, r)| r.as_str()).collect();
        assert_eq!(
            remotes,
            ["/opt/app/data/b.txt", "/opt/app/data/nested/a.txt"],
            "entries are sorted, parents before children"
        );
    }

    #[test]
    fn sftp_put_lines_never_emit_backslashes() {
        // The batch parser treats `\` as an escape — a Windows path must not leak through.
        let plan = TransferPlan {
            dirs: Vec::new(),
            files: vec![(
                "assets\\sub\\f.bin".to_owned(),
                "~/app/assets/sub/f.bin".to_owned(),
            )],
        };
        let lines = sftp_put_lines(&plan);
        assert_eq!(lines, ["put \"assets/sub/f.bin\" \"app/assets/sub/f.bin\""]);
    }

    #[test]
    fn collect_entries_errors_on_missing_directory() {
        let mut plan = TransferPlan::default();
        let err = collect_entries(Path::new("no/such/dir"), "/opt/app", &mut plan).unwrap_err();
        assert!(
            err.to_string().contains("sync directory"),
            "error should name the failing step: {err}"
        );
    }

    #[test]
    fn mkdir_command_creates_all_dirs_in_one_call() {
        let cmd = mkdir_command(&["/opt/app".into(), "/opt/app/models".into()]);
        assert_eq!(cmd, "mkdir -p /opt/app /opt/app/models");
    }

    #[test]
    fn mkdir_command_expands_tilde_on_the_device() {
        // `~` must reach the remote shell unquoted, or the device would get a literal `~` dir.
        let cmd = mkdir_command(&["~/app".into()]);
        assert_eq!(cmd, "mkdir -p \"$HOME\"/app");
    }

    // ── binary path ──────────────────────────────────────────────────────────────

    #[test]
    fn local_binary_path_uses_target_and_profile() {
        assert_eq!(
            local_binary_path(Some("aarch64-unknown-linux-gnu"), false, "app"),
            "target/aarch64-unknown-linux-gnu/debug/app"
        );
        assert_eq!(
            local_binary_path(Some("aarch64-unknown-linux-gnu"), true, "app"),
            "target/aarch64-unknown-linux-gnu/release/app"
        );
        assert_eq!(local_binary_path(None, false, "app"), "target/debug/app");
    }

    // ── rsync argument building ──────────────────────────────────────────────────

    #[test]
    fn rsync_rsh_omitted_when_defaults_suffice() {
        assert_eq!(rsync_rsh("ssh", None), None);
    }

    #[test]
    fn rsync_rsh_leaves_posix_key_paths_unchanged() {
        assert_eq!(
            rsync_rsh("ssh", Some("/home/me/.ssh/id_ed25519")).unwrap(),
            "ssh -i /home/me/.ssh/id_ed25519"
        );
    }

    #[test]
    fn rsync_rsh_normalises_windows_separators() {
        // Backslashes would be eaten as escapes when rsync re-splits the -e string.
        assert_eq!(
            rsync_rsh("ssh", Some("C:\\Users\\me\\.ssh\\id_ed25519")).unwrap(),
            "ssh -i C:/Users/me/.ssh/id_ed25519"
        );
    }

    #[test]
    fn rsync_rsh_quotes_words_with_spaces() {
        // "C:\Program Files\Git\usr\bin\ssh.exe" is the common Windows case for both words.
        assert_eq!(
            rsync_rsh(
                "C:\\Program Files\\Git\\usr\\bin\\ssh.exe",
                Some("C:\\Users\\First Last\\.ssh\\key")
            )
            .unwrap(),
            "\"C:/Program Files/Git/usr/bin/ssh.exe\" -i \"C:/Users/First Last/.ssh/key\""
        );
    }

    #[test]
    fn rsync_rsh_included_for_custom_ssh_without_key() {
        assert_eq!(
            rsync_rsh("/usr/bin/ssh", None).unwrap(),
            "/usr/bin/ssh".to_owned()
        );
    }

    // ── sftp argument building ───────────────────────────────────────────────────

    #[test]
    fn sftp_binary_upload_restores_the_executable_bit() {
        // SFTP `put` drops the mode, so the device would refuse to exec the binary.
        let lines = sftp_binary_lines("target/debug/app", "app-dir/app");
        assert_eq!(
            lines,
            [
                "put \"target/debug/app\" \"app-dir/app\"",
                "chmod 755 \"app-dir/app\"",
            ]
        );
    }

    #[test]
    fn sftp_quote_wraps_and_normalises() {
        assert_eq!(sftp_quote("/opt/app/bin"), "\"/opt/app/bin\"");
        assert_eq!(sftp_quote("target\\debug\\app"), "\"target/debug/app\"");
    }

    #[test]
    fn sftp_quote_survives_spaces_and_quotes() {
        assert_eq!(sftp_quote("my app"), "\"my app\"");
        assert_eq!(sftp_quote("od\"d"), "\"od\\\"d\"");
    }

    #[test]
    fn sftp_path_turns_home_relative_paths_into_relative_ones() {
        // sftp has no shell: `~` would be created as a literal directory name.
        assert_eq!(sftp_path("~/paibeam"), "paibeam");
        assert_eq!(sftp_path("~"), ".");
        assert_eq!(sftp_path("/home/radxa/paibeam"), "/home/radxa/paibeam");
    }
}

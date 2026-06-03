//! Remote execution via SSH — streams stdout and stderr locally and forwards the exit code.

use crate::device::Device;
use anyhow::{Context, Result};
use std::process::Command;
use which::which;

/// Execute the binary on the device, streaming its output locally and forwarding the exit code.
/// For desktop devices, re-invokes `cargo run` locally (idempotent with the prior build step).
pub fn execute(
    device: &Device,
    binary_name: &str,
    cargo_args: &[String],
    remote_args: &[String],
) -> Result<()> {
    if device.is_desktop() {
        tracing::info!("running locally (desktop)");
        let mut cmd = Command::new("cargo");
        cmd.arg("run")
            .args(cargo_args)
            .arg("--bin")
            .arg(binary_name);
        if !remote_args.is_empty() {
            cmd.arg("--").args(remote_args);
        }
        let status = cmd.status().context("failed to invoke cargo run")?;
        if !status.success() {
            if let Some(code) = status.code() {
                std::process::exit(code);
            }
            anyhow::bail!("local process terminated by signal");
        }
        return Ok(());
    }
    which("ssh").context("ssh is not installed — install OpenSSH to use run")?;
    let (host, deploy_path) = device.require_ssh()?;
    tracing::info!(
        %host,
        %deploy_path,
        %binary_name,
        remote_args = ?remote_args,
        "executing on device via SSH"
    );
    let mut cmd = Command::new("ssh");
    if let Some(key) = &device.ssh_key {
        cmd.arg("-i").arg(key);
    }
    let script = remote_shell_command(deploy_path, binary_name, remote_args);
    // Single remote command string — parsed by the user's login shell (avoids `sh -c` arg splitting).
    cmd.arg(host).arg(script);
    let status = cmd.status().context("failed to invoke ssh")?;
    if !status.success() {
        if let Some(code) = status.code() {
            std::process::exit(code);
        }
        anyhow::bail!("remote process terminated by signal");
    }
    Ok(())
}

/// Run the binary from `deploy_path` so relative `sync_dirs` assets resolve on the device.
fn remote_shell_command(deploy_path: &str, binary_name: &str, remote_args: &[String]) -> String {
    let bin = format!("./{}", shell_escape(binary_name));
    let args = remote_args
        .iter()
        .map(|a| shell_escape(a))
        .collect::<Vec<_>>()
        .join(" ");
    if args.is_empty() {
        format!("cd {} && exec {}", shell_escape(deploy_path), bin)
    } else {
        format!("cd {} && exec {} {}", shell_escape(deploy_path), bin, args)
    }
}

fn shell_escape(s: &str) -> String {
    if s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-_./:".contains(&b))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_shell_changes_to_deploy_path() {
        let script = remote_shell_command("/opt/myapp", "myapp", &["status".into()]);
        assert_eq!(script, "cd /opt/myapp && exec ./myapp status");
    }

    #[test]
    fn remote_shell_no_args() {
        let script = remote_shell_command("/opt/myapp", "myapp", &[]);
        assert_eq!(script, "cd /opt/myapp && exec ./myapp");
    }

    #[test]
    fn shell_escape_plain_paths_unchanged() {
        assert_eq!(shell_escape("/opt/myapp"), "/opt/myapp");
        assert_eq!(shell_escape("myapp-v2.0"), "myapp-v2.0");
    }

    #[test]
    fn shell_escape_spaces_are_quoted() {
        assert_eq!(shell_escape("my app"), "'my app'");
    }

    #[test]
    fn shell_escape_single_quotes_escaped() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn shell_escape_tilde_is_quoted_not_expanded() {
        // `~` in a deploy_path is already expanded by device::resolve; if it still
        // appears here it must be treated as a literal to avoid double-expansion.
        assert_eq!(shell_escape("~/myapp"), "'~/myapp'");
    }
}

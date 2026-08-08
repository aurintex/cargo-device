//! Remote execution via SSH — streams stdout and stderr locally and forwards the exit code.

use crate::device::Device;
use crate::remote::{remote_path_token, shell_escape};
use anyhow::{Context, Result};
use std::io::IsTerminal;
use std::process::{Command, ExitStatus};
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
        // Fast path: no scripts to source → invoke cargo directly without a shell.
        if device.run_source.is_empty() {
            let mut cmd = Command::new("cargo");
            cmd.arg("run")
                .args(cargo_args)
                .arg("--bin")
                .arg(binary_name);
            if !remote_args.is_empty() {
                cmd.arg("--").args(remote_args);
            }
            let status = cmd.status().context("failed to invoke cargo run")?;
            return forward_exit(status, "local process terminated by signal");
        }
        // Source the run scripts in Bash, then exec `cargo run`. ROS/colcon setup files are
        // conventionally `setup.bash`, so POSIX `sh` is not enough for this path.
        which("bash").context(
            "run_source needs `bash` to source the scripts, but it is not on PATH — \
             on Windows install Git Bash or MSYS2, or drop run_source for the desktop device",
        )?;
        let script = local_run_command(&device.run_source, binary_name, cargo_args, remote_args);
        let status = Command::new("bash")
            .arg("-c")
            .arg(script)
            .status()
            .context("failed to invoke bash for cargo run")?;
        return forward_exit(status, "local process terminated by signal");
    }
    let ssh = device.ssh_cmd();
    which(ssh)
        .with_context(|| format!("`{ssh}` not found — install OpenSSH, or set ssh_program"))?;
    let (host, deploy_path) = device.require_ssh()?;
    tracing::info!(
        %host,
        %deploy_path,
        %binary_name,
        run_source = ?device.run_source,
        remote_args = ?remote_args,
        "executing on device via SSH"
    );
    let mut cmd = Command::new(ssh);
    // Allocate a PTY so interactive apps (e.g. ratatui TUI) get a real terminal on the
    // device — but only when our own stdin is one. Otherwise ssh warns
    // ("Pseudo-terminal will not be allocated because stdin is not a terminal") on every
    // piped or CI run, and the Windows client is stricter still about a non-console stdin.
    if std::io::stdin().is_terminal() {
        cmd.arg("-t");
    }
    if let Some(key) = &device.ssh_key {
        cmd.arg("-i").arg(key);
    }
    let script = remote_shell_command(deploy_path, binary_name, &device.run_source, remote_args);
    // Single remote command string — parsed by the user's login shell (avoids `sh -c` arg splitting).
    cmd.arg(host).arg(script);
    let status = cmd.status().context("failed to invoke ssh")?;
    forward_exit(status, "remote process terminated by signal")
}

/// Forward a child's exit status: exit with its code, or fail if it was killed by a signal.
fn forward_exit(status: ExitStatus, signal_msg: &str) -> Result<()> {
    if !status.success() {
        if let Some(code) = status.code() {
            std::process::exit(code);
        }
        anyhow::bail!("{signal_msg}");
    }
    Ok(())
}

/// Run the binary from `deploy_path` so relative `sync_dirs` assets resolve on the device,
/// sourcing each `run_source` script (in order) on the remote shell before exec.
fn remote_shell_command(
    deploy_path: &str,
    binary_name: &str,
    run_source: &[String],
    remote_args: &[String],
) -> String {
    let src = source_clause(run_source);
    let bin = format!("./{}", shell_escape(binary_name));
    let args = remote_args
        .iter()
        .map(|a| shell_escape(a))
        .collect::<Vec<_>>()
        .join(" ");
    // deploy_path is a *device* path: `~/app` must be expanded by the remote shell.
    let cwd = remote_path_token(deploy_path);
    let command = if args.is_empty() {
        format!("cd {cwd} && {src}exec {bin}")
    } else {
        format!("cd {cwd} && {src}exec {bin} {args}")
    };
    if run_source.is_empty() {
        command
    } else {
        format!("bash -lc {}", shell_escape(&command))
    }
}

/// Build the local `sh -c` command that sources the run scripts, then exec's `cargo run`.
fn local_run_command(
    run_source: &[String],
    binary_name: &str,
    cargo_args: &[String],
    remote_args: &[String],
) -> String {
    let src = source_clause(run_source);
    let mut parts = vec!["exec".to_owned(), "cargo".to_owned(), "run".to_owned()];
    parts.extend(cargo_args.iter().map(|a| shell_escape(a)));
    parts.push("--bin".to_owned());
    parts.push(shell_escape(binary_name));
    if !remote_args.is_empty() {
        parts.push("--".to_owned());
        parts.extend(remote_args.iter().map(|a| shell_escape(a)));
    }
    format!("{src}{}", parts.join(" "))
}

/// Build a `. <script> && . <script> && ` prefix sourcing each script in order, or `""` when
/// the list is empty. A leading `~/` is translated to `"$HOME"/…` so the run-host shell
/// expands it (these paths live on the run host, not the local machine).
fn source_clause(run_source: &[String]) -> String {
    run_source
        .iter()
        .map(|s| format!(". {} && ", remote_path_token(s)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_shell_changes_to_deploy_path() {
        let script = remote_shell_command("/opt/myapp", "myapp", &[], &["status".into()]);
        assert_eq!(script, "cd /opt/myapp && exec ./myapp status");
    }

    #[test]
    fn remote_shell_no_args() {
        let script = remote_shell_command("/opt/myapp", "myapp", &[], &[]);
        assert_eq!(script, "cd /opt/myapp && exec ./myapp");
    }

    #[test]
    fn remote_shell_empty_run_source_unchanged() {
        // No scripts → byte-identical to the pre-run_source behaviour.
        let script = remote_shell_command("/opt/app", "app", &[], &[]);
        assert_eq!(script, "cd /opt/app && exec ./app");
    }

    #[test]
    fn remote_shell_sources_absolute_script() {
        let src = vec!["/opt/ros/humble/setup.bash".into()];
        let script = remote_shell_command("/opt/app", "app", &src, &[]);
        assert_eq!(
            script,
            "bash -lc 'cd /opt/app && . /opt/ros/humble/setup.bash && exec ./app'"
        );
    }

    #[test]
    fn remote_shell_bash_wrapper_escapes_inner_quotes() {
        let src = vec!["/opt/ros/humble/setup.bash".into()];
        let script = remote_shell_command("/opt/app's", "app", &src, &["it's".into()]);
        assert_eq!(
            script,
            "bash -lc 'cd '\\''/opt/app'\\''\\'\\'''\\''s'\\'' && . /opt/ros/humble/setup.bash && exec ./app '\\''it'\\''\\'\\'''\\''s'\\'''"
        );
    }

    #[test]
    fn remote_shell_tilde_script_uses_remote_home() {
        // `~/` must be expanded by the *remote* shell, not quoted literally.
        let src = vec!["~/ros2_humble/install/setup.bash".into()];
        let script = remote_shell_command("/opt/app", "app", &src, &[]);
        assert_eq!(
            script,
            "bash -lc 'cd /opt/app && . \"$HOME\"/ros2_humble/install/setup.bash && exec ./app'"
        );
    }

    #[test]
    fn remote_shell_sources_multiple_scripts_in_order() {
        let src = vec![
            "~/ros2_humble/install/setup.bash".into(),
            "~/ldlidar_ros2_ws/install/setup.bash".into(),
        ];
        let script = remote_shell_command("/opt/app", "app", &src, &["run".into()]);
        assert_eq!(
            script,
            "bash -lc 'cd /opt/app && \
             . \"$HOME\"/ros2_humble/install/setup.bash && \
             . \"$HOME\"/ldlidar_ros2_ws/install/setup.bash && \
             exec ./app run'"
        );
    }

    #[test]
    fn local_run_sources_then_execs_cargo() {
        let src = vec!["/opt/ros/humble/setup.bash".into()];
        let cmd = local_run_command(&src, "app", &["--release".into()], &["run".into()]);
        assert_eq!(
            cmd,
            ". /opt/ros/humble/setup.bash && exec cargo run --release --bin app -- run"
        );
    }

    #[test]
    fn remote_shell_tilde_deploy_path_uses_remote_home() {
        // `deploy_path` is a device path: `~` belongs to the device user, so it must be
        // expanded by the remote shell — never locally (which on Windows would produce
        // something like `C:\Users\me/app`).
        let script = remote_shell_command("~/myapp", "app", &[], &[]);
        assert_eq!(script, "cd \"$HOME\"/myapp && exec ./app");
    }
}

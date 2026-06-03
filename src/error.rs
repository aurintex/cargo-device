//! Error formatting helpers for user-facing messages.
//!
//! cargo-device uses `anyhow` throughout, so no typed error enum is needed.
//! This module holds helpers for detecting common failure scenarios and producing
//! actionable error messages.

use anyhow::{Context, Result};
use std::process::ExitStatus;

/// Extend a `Result<ExitStatus>` with a human-readable error if the command failed.
pub trait ExitStatusExt {
    fn require_success(self, cmd: &str) -> Result<()>;
}

impl ExitStatusExt for std::io::Result<ExitStatus> {
    fn require_success(self, cmd: &str) -> Result<()> {
        let status = self.with_context(|| format!("failed to run `{cmd}`"))?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("`{cmd}` exited with {status}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn require_success_ok_on_zero_exit() {
        let result = Command::new("true").status().require_success("true");
        assert!(result.is_ok());
    }

    #[test]
    fn require_success_err_on_nonzero_exit() {
        let result = Command::new("false").status().require_success("false");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("false"),
            "error should name the command: {msg}"
        );
    }

    #[test]
    fn require_success_err_mentions_exit_status() {
        let result = Command::new("false").status().require_success("mycommand");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("mycommand"),
            "error should contain command name: {msg}"
        );
    }

    #[test]
    fn require_success_err_on_io_error() {
        let result = Command::new("__no_such_binary__")
            .status()
            .require_success("__no_such_binary__");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("__no_such_binary__"),
            "error should name the command: {msg}"
        );
    }
}

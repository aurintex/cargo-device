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

//! Allow-listed handler execution.
//!
//! A job document names a handler by file name plus arguments. This module
//! resolves the handler *only* inside a configured allow-list directory, runs it
//! with a bounded timeout, captures its output, and maps the result to a job
//! status.

use crate::error::{Error, Result};
use crate::jobs::model::{HandlerAction, JobStatus};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

/// Outcome of running a handler.
#[derive(Debug, Clone)]
pub struct HandlerOutcome {
    /// The job status to report.
    pub status: JobStatus,
    /// Details to attach to `statusDetails` (reason, captured output).
    pub details: std::collections::BTreeMap<String, String>,
}

/// Configuration for handler execution.
#[derive(Debug, Clone)]
pub struct HandlerRunner {
    /// Directory containing the allow-listed handler executables.
    pub handler_dir: PathBuf,
    /// Default per-handler timeout when the job does not specify one.
    pub default_timeout: Duration,
    /// Whether to include captured stdout in the reported `statusDetails`.
    pub include_stdout: bool,
}

impl HandlerRunner {
    /// Create a runner rooted at `handler_dir`.
    pub fn new(handler_dir: impl Into<PathBuf>, default_timeout: Duration) -> Self {
        Self {
            handler_dir: handler_dir.into(),
            default_timeout,
            include_stdout: false,
        }
    }

    /// Resolve the handler path, ensuring it stays inside the allow-list dir.
    ///
    /// Rejects any handler name containing path separators or `..` so a job
    /// document cannot escape the allow-list directory.
    fn resolve(&self, handler: &str) -> Result<PathBuf> {
        if handler.is_empty()
            || handler.contains('/')
            || handler.contains('\\')
            || handler.contains("..")
        {
            return Err(Error::HandlerNotAllowed(format!(
                "handler name must be a bare file name: {handler:?}"
            )));
        }
        let path = self.handler_dir.join(handler);
        if !path.is_file() {
            return Err(Error::HandlerNotAllowed(format!(
                "handler not found in allow-list dir: {}",
                path.display()
            )));
        }
        Ok(path)
    }

    /// Run the action, returning the job outcome to report.
    ///
    /// `timeout` overrides the default when `Some` (e.g. the job's step timeout).
    pub async fn run(
        &self,
        action: &HandlerAction,
        timeout: Option<Duration>,
    ) -> Result<HandlerOutcome> {
        let path = self.resolve(&action.handler)?;
        let budget = timeout.unwrap_or(self.default_timeout);

        let mut cmd = Command::new(&path);
        cmd.args(&action.args)
            .current_dir(&self.handler_dir)
            .kill_on_drop(true)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = cmd
            .spawn()
            .map_err(|e| Error::HandlerExec(format!("spawn {}: {e}", path.display())))?;

        let output = match tokio::time::timeout(budget, child.wait_with_output()).await {
            Ok(res) => res.map_err(|e| Error::HandlerExec(e.to_string()))?,
            Err(_) => {
                return Ok(timed_out(budget));
            }
        };

        Ok(self.map_output(&output))
    }

    fn map_output(&self, output: &std::process::Output) -> HandlerOutcome {
        let mut details = std::collections::BTreeMap::new();
        let stderr = truncate(&String::from_utf8_lossy(&output.stderr));
        if !stderr.is_empty() {
            details.insert("stderr".to_string(), stderr);
        }
        if self.include_stdout {
            let stdout = truncate(&String::from_utf8_lossy(&output.stdout));
            if !stdout.is_empty() {
                details.insert("stdout".to_string(), stdout);
            }
        }

        if output.status.success() {
            HandlerOutcome {
                status: JobStatus::Succeeded,
                details,
            }
        } else {
            let code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string());
            details.insert("exitCode".to_string(), code.clone());
            details.insert("reason".to_string(), format!("handler exited with {code}"));
            HandlerOutcome {
                status: JobStatus::Failed,
                details,
            }
        }
    }
}

fn timed_out(budget: Duration) -> HandlerOutcome {
    let mut details = std::collections::BTreeMap::new();
    details.insert("reason".to_string(), format!("timed out after {budget:?}"));
    HandlerOutcome {
        status: JobStatus::TimedOut,
        details,
    }
}

/// AWS IoT `statusDetails` values are limited; keep captured output bounded.
fn truncate(s: &str) -> String {
    const MAX: usize = 1024;
    let s = s.trim();
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…(truncated)", &s[..MAX])
    }
}

/// Path helper used by tests/docs to check a handler dir exists.
pub fn dir_exists(dir: &Path) -> bool {
    dir.is_dir()
}

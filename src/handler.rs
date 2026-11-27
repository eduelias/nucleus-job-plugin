//! Execution of job actions: allow-listed handlers and direct commands.
//!
//! A job document produces an [`Action`]. This module runs it safely, following
//! the `aws-iot-device-client` convention so AWS managed templates and existing
//! sample handlers work unchanged:
//!
//! * `runHandler` resolves a **bare file name** inside the handler directory (or a
//!   `path` override that must be on the configured allow-list of directories) and
//!   invokes it **device-client style**: the first argument is the `runAsUser`
//!   name (empty string when unset), followed by the template args. The handler
//!   script is responsible for dropping privileges (typically `sudo -u "$user"`).
//! * `runCommand` runs an argv directly (no shell), optionally subject to an
//!   executable allow-list. Because there is no script to drop privileges, the
//!   runner drops to `runAsUser`'s uid/gid itself when it is running as root.
//!
//! Both enforce a bounded timeout, capture output, and map the result to a job
//! status.

use crate::error::{Error, Result};
use crate::jobs::model::{Action, ActionKind, JobStatus};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tracing::warn;

/// Outcome of running an action.
#[derive(Debug, Clone)]
pub struct HandlerOutcome {
    /// The job status to report.
    pub status: JobStatus,
    /// Details to attach to `statusDetails` (reason, captured output).
    pub details: BTreeMap<String, String>,
}

/// Configuration for action execution.
#[derive(Debug, Clone)]
pub struct HandlerRunner {
    /// Directory containing the allow-listed handler executables.
    pub handler_dir: PathBuf,
    /// Default per-action timeout when the job does not specify one.
    pub default_timeout: Duration,
    /// Whether to include captured stdout in the reported `statusDetails`.
    pub include_stdout: bool,
    /// Additional directories a job's `path` override may point at. The configured
    /// `handler_dir` is always permitted; any other override must be listed here.
    pub allowed_path_overrides: Vec<PathBuf>,
    /// If set, `runCommand` may only run executables whose file name is in this
    /// list. `None` disables the check (any command permitted).
    pub command_allow_list: Option<Vec<String>>,
}

impl HandlerRunner {
    /// Create a runner rooted at `handler_dir`.
    pub fn new(handler_dir: impl Into<PathBuf>, default_timeout: Duration) -> Self {
        Self {
            handler_dir: handler_dir.into(),
            default_timeout,
            include_stdout: false,
            allowed_path_overrides: Vec::new(),
            command_allow_list: None,
        }
    }

    /// Run an action, returning the job outcome to report.
    ///
    /// `timeout` overrides the default when `Some` (e.g. the job's step timeout).
    pub async fn run(&self, action: &Action, timeout: Option<Duration>) -> Result<HandlerOutcome> {
        let budget = timeout.unwrap_or(self.default_timeout);
        let mut cmd = self.build_command(action)?;

        // Privilege drop: `runHandler` scripts drop privileges themselves (they get
        // the user as their first arg, device-client style), so the runner only
        // drops uid/gid natively for `runCommand`.
        if matches!(action.kind, ActionKind::RunCommand { .. }) {
            apply_run_as_user(&mut cmd, action.run_as_user.as_deref());
        }

        cmd.kill_on_drop(true)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = cmd
            .spawn()
            .map_err(|e| Error::HandlerExec(format!("spawn: {e}")))?;

        let output = match tokio::time::timeout(budget, child.wait_with_output()).await {
            Ok(res) => res.map_err(|e| Error::HandlerExec(e.to_string()))?,
            Err(_) => return Ok(timed_out(budget)),
        };

        Ok(self.map_output(&output))
    }

    /// Construct the (not-yet-spawned) command for an action.
    fn build_command(&self, action: &Action) -> Result<Command> {
        match &action.kind {
            ActionKind::RunHandler {
                handler,
                args,
                path,
            } => {
                let dir = self.resolve_dir(path.as_deref())?;
                let exe = self.resolve_handler(&dir, handler)?;
                let mut cmd = Command::new(exe);
                // Device-client convention: argv[0] is the runAsUser (empty when
                // unset), followed by the template args. The script handles the
                // privilege drop (e.g. `sudo -u "$user"`).
                cmd.arg(action.run_as_user.as_deref().unwrap_or(""));
                cmd.args(args).current_dir(&dir);
                Ok(cmd)
            }
            ActionKind::RunCommand { argv } => {
                let program = argv
                    .first()
                    .ok_or_else(|| Error::InvalidJobDocument("empty command argv".into()))?;
                self.check_command_allowed(program)?;
                let mut cmd = Command::new(program);
                cmd.args(&argv[1..]);
                Ok(cmd)
            }
        }
    }

    /// Resolve the handler directory, honoring an allow-listed `path` override.
    fn resolve_dir(&self, path: Option<&str>) -> Result<PathBuf> {
        match path {
            None => Ok(self.handler_dir.clone()),
            Some(p) => {
                let requested = PathBuf::from(p);
                if requested == self.handler_dir
                    || self.allowed_path_overrides.iter().any(|d| d == &requested)
                {
                    Ok(requested)
                } else {
                    Err(Error::HandlerNotAllowed(format!(
                        "path override not in allow-list: {p}"
                    )))
                }
            }
        }
    }

    /// Resolve the handler path inside `dir`, ensuring it stays inside it.
    ///
    /// Rejects any handler name containing path separators or `..` so a job
    /// document cannot escape the directory.
    fn resolve_handler(&self, dir: &Path, handler: &str) -> Result<PathBuf> {
        if !is_bare_name(handler) {
            return Err(Error::HandlerNotAllowed(format!(
                "handler name must be a bare file name: {handler:?}"
            )));
        }
        let path = dir.join(handler);
        if !path.is_file() {
            return Err(Error::HandlerNotAllowed(format!(
                "handler not found: {}",
                path.display()
            )));
        }
        Ok(path)
    }

    fn check_command_allowed(&self, program: &str) -> Result<()> {
        if let Some(list) = &self.command_allow_list {
            // Exact match only: matching on the file name would let `/tmp/x/ls` pass for `ls`.
            if !list.iter().any(|a| a == program) {
                return Err(Error::HandlerNotAllowed(format!(
                    "command not in allow-list: {program}"
                )));
            }
        }
        Ok(())
    }

    fn map_output(&self, output: &std::process::Output) -> HandlerOutcome {
        let mut details = BTreeMap::new();
        let stderr = truncate(&String::from_utf8_lossy(&output.stderr), MAX_DETAIL);
        if !stderr.is_empty() {
            details.insert("stderr".to_string(), stderr);
        }
        if self.include_stdout {
            let stdout = truncate(&String::from_utf8_lossy(&output.stdout), MAX_DETAIL);
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

/// Configure the child to run as `user` by dropping to its uid/gid.
///
/// Only effective on unix when the current process is root. If the runner is not
/// root, the request is logged and ignored (the child runs as the current user).
#[cfg(unix)]
fn apply_run_as_user(cmd: &mut Command, user: Option<&str>) {
    let Some(user) = user else { return };

    // Only root can change the child's uid/gid.
    // SAFETY: `geteuid` is always safe to call.
    let is_root = unsafe { libc_geteuid() } == 0;
    if !is_root {
        warn!(
            user = %user,
            "runAsUser requested but the runner is not root; running as the current user"
        );
        return;
    }

    match uzers::get_user_by_name(user) {
        Some(u) => {
            let uid = u.uid();
            let gid = u.primary_group_id();
            cmd.uid(uid).gid(gid);
        }
        None => {
            warn!(user = %user, "runAsUser not found in passwd database; running as root");
        }
    }
}

#[cfg(not(unix))]
fn apply_run_as_user(_cmd: &mut Command, user: Option<&str>) {
    if let Some(user) = user {
        warn!(user = %user, "runAsUser is only supported on unix; ignoring");
    }
}

// Minimal libc binding to avoid a full `libc` dependency just for geteuid.
#[cfg(unix)]
extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
}

/// A handler name that names a file directly inside its directory: no separators,
/// no `..`, not empty, not `.`.
fn is_bare_name(handler: &str) -> bool {
    !(handler.is_empty()
        || handler == "."
        || handler.contains('/')
        || handler.contains('\\')
        || handler.contains(".."))
}

fn timed_out(budget: Duration) -> HandlerOutcome {
    let mut details = BTreeMap::new();
    details.insert("reason".to_string(), format!("timed out after {budget:?}"));
    HandlerOutcome {
        status: JobStatus::TimedOut,
        details,
    }
}

/// Max bytes of captured output kept per `statusDetails` value.
const MAX_DETAIL: usize = 1024;

/// AWS IoT `statusDetails` values are limited; keep captured output bounded.
fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.len() <= max {
        s.to_string()
    } else {
        let mut out = s[..s.floor_char_boundary(max)].to_string();
        out.push_str("…(truncated)");
        out
    }
}

/// Path helper used by tests/docs to check a handler dir exists.
pub fn dir_exists(dir: &Path) -> bool {
    dir.is_dir()
}

#[cfg(kani)]
mod proofs {
    use super::*;

    /// Truncation never panics (even mid-character), keeps short output whole,
    /// and bounds long output.
    #[kani::proof]
    #[kani::unwind(6)]
    fn truncate_is_safe_and_bounded() {
        // Up to 4 bytes of 'a', ' ' and the two bytes of 'é' (0xC3 0xA9), in any
        // order, so the cut can land mid-character. A fixed buffer keeps CBMC fast.
        let set = [b'a', b' ', 0xC3, 0xA9];
        let buf: [u8; 4] = std::array::from_fn(|_| set[kani::any_where(|k: &usize| *k < 4)]);
        let len: usize = kani::any_where(|l| *l <= 4);
        let Ok(s) = std::str::from_utf8(&buf[..len]) else {
            return;
        };
        let max: usize = kani::any_where(|m| *m <= 4);
        let out = truncate(s, max);
        if s.trim().len() <= max {
            assert_eq!(out, s.trim());
        } else {
            assert!(out.len() <= max + "…(truncated)".len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Component;

    /// A name that passes the handler check is exactly one plain path component, so
    /// joined to the handler directory it cannot climb out or point elsewhere.
    #[test]
    fn bare_name_is_one_plain_component() {
        for h in crate::all_strings(&['a', '.', '/', '\\'], 5) {
            if is_bare_name(&h) {
                let c: Vec<_> = Path::new(&h).components().collect();
                assert!(
                    matches!(c[..], [Component::Normal(n)] if n == h.as_str()),
                    "{h:?} passed the check but is {c:?}"
                );
            }
        }
    }
}

//! JSON shapes for the AWS IoT Jobs MQTT protocol and the job document.
//!
//! Field names follow the AWS IoT Jobs API (see `reference/JOBS_PROTOCOL.md`).
//! Parsing is intentionally lenient (unknown fields ignored) so new service or
//! job-document fields don't break the runner.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Terminal and non-terminal job execution statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JobStatus {
    /// Queued, not yet picked up.
    Queued,
    /// Picked up and running on the device.
    InProgress,
    /// Completed successfully (terminal).
    Succeeded,
    /// Failed (terminal).
    Failed,
    /// Step timer expired (terminal).
    TimedOut,
    /// Declined by the device (terminal).
    Rejected,
    /// Removed from the device's list (terminal).
    Removed,
    /// Canceled in the cloud (terminal).
    Canceled,
}

/// A pending job execution as returned by StartNext / Describe / NextJobExecutionChanged.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobExecutionData {
    /// Unique job id.
    pub job_id: String,
    /// Thing this execution targets.
    #[serde(default)]
    pub thing_name: Option<String>,
    /// The job document (arbitrary JSON object defined by the job creator).
    #[serde(default)]
    pub job_document: serde_json::Value,
    /// Current status.
    pub status: JobStatus,
    /// Opaque name/value status details.
    #[serde(default)]
    pub status_details: BTreeMap<String, String>,
    /// Optimistic-concurrency version; echo as `expectedVersion` on updates.
    #[serde(default)]
    pub version_number: u64,
    /// Execution number identifying this run on the device.
    #[serde(default)]
    pub execution_number: Option<i64>,
}

/// Response envelope for `start-next/accepted` and `{jobId}/get/accepted`.
///
/// `execution` is absent when there is no pending job.
#[derive(Debug, Clone, Deserialize)]
pub struct DescribeResponse {
    /// The job to run, if any.
    #[serde(default)]
    pub execution: Option<JobExecutionData>,
}

/// Payload of the `notify-next` topic.
#[derive(Debug, Clone, Deserialize)]
pub struct NextJobExecutionChanged {
    /// The new next job, if any.
    #[serde(default)]
    pub execution: Option<JobExecutionData>,
}

/// Request body for StartNextPendingJobExecution (`.../start-next`).
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StartNextRequest {
    /// Optional status details to record on pickup.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub status_details: BTreeMap<String, String>,
    /// Optional step timer in minutes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_timeout_in_minutes: Option<i64>,
    /// Correlation token echoed in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
}

/// Request body for UpdateJobExecution (`.../{jobId}/update`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRequest {
    /// New status (required on every update).
    pub status: JobStatus,
    /// Status details (e.g. failure reason, captured stderr).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub status_details: BTreeMap<String, String>,
    /// Expected current version for optimistic concurrency.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<u64>,
    /// Execution number to update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_number: Option<i64>,
    /// Correlation token echoed in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_token: Option<String>,
}

/// Error payload published on `.../rejected` topics.
#[derive(Debug, Clone, Deserialize)]
pub struct ErrorResponse {
    /// Service error code (e.g. `VersionMismatch`).
    pub code: String,
    /// Human-readable message.
    #[serde(default)]
    pub message: String,
}

/// A parsed step action extracted from a job document.
///
/// Supports the `aws-iot-device-client` / AWS managed-template schema. Two action
/// types are recognized:
///
/// * `runHandler` — run an allow-listed handler executable (most managed templates
///   and custom handlers). Also accepts the flat convenience form
///   `{ "operation"|"handler": "h.sh", "args": [..] }`.
/// * `runCommand` — run a comma-separated argv directly (the `AWS-Run-Command`
///   template).
///
/// Both carry an optional `run_as_user` (empty string means "unset").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// The concrete work to perform.
    pub kind: ActionKind,
    /// User to drop privileges to before executing (empty/None => component user).
    pub run_as_user: Option<String>,
}

/// The concrete action variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionKind {
    /// Run an allow-listed handler executable.
    RunHandler {
        /// Handler executable file name (resolved inside the handler directory).
        handler: String,
        /// Positional arguments passed to the handler.
        args: Vec<String>,
        /// Optional handler-directory override (empty => configured handler dir).
        path: Option<String>,
    },
    /// Run a command argv directly (no shell).
    RunCommand {
        /// The argv: `argv[0]` is the program, the rest are arguments.
        argv: Vec<String>,
    },
}

impl Action {
    /// Extract the action from an arbitrary job-document JSON value.
    pub fn from_document(doc: &serde_json::Value) -> crate::error::Result<Self> {
        use crate::error::Error;

        // Flat convenience form: { "operation" | "handler": "name", "args": [...] }
        if let Some(name) = doc
            .get("operation")
            .or_else(|| doc.get("handler"))
            .and_then(|v| v.as_str())
        {
            return Ok(Self {
                kind: ActionKind::RunHandler {
                    handler: name.to_string(),
                    args: parse_args(doc.get("args")),
                    path: opt_str(doc.get("path")),
                },
                run_as_user: opt_str(doc.get("runAsUser")),
            });
        }

        // Stepped form: { "steps": [ { "action": { "type", "input", "runAsUser" } } ] }
        let action = doc
            .get("steps")
            .and_then(|s| s.as_array())
            .and_then(|s| s.first())
            .and_then(|step| step.get("action"))
            .ok_or_else(|| {
                Error::InvalidJobDocument(
                    "no `operation`/`handler` or `steps[].action` found".into(),
                )
            })?;

        let input = action
            .get("input")
            .ok_or_else(|| Error::InvalidJobDocument("steps[0].action.input missing".into()))?;
        let run_as_user = opt_str(action.get("runAsUser"));

        // Dispatch on the action `type`; default to runHandler when absent.
        let action_type = action.get("type").and_then(|v| v.as_str());
        match action_type {
            Some("runCommand") => {
                let command = input
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        Error::InvalidJobDocument("runCommand action missing input.command".into())
                    })?;
                let argv = parse_command(command);
                if argv.is_empty() {
                    return Err(Error::InvalidJobDocument(
                        "runCommand command is empty".into(),
                    ));
                }
                Ok(Self {
                    kind: ActionKind::RunCommand { argv },
                    run_as_user,
                })
            }
            Some("runHandler") | None => {
                let handler = input
                    .get("handler")
                    .or_else(|| input.get("operation"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        Error::InvalidJobDocument("runHandler action missing input.handler".into())
                    })?;
                Ok(Self {
                    kind: ActionKind::RunHandler {
                        handler: handler.to_string(),
                        args: parse_args(input.get("args")),
                        path: opt_str(input.get("path")),
                    },
                    run_as_user,
                })
            }
            Some(other) => Err(Error::InvalidJobDocument(format!(
                "unsupported action type: {other:?}"
            ))),
        }
    }
}

fn parse_args(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Read an optional string field, treating empty strings as unset.
fn opt_str(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Split a `runCommand` comma-separated argv, honoring `\,` as an escaped comma.
///
/// Per the `AWS-Run-Command` template spec the command is a comma-separated list
/// of argv tokens; a literal comma inside a token is escaped as `\,`.
pub fn parse_command(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&',') => {
                cur.push(',');
                chars.next();
            }
            ',' => {
                out.push(std::mem::take(&mut cur));
            }
            other => cur.push(other),
        }
    }
    out.push(cur);
    // Drop leading/trailing whitespace on each token; keep empty interior tokens out.
    out.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_start_next_with_execution() {
        let raw = r#"{
            "execution": {
                "jobId": "022", "thingName": "MyThing",
                "jobDocument": {"operation": "touch.sh", "args": ["/tmp/x"]},
                "status": "QUEUED", "versionNumber": 1, "executionNumber": 7
            },
            "timestamp": 1489088524284, "clientToken": "c1"
        }"#;
        let r: DescribeResponse = serde_json::from_str(raw).unwrap();
        let e = r.execution.expect("execution present");
        assert_eq!(e.job_id, "022");
        assert_eq!(e.status, JobStatus::Queued);
        assert_eq!(e.version_number, 1);
    }

    #[test]
    fn parses_empty_start_next() {
        let r: DescribeResponse = serde_json::from_str(r#"{"timestamp":1}"#).unwrap();
        assert!(r.execution.is_none());
    }

    #[test]
    fn handler_action_flat() {
        let doc = serde_json::json!({"operation": "h.sh", "args": ["a", "b"]});
        let a = Action::from_document(&doc).unwrap();
        assert_eq!(a.run_as_user, None);
        match a.kind {
            ActionKind::RunHandler {
                handler,
                args,
                path,
            } => {
                assert_eq!(handler, "h.sh");
                assert_eq!(args, vec!["a", "b"]);
                assert_eq!(path, None);
            }
            other => panic!("expected RunHandler, got {other:?}"),
        }
    }

    #[test]
    fn handler_action_stepped_managed_template() {
        // Shape emitted by e.g. AWS-Download-File after server-side substitution.
        let doc = serde_json::json!({
            "version": "1.0",
            "steps": [{
                "action": {
                    "name": "Download-File",
                    "type": "runHandler",
                    "input": {"handler": "download-file.sh", "args": ["https://x", "/opt/f"], "path": ""},
                    "runAsUser": ""
                }
            }]
        });
        let a = Action::from_document(&doc).unwrap();
        assert_eq!(a.run_as_user, None); // empty string => unset
        match a.kind {
            ActionKind::RunHandler {
                handler,
                args,
                path,
            } => {
                assert_eq!(handler, "download-file.sh");
                assert_eq!(args, vec!["https://x", "/opt/f"]);
                assert_eq!(path, None); // empty path => configured dir
            }
            other => panic!("expected RunHandler, got {other:?}"),
        }
    }

    #[test]
    fn run_command_action() {
        let doc = serde_json::json!({
            "steps": [{
                "action": {
                    "type": "runCommand",
                    "input": {"command": "systemctl,restart,my.service"},
                    "runAsUser": "root"
                }
            }]
        });
        let a = Action::from_document(&doc).unwrap();
        assert_eq!(a.run_as_user.as_deref(), Some("root"));
        match a.kind {
            ActionKind::RunCommand { argv } => {
                assert_eq!(argv, vec!["systemctl", "restart", "my.service"]);
            }
            other => panic!("expected RunCommand, got {other:?}"),
        }
    }

    #[test]
    fn run_command_escaped_comma() {
        assert_eq!(
            parse_command(r"echo,a\,b,c"),
            vec!["echo".to_string(), "a,b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn handler_with_path_override_and_user() {
        let doc = serde_json::json!({
            "steps": [{
                "action": {
                    "type": "runHandler",
                    "input": {"handler": "h.sh", "path": "/opt/handlers"},
                    "runAsUser": "ggc_user"
                }
            }]
        });
        let a = Action::from_document(&doc).unwrap();
        assert_eq!(a.run_as_user.as_deref(), Some("ggc_user"));
        match a.kind {
            ActionKind::RunHandler { path, .. } => {
                assert_eq!(path.as_deref(), Some("/opt/handlers"))
            }
            other => panic!("expected RunHandler, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_action_type_is_error() {
        let doc = serde_json::json!({
            "steps": [{"action": {"type": "runOta", "input": {}}}]
        });
        assert!(Action::from_document(&doc).is_err());
    }

    #[test]
    fn handler_action_missing() {
        let doc = serde_json::json!({"foo": "bar"});
        assert!(Action::from_document(&doc).is_err());
    }

    #[test]
    fn update_request_omits_empty_fields() {
        let req = UpdateRequest {
            status: JobStatus::InProgress,
            status_details: BTreeMap::new(),
            expected_version: Some(3),
            execution_number: None,
            client_token: None,
        };
        let s = serde_json::to_string(&req).unwrap();
        assert_eq!(s, r#"{"status":"IN_PROGRESS","expectedVersion":3}"#);
    }
}

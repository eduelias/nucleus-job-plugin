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

/// The parsed handler action extracted from a job document.
///
/// This runner supports a small, well-defined document shape compatible with the
/// `aws-iot-device-client` Jobs "run handler" model. Two forms are accepted:
///
/// * a flat document: `{ "operation": "h.sh", "args": [..], "path": "default" }`
/// * a stepped document: `{ "steps": [ { "action": { "input": { "handler": "h.sh", "args": [..] } } } ] }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerAction {
    /// Handler executable file name (looked up in the allow-list directory).
    pub handler: String,
    /// Arguments passed to the handler.
    pub args: Vec<String>,
}

impl HandlerAction {
    /// Extract the handler action from an arbitrary job-document JSON value.
    pub fn from_document(doc: &serde_json::Value) -> crate::error::Result<Self> {
        use crate::error::Error;

        // Flat form: { "operation" | "handler": "name", "args": [...] }
        if let Some(name) = doc
            .get("operation")
            .or_else(|| doc.get("handler"))
            .and_then(|v| v.as_str())
        {
            return Ok(Self {
                handler: name.to_string(),
                args: parse_args(doc.get("args")),
            });
        }

        // Stepped form: { "steps": [ { "action": { "input": { "handler", "args" } } } ] }
        if let Some(step) = doc
            .get("steps")
            .and_then(|s| s.as_array())
            .and_then(|s| s.first())
        {
            let input = step
                .get("action")
                .and_then(|a| a.get("input"))
                .ok_or_else(|| Error::InvalidJobDocument("steps[0].action.input missing".into()))?;
            let name = input
                .get("handler")
                .or_else(|| input.get("operation"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    Error::InvalidJobDocument("steps[0].action.input.handler missing".into())
                })?;
            return Ok(Self {
                handler: name.to_string(),
                args: parse_args(input.get("args")),
            });
        }

        Err(Error::InvalidJobDocument(
            "no `operation`/`handler` or `steps[].action.input.handler` found".into(),
        ))
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
        let a = HandlerAction::from_document(&doc).unwrap();
        assert_eq!(a.handler, "h.sh");
        assert_eq!(a.args, vec!["a", "b"]);
    }

    #[test]
    fn handler_action_stepped() {
        let doc = serde_json::json!({
            "steps": [{"action": {"input": {"handler": "run.sh", "args": ["1"]}}}]
        });
        let a = HandlerAction::from_document(&doc).unwrap();
        assert_eq!(a.handler, "run.sh");
        assert_eq!(a.args, vec!["1"]);
    }

    #[test]
    fn handler_action_missing() {
        let doc = serde_json::json!({"foo": "bar"});
        assert!(HandlerAction::from_document(&doc).is_err());
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

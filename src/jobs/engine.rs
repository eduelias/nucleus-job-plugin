//! The jobs workflow state machine.
//!
//! Flow: subscribe to response + `notify-next` topics → request the next job →
//! on receipt, mark `IN_PROGRESS`, run the handler, then report the terminal
//! status. Repeat on each `notify-next`.

use crate::error::{Error, Result};
use crate::handler::HandlerRunner;
use crate::jobs::model::{
    Action, DescribeResponse, ErrorResponse, JobExecutionData, JobStatus, NextJobExecutionChanged,
    StartNextRequest, UpdateRequest,
};
use crate::jobs::topics::JobTopics;
use crate::transport::{Incoming, JobsTransport};
use std::collections::{BTreeMap, HashMap};
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// After a transport error, how long to wait before asking for the next job again.
/// AWS sends nothing on its own after a lost publish, so this is what brings a
/// stranded job back.
const RETRY_DELAY: Duration = if cfg!(test) {
    Duration::from_millis(100)
} else {
    Duration::from_secs(30)
};

/// A job's final status and its `statusDetails`.
type FinalStatus = (JobStatus, BTreeMap<String, String>);

/// Drives the IoT Jobs workflow over a [`JobsTransport`].
pub struct Engine<T: JobsTransport> {
    transport: T,
    topics: JobTopics,
    runner: HandlerRunner,
    client_token: String,
    /// Job ids that are in-flight or already finished this session, so the same
    /// job is never processed twice (each status update mutates the pending list
    /// and re-triggers `notify-next`, which would otherwise re-enter processing).
    seen: std::sync::Mutex<std::collections::HashSet<String>>,
    /// Final statuses whose publish failed after the handler ran, re-sent when the
    /// job is delivered again (the handler must not run twice).
    unsent: std::sync::Mutex<HashMap<String, FinalStatus>>,
}

impl<T: JobsTransport> Engine<T> {
    /// Create an engine for `thing_name` using `runner` to execute handlers.
    pub fn new(transport: T, thing_name: &str, runner: HandlerRunner) -> Self {
        Self {
            transport,
            topics: JobTopics::new(thing_name),
            runner,
            client_token: "nucleus-job-plugin".to_string(),
            seen: std::sync::Mutex::new(std::collections::HashSet::new()),
            unsent: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Claim a job id for processing. Returns `false` if it is already in-flight or
    /// finished this session (so the caller should skip it).
    fn claim(&self, job_id: &str) -> bool {
        self.seen.lock().unwrap().insert(job_id.to_string())
    }

    /// Undo a claim for a job whose handler has not run, so a later
    /// `notify-next`/`start-next` retries it instead of skipping it forever.
    fn release(&self, job_id: &str) {
        self.seen.lock().unwrap().remove(job_id);
    }

    /// Subscribe to the topics the engine listens on.
    pub async fn subscribe_all(&self) -> Result<()> {
        self.transport.subscribe(&self.topics.notify_next()).await?;
        self.transport
            .subscribe(&self.topics.start_next_accepted())
            .await?;
        self.transport
            .subscribe(&self.topics.start_next_rejected())
            .await?;
        Ok(())
    }

    /// Publish a StartNextPendingJobExecution request.
    pub async fn request_next(&self) -> Result<()> {
        let req = StartNextRequest {
            client_token: Some(self.client_token.clone()),
            ..Default::default()
        };
        debug!("requesting next pending job");
        self.transport
            .publish(&self.topics.start_next(), serde_json::to_vec(&req)?)
            .await
    }

    /// Run the engine loop until the incoming stream closes.
    ///
    /// Subscribes, kicks off an initial `start-next`, then reacts to messages.
    pub async fn run(mut self) -> Result<()> {
        let mut rx = self
            .transport
            .incoming()
            .ok_or_else(|| Error::Transport("incoming stream already taken".into()))?;

        self.subscribe_all().await?;
        self.request_next().await?;

        use tokio::time::{sleep_until, Instant};
        let mut retry_at: Option<Instant> = None;
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    let Some(msg) = msg else { break };
                    if let Err(e) = self.handle_message(&msg).await {
                        error!(error = %e, topic = %msg.topic, "error handling message");
                        if matches!(e, Error::Transport(_)) {
                            retry_at.get_or_insert(Instant::now() + RETRY_DELAY);
                        }
                    }
                }
                _ = sleep_until(retry_at.unwrap_or_else(Instant::now)), if retry_at.is_some() => {
                    let failed = self.request_next().await.is_err();
                    retry_at = failed.then(|| Instant::now() + RETRY_DELAY);
                }
            }
        }
        info!("incoming stream closed; engine stopping");
        Ok(())
    }

    /// Route an incoming message to the right handler.
    async fn handle_message(&self, msg: &Incoming) -> Result<()> {
        let topic = &msg.topic;
        if topic == &self.topics.notify_next() {
            let n: NextJobExecutionChanged = serde_json::from_slice(&msg.payload)?;
            match n.execution {
                Some(job) => self.process_job(job).await,
                None => {
                    debug!("notify-next: no pending job");
                    Ok(())
                }
            }
        } else if topic == &self.topics.start_next_accepted() {
            let r: DescribeResponse = serde_json::from_slice(&msg.payload)?;
            match r.execution {
                Some(job) => self.process_job(job).await,
                None => {
                    debug!("start-next: no pending job");
                    Ok(())
                }
            }
        } else if topic == &self.topics.start_next_rejected() {
            let err: ErrorResponse = serde_json::from_slice(&msg.payload)?;
            warn!(code = %err.code, "start-next rejected: {}", err.message);
            Ok(())
        } else if topic.ends_with("/update/accepted") {
            debug!(topic = %topic, "update accepted");
            Ok(())
        } else if topic.ends_with("/update/rejected") {
            let err: ErrorResponse = serde_json::from_slice(&msg.payload)?;
            warn!(code = %err.code, "update rejected: {}", err.message);
            Ok(())
        } else {
            debug!(topic = %topic, "ignoring unrecognized topic");
            Ok(())
        }
    }

    /// Full processing of one job: in-progress → run handler → terminal status.
    async fn process_job(&self, job: JobExecutionData) -> Result<()> {
        // De-duplicate: skip jobs already in-flight or finished this session.
        if !self.claim(&job.job_id) {
            let unsent = self.unsent.lock().unwrap().remove(&job.job_id);
            if let Some((status, details)) = unsent {
                info!(job_id = %job.job_id, "re-sending final status");
                return self.finish(&job, status, details).await;
            }
            debug!(job_id = %job.job_id, "job already handled; skipping");
            return Ok(());
        }
        info!(job_id = %job.job_id, "picked up job");

        // Parse the handler action up front; a bad document fails the job.
        let action = match Action::from_document(&job.job_document) {
            Ok(a) => a,
            Err(e) => {
                warn!(job_id = %job.job_id, error = %e, "invalid job document");
                let mut details = BTreeMap::new();
                details.insert("reason".to_string(), e.to_string());
                return self
                    .update(&job, JobStatus::Failed, details)
                    .await
                    .inspect_err(|_| self.release(&job.job_id));
            }
        };

        // Mark in progress. If that publish fails the handler has not run, so release
        // the claim; past this point the claim stays (at-most-once execution).
        self.update(&job, JobStatus::InProgress, BTreeMap::new())
            .await
            .inspect_err(|_| self.release(&job.job_id))?;

        // Execute.
        let outcome = match self.runner.run(&action, None).await {
            Ok(o) => o,
            Err(e) => {
                error!(job_id = %job.job_id, error = %e, "handler error");
                let mut details = BTreeMap::new();
                details.insert("reason".to_string(), e.to_string());
                return self.finish(&job, JobStatus::Failed, details).await;
            }
        };

        info!(job_id = %job.job_id, status = ?outcome.status, "job finished");
        self.finish(&job, outcome.status, outcome.details).await
    }

    /// Publish a job's final status. If that fails, keep it so the next delivery of
    /// the job re-sends it instead of skipping the job.
    async fn finish(
        &self,
        job: &JobExecutionData,
        status: JobStatus,
        details: BTreeMap<String, String>,
    ) -> Result<()> {
        self.update(job, status, details.clone())
            .await
            .inspect_err(|_| {
                self.unsent
                    .lock()
                    .unwrap()
                    .insert(job.job_id.clone(), (status, details));
            })
    }

    /// Publish an UpdateJobExecution.
    ///
    /// We intentionally do **not** send `expectedVersion`: a single runner owns the
    /// execution, and each update (e.g. `IN_PROGRESS`) increments the server-side
    /// version, so echoing the original version on the terminal update would be
    /// rejected as a `VersionMismatch` and leave the job stuck `IN_PROGRESS`.
    async fn update(
        &self,
        job: &JobExecutionData,
        status: JobStatus,
        status_details: BTreeMap<String, String>,
    ) -> Result<()> {
        let req = UpdateRequest {
            status,
            status_details,
            expected_version: None,
            execution_number: job.execution_number,
            client_token: Some(self.client_token.clone()),
        };
        self.transport
            .publish(&self.topics.update(&job.job_id), serde_json::to_vec(&req)?)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::mock::MockTransport;

    /// The handler finishes but its SUCCEEDED publish is lost. The engine must ask
    /// for the next job again on its own, and when AWS hands the same job back,
    /// re-send SUCCEEDED without running the handler a second time.
    #[tokio::test]
    async fn lost_final_status_is_resent_without_rerunning() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("runs.log");
        let script = tmp.path().join("slow.sh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho run >> '{}'\nsleep 0.3\n", log.display()),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let (transport, h) = MockTransport::new();
        let runner = HandlerRunner::new(tmp.path(), Duration::from_secs(5));
        let engine = Engine::new(transport, "T", runner);
        let topics = JobTopics::new("T");
        let job = serde_json::json!({"execution": {
            "jobId": "j", "jobDocument": {"operation": "slow.sh"},
            "status": "IN_PROGRESS", "versionNumber": 2
        }});
        let start_nexts = |h: &crate::transport::mock::MockHandle| {
            let t = topics.start_next();
            h.published().iter().filter(|p| p.topic == t).count()
        };

        let driver = {
            let (h, topics, job) = (h.clone(), topics.clone(), job.clone());
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                h.inject(&topics.notify_next(), job.clone()).await;
                // IN_PROGRESS is out and the handler is sleeping: lose the next publish.
                tokio::time::sleep(Duration::from_millis(100)).await;
                h.fail_next_publishes(1);
                // The engine retries start-next by itself; answer it with the same job.
                tokio::time::sleep(Duration::from_millis(600)).await;
                h.inject(&topics.start_next_accepted(), job).await;
                tokio::time::sleep(Duration::from_millis(100)).await;
            })
        };
        let _ = tokio::time::timeout(Duration::from_millis(1000), engine.run()).await;
        driver.await.unwrap();

        assert!(start_nexts(&h) >= 2, "engine should re-request start-next");
        let update = topics.update("j");
        let statuses: Vec<String> = h
            .published()
            .iter()
            .filter(|p| p.topic == update)
            .map(|p| {
                serde_json::from_slice::<serde_json::Value>(&p.payload).unwrap()["status"]
                    .to_string()
            })
            .collect();
        assert_eq!(statuses, [r#""IN_PROGRESS""#, r#""SUCCEEDED""#]);
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "run\n",
            "handler ran once"
        );
    }
}

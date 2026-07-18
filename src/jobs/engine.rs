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
use std::collections::BTreeMap;
use tracing::{debug, error, info, warn};

/// Drives the IoT Jobs workflow over a [`JobsTransport`].
pub struct Engine<T: JobsTransport> {
    transport: T,
    topics: JobTopics,
    runner: HandlerRunner,
    client_token: String,
}

impl<T: JobsTransport> Engine<T> {
    /// Create an engine for `thing_name` using `runner` to execute handlers.
    pub fn new(transport: T, thing_name: &str, runner: HandlerRunner) -> Self {
        Self {
            transport,
            topics: JobTopics::new(thing_name),
            runner,
            client_token: "nucleus-job-plugin".to_string(),
        }
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

        while let Some(msg) = rx.recv().await {
            if let Err(e) = self.handle_message(&msg).await {
                error!(error = %e, topic = %msg.topic, "error handling message");
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
        info!(job_id = %job.job_id, "picked up job");

        // Parse the handler action up front; a bad document fails the job.
        let action = match Action::from_document(&job.job_document) {
            Ok(a) => a,
            Err(e) => {
                warn!(job_id = %job.job_id, error = %e, "invalid job document");
                let mut details = BTreeMap::new();
                details.insert("reason".to_string(), e.to_string());
                self.update(&job, JobStatus::Failed, details).await?;
                return Ok(());
            }
        };

        // Mark in progress (best effort; version-conflict just logs).
        self.update(&job, JobStatus::InProgress, BTreeMap::new())
            .await?;

        // Execute.
        let outcome = match self.runner.run(&action, None).await {
            Ok(o) => o,
            Err(e) => {
                error!(job_id = %job.job_id, error = %e, "handler error");
                let mut details = BTreeMap::new();
                details.insert("reason".to_string(), e.to_string());
                self.update(&job, JobStatus::Failed, details).await?;
                return Ok(());
            }
        };

        info!(job_id = %job.job_id, status = ?outcome.status, "job finished");
        self.update(&job, outcome.status, outcome.details).await?;

        // Ask for the next one.
        self.request_next().await
    }

    /// Publish an UpdateJobExecution with optimistic-concurrency version.
    async fn update(
        &self,
        job: &JobExecutionData,
        status: JobStatus,
        status_details: BTreeMap<String, String>,
    ) -> Result<()> {
        let req = UpdateRequest {
            status,
            status_details,
            expected_version: Some(job.version_number),
            execution_number: job.execution_number,
            client_token: Some(self.client_token.clone()),
        };
        self.transport
            .publish(&self.topics.update(&job.job_id), serde_json::to_vec(&req)?)
            .await
    }
}

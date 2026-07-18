//! Builders for the reserved AWS IoT Jobs MQTT topics.
//!
//! All topics are prefixed with `$aws/things/{thingName}/jobs/`. See
//! `docs/JOBS_PROTOCOL.md` for the authoritative list.

/// Builds the reserved Jobs topic strings for a single thing.
#[derive(Debug, Clone)]
pub struct JobTopics {
    prefix: String,
}

impl JobTopics {
    /// Create a topic builder for `thing_name`.
    pub fn new(thing_name: &str) -> Self {
        Self {
            prefix: format!("$aws/things/{thing_name}/jobs"),
        }
    }

    /// `.../get` — GetPendingJobExecutions request.
    pub fn get(&self) -> String {
        format!("{}/get", self.prefix)
    }

    /// `.../start-next` — StartNextPendingJobExecution request.
    pub fn start_next(&self) -> String {
        format!("{}/start-next", self.prefix)
    }

    /// `.../start-next/accepted`.
    pub fn start_next_accepted(&self) -> String {
        format!("{}/start-next/accepted", self.prefix)
    }

    /// `.../start-next/rejected`.
    pub fn start_next_rejected(&self) -> String {
        format!("{}/start-next/rejected", self.prefix)
    }

    /// `.../{jobId}/get` — DescribeJobExecution request (`jobId` may be `$next`).
    pub fn describe(&self, job_id: &str) -> String {
        format!("{}/{job_id}/get", self.prefix)
    }

    /// `.../{jobId}/update` — UpdateJobExecution request.
    pub fn update(&self, job_id: &str) -> String {
        format!("{}/{job_id}/update", self.prefix)
    }

    /// `.../{jobId}/update/accepted`.
    pub fn update_accepted(&self, job_id: &str) -> String {
        format!("{}/{job_id}/update/accepted", self.prefix)
    }

    /// `.../{jobId}/update/rejected`.
    pub fn update_rejected(&self, job_id: &str) -> String {
        format!("{}/{job_id}/update/rejected", self.prefix)
    }

    /// `.../notify-next` — NextJobExecutionChanged subscription.
    pub fn notify_next(&self) -> String {
        format!("{}/notify-next", self.prefix)
    }

    /// `.../notify` — JobExecutionsChanged subscription.
    pub fn notify(&self) -> String {
        format!("{}/notify", self.prefix)
    }

    /// Wildcard covering every reserved jobs topic for this thing.
    ///
    /// Useful for the component's IPC `mqttproxy` authorization policy.
    pub fn wildcard(&self) -> String {
        format!("{}/*", self.prefix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_expected_topics() {
        let t = JobTopics::new("myThing");
        assert_eq!(t.start_next(), "$aws/things/myThing/jobs/start-next");
        assert_eq!(
            t.start_next_accepted(),
            "$aws/things/myThing/jobs/start-next/accepted"
        );
        assert_eq!(t.describe("$next"), "$aws/things/myThing/jobs/$next/get");
        assert_eq!(t.update("022"), "$aws/things/myThing/jobs/022/update");
        assert_eq!(
            t.update_rejected("022"),
            "$aws/things/myThing/jobs/022/update/rejected"
        );
        assert_eq!(t.notify_next(), "$aws/things/myThing/jobs/notify-next");
        assert_eq!(t.wildcard(), "$aws/things/myThing/jobs/*");
    }
}

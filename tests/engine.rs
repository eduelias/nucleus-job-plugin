//! Engine workflow tests driven by the mock transport and fake handler scripts.

use nucleus_job_plugin::handler::HandlerRunner;
use nucleus_job_plugin::jobs::topics::JobTopics;
use nucleus_job_plugin::jobs::Engine;
use nucleus_job_plugin::transport::mock::MockTransport;
use std::time::Duration;

const THING: &str = "MyThing";

fn write_handler(dir: &std::path::Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[tokio::test]
async fn runs_a_job_end_to_end_and_reports_succeeded() {
    let tmp = tempfile::tempdir().unwrap();
    let marker = tmp.path().join("ran.txt");
    write_handler(
        tmp.path(),
        "touch.sh",
        &format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    );

    let (transport, handle) = MockTransport::new();
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let engine = Engine::new(transport, THING, runner);

    let topics = JobTopics::new(THING);
    let h = handle.clone();
    let accepted = topics.start_next_accepted();
    let driver = tokio::spawn(async move {
        // Wait for the engine to subscribe + request, then feed a job.
        tokio::time::sleep(Duration::from_millis(50)).await;
        h.inject(
            &accepted,
            serde_json::json!({
                "execution": {
                    "jobId": "j-1", "thingName": THING,
                    "jobDocument": {"operation": "touch.sh"},
                    "status": "QUEUED", "versionNumber": 3, "executionNumber": 1
                }
            }),
        )
        .await;
        // Give the engine time to process, then close the stream.
        tokio::time::sleep(Duration::from_millis(200)).await;
    });

    // Run the engine until driver drops its injector by ending... we stop via timeout.
    let _ = tokio::time::timeout(Duration::from_secs(2), engine.run()).await;
    driver.await.unwrap();

    // Handler ran.
    assert!(
        marker.exists(),
        "handler should have created the marker file"
    );

    // Verify the published sequence: start-next, update(IN_PROGRESS), update(SUCCEEDED).
    let pubs = handle.published();
    let update_topic = topics.update("j-1");
    let updates: Vec<_> = pubs.iter().filter(|p| p.topic == update_topic).collect();
    assert_eq!(updates.len(), 2, "two updates expected");

    let first: serde_json::Value = serde_json::from_slice(&updates[0].payload).unwrap();
    assert_eq!(first["status"], "IN_PROGRESS");
    // expectedVersion is intentionally not sent (single-owner runner).
    assert!(first.get("expectedVersion").is_none());

    let second: serde_json::Value = serde_json::from_slice(&updates[1].payload).unwrap();
    assert_eq!(second["status"], "SUCCEEDED");

    // Subscribed to notify-next and start-next responses.
    let subs = handle.subscribed();
    assert!(subs.contains(&topics.notify_next()));
    assert!(subs.contains(&topics.start_next_accepted()));
}

#[tokio::test]
async fn invalid_job_document_reports_failed_without_running() {
    let tmp = tempfile::tempdir().unwrap();
    let (transport, handle) = MockTransport::new();
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let engine = Engine::new(transport, THING, runner);

    let topics = JobTopics::new(THING);
    let h = handle.clone();
    let notify = topics.notify_next();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        h.inject(
            &notify,
            serde_json::json!({
                "execution": {
                    "jobId": "bad", "jobDocument": {"nope": true},
                    "status": "QUEUED", "versionNumber": 1
                }
            }),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(150)).await;
    });

    let _ = tokio::time::timeout(Duration::from_secs(2), engine.run()).await;

    let pubs = handle.published();
    let update_topic = topics.update("bad");
    let updates: Vec<_> = pubs.iter().filter(|p| p.topic == update_topic).collect();
    assert_eq!(updates.len(), 1, "only the FAILED update expected");
    let body: serde_json::Value = serde_json::from_slice(&updates[0].payload).unwrap();
    assert_eq!(body["status"], "FAILED");
}

#[tokio::test]
async fn same_job_delivered_twice_is_processed_once() {
    // Each status update mutates the pending list and re-triggers notify-next; the
    // engine must not reprocess a job it has already handled.
    let tmp = tempfile::tempdir().unwrap();
    write_handler(tmp.path(), "ok.sh", "#!/bin/sh\nexit 0\n");
    let (transport, handle) = MockTransport::new();
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let engine = Engine::new(transport, THING, runner);

    let topics = JobTopics::new(THING);
    let h = handle.clone();
    let notify = topics.notify_next();
    let accepted = topics.start_next_accepted();
    tokio::spawn(async move {
        let job = serde_json::json!({
            "execution": {
                "jobId": "dup-1", "thingName": THING,
                "jobDocument": {"operation": "ok.sh"},
                "status": "QUEUED", "versionNumber": 1, "executionNumber": 1
            }
        });
        tokio::time::sleep(Duration::from_millis(40)).await;
        // Deliver the same job three times through different topics.
        h.inject(&accepted, job.clone()).await;
        h.inject(&notify, job.clone()).await;
        h.inject(&notify, job).await;
        tokio::time::sleep(Duration::from_millis(250)).await;
    });

    let _ = tokio::time::timeout(Duration::from_secs(2), engine.run()).await;

    // Exactly one IN_PROGRESS + one SUCCEEDED update for the single job.
    let pubs = handle.published();
    let update_topic = topics.update("dup-1");
    let updates: Vec<_> = pubs.iter().filter(|p| p.topic == update_topic).collect();
    assert_eq!(
        updates.len(),
        2,
        "job should be processed once: IN_PROGRESS + SUCCEEDED only, got {}",
        updates.len()
    );
}

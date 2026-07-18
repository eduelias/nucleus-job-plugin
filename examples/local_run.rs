//! Run the jobs engine against the in-memory mock transport with a sample job.
//!
//! ```sh
//! cargo run --example local_run
//! ```
//!
//! No network or AWS credentials required: it feeds a canned `start-next/accepted`
//! job whose document runs a temporary handler script, and prints what the engine
//! publishes back.

use nucleus_job_plugin::handler::HandlerRunner;
use nucleus_job_plugin::jobs::topics::JobTopics;
use nucleus_job_plugin::jobs::Engine;
use nucleus_job_plugin::transport::mock::MockTransport;
use std::time::Duration;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let thing = "DemoThing";
    let tmp = std::env::temp_dir().join("nucleus-job-plugin-demo");
    std::fs::create_dir_all(&tmp).unwrap();
    let handler = tmp.join("hello.sh");
    std::fs::write(&handler, "#!/bin/sh\necho \"hello from handler: $1\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&handler, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let (transport, handle) = MockTransport::new();
    let runner = HandlerRunner::new(tmp.clone(), Duration::from_secs(5));
    let engine = Engine::new(transport, thing, runner);

    let topics = JobTopics::new(thing);
    let accepted = topics.start_next_accepted();
    let h = handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        h.inject(
            &accepted,
            serde_json::json!({
                "execution": {
                    "jobId": "demo-1", "thingName": thing,
                    "jobDocument": {"operation": "hello.sh", "args": ["world"]},
                    "status": "QUEUED", "versionNumber": 1, "executionNumber": 1
                }
            }),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
    });

    let _ = tokio::time::timeout(Duration::from_secs(2), engine.run()).await;

    println!("\n--- messages the engine published ---");
    for p in handle.published() {
        println!("{}  {}", p.topic, String::from_utf8_lossy(&p.payload));
    }
}

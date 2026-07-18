//! Handler execution tests: allow-list enforcement, exit-code mapping, timeout.

use nucleus_job_plugin::handler::HandlerRunner;
use nucleus_job_plugin::jobs::model::{HandlerAction, JobStatus};
use std::time::Duration;

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
async fn success_maps_to_succeeded() {
    let tmp = tempfile::tempdir().unwrap();
    write_handler(tmp.path(), "ok.sh", "#!/bin/sh\necho hi\nexit 0\n");
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let action = HandlerAction {
        handler: "ok.sh".into(),
        args: vec![],
    };
    let out = runner.run(&action, None).await.unwrap();
    assert_eq!(out.status, JobStatus::Succeeded);
}

#[tokio::test]
async fn nonzero_exit_maps_to_failed_with_details() {
    let tmp = tempfile::tempdir().unwrap();
    write_handler(tmp.path(), "bad.sh", "#!/bin/sh\necho boom >&2\nexit 7\n");
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let action = HandlerAction {
        handler: "bad.sh".into(),
        args: vec![],
    };
    let out = runner.run(&action, None).await.unwrap();
    assert_eq!(out.status, JobStatus::Failed);
    assert_eq!(out.details.get("exitCode").map(String::as_str), Some("7"));
    assert_eq!(out.details.get("stderr").map(String::as_str), Some("boom"));
}

#[tokio::test]
async fn timeout_maps_to_timed_out() {
    let tmp = tempfile::tempdir().unwrap();
    write_handler(tmp.path(), "slow.sh", "#!/bin/sh\nsleep 5\n");
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_millis(100));
    let action = HandlerAction {
        handler: "slow.sh".into(),
        args: vec![],
    };
    let out = runner.run(&action, None).await.unwrap();
    assert_eq!(out.status, JobStatus::TimedOut);
}

#[tokio::test]
async fn path_traversal_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    for bad in ["../evil", "sub/evil", "/etc/passwd", ".."] {
        let action = HandlerAction {
            handler: bad.into(),
            args: vec![],
        };
        assert!(
            runner.run(&action, None).await.is_err(),
            "should reject {bad}"
        );
    }
}

#[tokio::test]
async fn missing_handler_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let action = HandlerAction {
        handler: "nope.sh".into(),
        args: vec![],
    };
    assert!(runner.run(&action, None).await.is_err());
}

//! Action execution tests: allow-list enforcement, exit-code mapping, timeout,
//! runCommand, and path/runAsUser handling.

use nucleus_job_plugin::handler::HandlerRunner;
use nucleus_job_plugin::jobs::model::{Action, ActionKind, JobStatus};
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

fn run_handler(handler: &str, args: Vec<String>) -> Action {
    Action {
        kind: ActionKind::RunHandler {
            handler: handler.into(),
            args,
            path: None,
        },
        run_as_user: None,
    }
}

#[tokio::test]
async fn success_maps_to_succeeded() {
    let tmp = tempfile::tempdir().unwrap();
    write_handler(tmp.path(), "ok.sh", "#!/bin/sh\necho hi\nexit 0\n");
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let out = runner
        .run(&run_handler("ok.sh", vec![]), None)
        .await
        .unwrap();
    assert_eq!(out.status, JobStatus::Succeeded);
}

#[tokio::test]
async fn nonzero_exit_maps_to_failed_with_details() {
    let tmp = tempfile::tempdir().unwrap();
    write_handler(tmp.path(), "bad.sh", "#!/bin/sh\necho boom >&2\nexit 7\n");
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let out = runner
        .run(&run_handler("bad.sh", vec![]), None)
        .await
        .unwrap();
    assert_eq!(out.status, JobStatus::Failed);
    assert_eq!(out.details.get("exitCode").map(String::as_str), Some("7"));
    assert_eq!(out.details.get("stderr").map(String::as_str), Some("boom"));
}

#[tokio::test]
async fn timeout_maps_to_timed_out() {
    let tmp = tempfile::tempdir().unwrap();
    write_handler(tmp.path(), "slow.sh", "#!/bin/sh\nsleep 5\n");
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_millis(100));
    let out = runner
        .run(&run_handler("slow.sh", vec![]), None)
        .await
        .unwrap();
    assert_eq!(out.status, JobStatus::TimedOut);
}

#[tokio::test]
async fn path_traversal_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    for bad in ["../evil", "sub/evil", "/etc/passwd", ".."] {
        assert!(
            runner.run(&run_handler(bad, vec![]), None).await.is_err(),
            "should reject {bad}"
        );
    }
}

#[tokio::test]
async fn missing_handler_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    assert!(runner
        .run(&run_handler("nope.sh", vec![]), None)
        .await
        .is_err());
}

#[tokio::test]
async fn path_override_not_in_allow_list_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    write_handler(other.path(), "h.sh", "#!/bin/sh\nexit 0\n");
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let action = Action {
        kind: ActionKind::RunHandler {
            handler: "h.sh".into(),
            args: vec![],
            path: Some(other.path().to_string_lossy().into_owned()),
        },
        run_as_user: None,
    };
    assert!(runner.run(&action, None).await.is_err());
}

#[tokio::test]
async fn path_override_in_allow_list_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    write_handler(other.path(), "h.sh", "#!/bin/sh\nexit 0\n");
    let mut runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    runner.allowed_path_overrides = vec![other.path().to_path_buf()];
    let action = Action {
        kind: ActionKind::RunHandler {
            handler: "h.sh".into(),
            args: vec![],
            path: Some(other.path().to_string_lossy().into_owned()),
        },
        run_as_user: None,
    };
    let out = runner.run(&action, None).await.unwrap();
    assert_eq!(out.status, JobStatus::Succeeded);
}

#[tokio::test]
async fn run_command_succeeds() {
    let tmp = tempfile::tempdir().unwrap();
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let action = Action {
        kind: ActionKind::RunCommand {
            argv: vec!["true".into()],
        },
        run_as_user: None,
    };
    let out = runner.run(&action, None).await.unwrap();
    assert_eq!(out.status, JobStatus::Succeeded);
}

#[tokio::test]
async fn run_command_respects_allow_list() {
    let tmp = tempfile::tempdir().unwrap();
    let mut runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    runner.command_allow_list = Some(vec!["echo".into()]);
    // Allowed:
    let ok = Action {
        kind: ActionKind::RunCommand {
            argv: vec!["echo".into(), "hi".into()],
        },
        run_as_user: None,
    };
    assert_eq!(
        runner.run(&ok, None).await.unwrap().status,
        JobStatus::Succeeded
    );
    // Denied:
    let denied = Action {
        kind: ActionKind::RunCommand {
            argv: vec!["rm".into(), "-rf".into()],
        },
        run_as_user: None,
    };
    assert!(runner.run(&denied, None).await.is_err());
}

#[tokio::test]
async fn run_as_user_ignored_when_not_root() {
    // When not root, runAsUser is ignored (logged) and the command still runs.
    let tmp = tempfile::tempdir().unwrap();
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let action = Action {
        kind: ActionKind::RunCommand {
            argv: vec!["true".into()],
        },
        run_as_user: Some("nobody".into()),
    };
    // On CI this runs as a non-root user, so it should just succeed.
    let out = runner.run(&action, None).await.unwrap();
    assert_eq!(out.status, JobStatus::Succeeded);
}

#[tokio::test]
async fn run_handler_prepends_run_as_user_as_first_arg() {
    // Device-client convention: argv[0] passed to the handler is the runAsUser
    // (empty when unset), followed by the template args.
    let tmp = tempfile::tempdir().unwrap();
    let out_file = tmp.path().join("args.txt");
    write_handler(
        tmp.path(),
        "echo-args.sh",
        &format!(
            "#!/bin/sh\nprintf 'user=[%s] a1=[%s] a2=[%s]' \"$1\" \"$2\" \"$3\" > '{}'\n",
            out_file.display()
        ),
    );
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let action = Action {
        kind: ActionKind::RunHandler {
            handler: "echo-args.sh".into(),
            args: vec!["https://x".into(), "/opt/f".into()],
            path: None,
        },
        run_as_user: Some("ggc_user".into()),
    };
    let out = runner.run(&action, None).await.unwrap();
    assert_eq!(out.status, JobStatus::Succeeded);
    let recorded = std::fs::read_to_string(&out_file).unwrap();
    assert_eq!(recorded, "user=[ggc_user] a1=[https://x] a2=[/opt/f]");
}

#[tokio::test]
async fn run_handler_empty_user_when_unset() {
    let tmp = tempfile::tempdir().unwrap();
    let out_file = tmp.path().join("args.txt");
    write_handler(
        tmp.path(),
        "echo-user.sh",
        &format!(
            "#!/bin/sh\nprintf 'user=[%s]' \"$1\" > '{}'\n",
            out_file.display()
        ),
    );
    let runner = HandlerRunner::new(tmp.path().to_path_buf(), Duration::from_secs(5));
    let action = Action {
        kind: ActionKind::RunHandler {
            handler: "echo-user.sh".into(),
            args: vec![],
            path: None,
        },
        run_as_user: None,
    };
    runner.run(&action, None).await.unwrap();
    assert_eq!(std::fs::read_to_string(&out_file).unwrap(), "user=[]");
}

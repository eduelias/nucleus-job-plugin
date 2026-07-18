---
name: nucleus-job-plugin-dev
description: Use when implementing, extending, or debugging nucleus-job-plugin (a Rust AWS IoT Jobs runner that runs as a Greengrass generic component). Covers the IoT Jobs MQTT protocol, the job-execution state machine, the allow-listed handler model, the transport abstraction (direct MQTT / Greengrass IPC), and generic-component packaging.
---

# nucleus-job-plugin development

A Rust runner that executes **AWS IoT Jobs** on a device (job execution only, like the
`aws-iot-device-client` Jobs feature). It runs as a **generic Greengrass component**
(`aws.greengrass.generic`) — a standalone process, NOT a JVM nucleus plugin (that type is Java-only).

Read `.opencode/PLAN.md` for the full plan and `reference/JOBS_PROTOCOL.md` for the exact IoT Jobs
MQTT topics and JSON payloads (the source of truth — do not guess topic strings or field names).

## Core mental model

- The device speaks the **IoT Jobs MQTT protocol** over reserved `$aws/things/{thingName}/jobs/...`
  topics. It subscribes to `notify-next`, asks for the next job, sets it `IN_PROGRESS`, runs the
  job, and reports `SUCCEEDED`/`FAILED`.
- **Transport is abstracted** behind a `JobsTransport` trait: `subscribe(topic)`, `publish(topic,
  payload)`, and an incoming-message stream. Two implementations:
  - direct MQTT via `rumqttc` (device cert/key), and
  - (optionally, later) Greengrass IPC IoT Core pub/sub.
  Everything else (engine, model, handler) is transport-agnostic and unit-tested with a mock.
- **Job execution** runs an **allow-listed handler**: the job document names a handler + args; the
  runner only executes handlers found in a configured allow-list directory, with a timeout, mapping
  exit 0 → SUCCEEDED and non-zero → FAILED (stderr/reason in `statusDetails`).

## The job workflow (state machine)

1. Connect transport; subscribe to the response topics + `notify-next`.
2. On startup and on each `notify-next`, publish `start-next` (or describe `$next`).
3. If a job is returned: publish `UpdateJobExecution` → `IN_PROGRESS`; run the handler; publish
   `UpdateJobExecution` → `SUCCEEDED`/`FAILED`. Use `expectedVersion` for optimistic concurrency;
   on a `rejected` version conflict, re-describe.
4. Loop.

## Adding / changing behavior

- **New job-document field**: update the `JobDocument` struct in `src/jobs/model.rs` (keep it a thin,
  documented struct; accept the device-client schema where sensible).
- **New transport**: implement `JobsTransport` in `src/transport/` behind a cargo feature.
- Always cover changes with engine tests driven by the **mock transport** (canned responses) and, for
  the handler, fake handler scripts in a temp dir.

## Verifying

- `scripts/check.sh` runs fmt + clippy (-D warnings) + tests. Run before every commit.
- Engine/handler are fully unit-testable with no network (mock transport + fake handlers).
- Real-device validation (when hardware is available): deploy as a generic component to a dev thing
  group, create an IoT Job with a small handler, confirm it reaches SUCCEEDED in the cloud. See
  `.opencode/PLAN.md` §7 for the dev-account/board/CLI details.

## Guardrails

- Scope is **job execution only** — no OTA/file-download jobs, no fleet provisioning, tunneling, or
  defender.
- Don't guess Jobs topic strings or payload fields — use `reference/JOBS_PROTOCOL.md`.
- Handler execution must be safe: allow-list directory only, bounded timeout, capture output, never
  run arbitrary paths from the job document.
- Remember: this is a generic component, not a nucleus plugin. The binary runs as its own process.

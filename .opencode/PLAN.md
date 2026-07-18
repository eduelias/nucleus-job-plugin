# nucleus-job-plugin — Implementation Plan

> Authoritative plan for a Rust AWS IoT Jobs runner that runs as a Greengrass generic component,
> mimicking the `aws-iot-device-client` Jobs feature (job execution only). Single source of truth
> for a fresh implementation session.

---

## 0. TL;DR

Build a Rust program that:
1. connects to **AWS IoT Core** (preferably **via Greengrass IPC**, reusing the nucleus's MQTT
   connection; alternatively a direct MQTT connection with the device cert),
2. speaks the **AWS IoT Jobs MQTT protocol** (`$aws/things/{thingName}/jobs/...`),
3. picks up the next queued job, sets it `IN_PROGRESS`, **executes the job document** by running an
   allow-listed handler executable/script (à la `aws-iot-device-client`), and updates the job
   `SUCCEEDED`/`FAILED`,
4. runs as a **generic Greengrass component** (its own process), packaged the same native-binary way
   as the locker-manager (`~/reps/pnl-apl/apl-embedded-locker-manager/greengrass`).

**Locked/assumed decisions** (confirm at kickoff):
- Rust, async (`tokio`), std.
- Preferred transport: **Greengrass IPC IoT Core pub/sub** (needs `greengrass-ipc` Tier 2). Fallback:
  direct MQTT via `rumqttc` with the device's cert/key.
- Job-execution model mirrors device-client: a **handler directory** of allow-listed executables;
  the job document names an `operation`/handler + `args`; the runner runs it and reports status.
- License `Apache-2.0 OR MIT`; unofficial/community; generic component (not a JVM plugin).

---

## 1. Terminology: "plugin" vs generic component

Greengrass component types (from the AWS docs "Develop Greengrass components"):
- `aws.greengrass.plugin` — runs **inside the nucleus JVM** (same process/classloader). **Java only.**
  A Rust program cannot be this.
- `aws.greengrass.generic` — runs commands/executables as **separate processes** the nucleus
  supervises via lifecycle scripts (Install/Startup/Run/Shutdown). **This is what we build.**

So: the deliverable is a standalone Rust binary + a **generic-component recipe** that runs it. The
folder name `nucleus-job-plugin` is kept for continuity but the README/plan clarify this.

---

## 2. AWS IoT Jobs MQTT protocol (device side)

All topics are prefixed `$aws/things/{thingName}/jobs/`. Requests carry an optional `clientToken`
for correlation; responses arrive on `.../accepted` and `.../rejected`.

| Operation | Publish (request) | Response topics |
|---|---|---|
| GetPendingJobExecutions | `.../get` | `.../get/accepted`, `.../get/rejected` |
| StartNextPendingJobExecution | `.../start-next` | `.../start-next/accepted`, `.../start-next/rejected` |
| DescribeJobExecution | `.../{jobId}/get` (jobId `$next` for next) | `.../{jobId}/get/accepted`, `.../{jobId}/get/rejected` |
| UpdateJobExecution | `.../{jobId}/update` | `.../{jobId}/update/accepted`, `.../{jobId}/update/rejected` |
| JobExecutionsChanged (notify) | subscribe `.../notify` | — |
| NextJobExecutionChanged (notify-next) | subscribe `.../notify-next` | — |

Key JSON shapes:
- **StartNextPendingJobExecution** req: `{ "statusDetails": {..}, "stepTimeoutInMinutes": long, "clientToken": "..." }`
- **UpdateJobExecution** req: `{ "status": "IN_PROGRESS|SUCCEEDED|FAILED|REJECTED", "statusDetails": {..}, "expectedVersion": n, "executionNumber": n, "stepTimeoutInMinutes": long, "clientToken": "..." }` — `status` required on every update.
- **JobExecutionData** (in start-next/describe accepted): `{ "jobId", "thingName", "jobDocument", "status", "queuedAt", "lastUpdatedAt", "versionNumber", "executionNumber" }`. If no job pending, `execution` is omitted.
- Job statuses: `QUEUED`, `IN_PROGRESS`, `SUCCEEDED`, `FAILED`, `TIMED_OUT`, `REJECTED`, `REMOVED`, `CANCELED`.

**Device workflow (standard):**
1. Subscribe to `.../notify-next` (and the accepted/rejected response topics you use).
2. On startup and on each notify-next, request the next job (`start-next` or describe `$next`).
3. If a job is present: `UpdateJobExecution` → `IN_PROGRESS`, run the job, then
   `UpdateJobExecution` → `SUCCEEDED` or `FAILED` (with `statusDetails`). Use `expectedVersion` for
   optimistic concurrency; handle `rejected` (version conflict) by re-describing.
4. Loop for the next job.

---

## 3. Job execution model (mimicking aws-iot-device-client)

`aws-iot-device-client` (Apache-2.0) Jobs feature runs a **job handler**:
- A **handler directory** (allow-list) of executables (default in device-client: `~/.aws-iot-device-client/jobs/`).
- The **job document** specifies the operation/handler and args. Representative schema
  (device-client "job document"): fields like `operation` (handler file name), `args` (array),
  `path` (handler dir override), `allowStdErr`, plus an optional `includeStdOut`. Confirm exact
  schema from the device-client docs/source before finalizing.
- The runner: validates the requested handler is in the allow-list dir, executes it with the args,
  captures stdout/stderr and exit code, enforces a timeout, and maps exit 0 → `SUCCEEDED`, non-zero
  → `FAILED` (with `statusDetails` carrying reason/stderr).

**Our scope:** implement this handler-runner faithfully but **only** this (no OTA/file-download jobs).
Provide a small, safe default: handlers must be in a configured allow-list directory, be owned by
root (or the run user) and not world-writable, executed with a bounded timeout.

Design the job-document parsing to accept the device-client schema so existing job documents work,
but keep it a thin, well-documented struct we control.

---

## 4. Transport: how to reach IoT Core

Two options (support both, prefer A):

**A. Via Greengrass IPC (recommended)** — reuse the nucleus's MQTT connection using
`SubscribeToIoTCore` / `PublishToIoTCore` from the sibling `greengrass-ipc` crate (Tier 2). Benefits:
no second MQTT connection, no separate cert management, works within the component sandbox/authz.
- **Caveat to verify:** whether reserved `$aws/things/.../jobs/*` topics are permitted through the
  Greengrass IPC IoT Core path and the component authorization policy. The component recipe must
  grant `aws.greengrass.ipc.mqttproxy` `PublishToIoTCore`/`SubscribeToIoTCore` for the specific
  `$aws/things/<thing>/jobs/*` topics. Test early on the board.

**B. Direct MQTT (fallback / standalone)** — open our own MQTTS connection to the IoT Core endpoint
using the device's existing cert/key (the same ones Greengrass uses, discoverable from the nucleus
config / provisioning). Use `rumqttc` (MIT/Apache-2.0, mature, tokio) for the client.
- Reference for the Jobs state machine: `rustot` (crate, MIT OR Apache-2.0) implements AWS IoT Jobs
  (and even a Greengrass IPC MQTT backend) but is `no-std`/embedded-oriented and last released 2022
  — use as a **reference**, not necessarily a dependency.

Abstract the transport behind a trait (`JobsTransport`: `subscribe(topic)`, `publish(topic, payload)`,
incoming-message stream) so A and B are interchangeable and testable with a mock.

---

## 5. Crate layout

```
nucleus-job-plugin/
├── Cargo.toml                 # Apache-2.0 OR MIT
├── LICENSE-APACHE / LICENSE-MIT
├── README.md                  # what it is, the plugin-vs-generic clarification, disclaimer
├── CONTRIBUTING.md / CODE_OF_CONDUCT.md / CHANGELOG.md
├── .github/workflows/ci.yml   # fmt + clippy -D warnings + test
├── src/
│   ├── main.rs                # binary: config, connect transport, run the jobs loop
│   ├── config.rs              # thing name, handler dir, timeouts, transport choice, allow-list
│   ├── transport/
│   │   ├── mod.rs             # JobsTransport trait
│   │   ├── ipc.rs             # via greengrass-ipc (SubscribeToIoTCore/PublishToIoTCore)  [feature]
│   │   └── mqtt.rs            # direct rumqttc                                            [feature]
│   ├── jobs/
│   │   ├── mod.rs
│   │   ├── topics.rs          # $aws/things/{thing}/jobs/... topic builders
│   │   ├── model.rs           # request/response/JobExecutionData JSON shapes (serde)
│   │   └── engine.rs          # the workflow state machine (notify-next → next → in-progress → done)
│   ├── handler.rs             # allow-listed handler execution (spawn, timeout, capture, status map)
│   └── error.rs
├── examples/
│   └── local_run.rs           # run the engine against a mock transport with a sample job document
├── greengrass/                # generic-component packaging (mirror locker-manager's approach)
│   ├── recipe.json            # aws.greengrass.generic; Startup runs the binary; IPC mqttproxy authz
│   └── files/setup.sh         # provision: install binary, handler dir, run-user perms
└── tests/
    ├── engine.rs              # jobs workflow against a mock transport + fake handlers
    └── handler.rs             # allow-list, timeout, exit-code→status mapping
```

---

## 6. Component packaging (generic component)

Follow the pattern proven in `~/reps/pnl-apl/apl-embedded-locker-manager/greengrass`:
- **Native aarch64 binary** shipped in `greengrass/files/` (built via a Fedora toolchain container),
  `filesConfig.executePermissions`.
- Recipe (`aws.greengrass.generic`):
  - `Install` (privileged): install binary to `/usr/local/bin`, create the handler allow-list dir,
    set run-user ownership.
  - `Startup`: run the binary **as the component user** (`ggc_user` by default). Report RUNNING via
    Greengrass IPC (`UpdateState`) so the component shows RUNNING — reuse `greengrass-ipc`.
  - **`accessControl`** granting `aws.greengrass.ipc.mqttproxy` `PublishToIoTCore` +
    `SubscribeToIoTCore` on `$aws/things/<thing>/jobs/*` (required for transport option A).
  - Component configuration for: handler dir, per-job timeout, allow-list policy, transport choice.
- Reuse the lessons learned from locker-manager: create runtime dirs for the non-root user in the
  privileged Install step; keep the binary at `/usr/local/bin`.

---

## 7. Testing & validation

1. **Unit:** topic builders; job model (de)serialization vs captured AWS payloads; handler exit-code
   → status mapping; allow-list enforcement; timeout handling.
2. **Engine tests:** drive the workflow state machine with a **mock transport** (canned
   notify-next/start-next/update responses) and fake handler scripts — full coverage without AWS.
3. **Real hardware:** deploy the generic component to the prototype board `root@192.168.2.95` via the
   manual AWS-CLI dev flow (dev account `590183682129`, region eu-west-1, thing group
   `EduardosDynamicTestGroup`, thing `fp_777007_000000007bac5a8e`, component bucket
   `dev-aple-ggv2-components`, profile `pnl-apl-dev-elevated`). Then, from the AWS console / CLI,
   **create an IoT Job** targeting that thing with a small job document that invokes an allow-listed
   handler (e.g. a script that touches a file), and verify the runner: picks it up, runs the handler,
   and the job reaches `SUCCEEDED` in the cloud.

---

## 8. Sequencing (suggested session order)

1. **Scaffold**: Cargo, dual license, README (+ plugin-vs-generic clarification + disclaimer), CI,
   CONTRIBUTING, CoC, CHANGELOG.
2. **Job model + topics + engine** against a **mock transport** (no network) — the core value, fully
   unit-tested.
3. **Handler execution** (allow-list, spawn, timeout, capture, status mapping) + tests.
4. **Transport A (Greengrass IPC)** using `greengrass-ipc` (or **Transport B (rumqttc)** first if the
   SDK's IoT-Core ops aren't ready). Verify reserved `$aws/jobs` topics work over the chosen path.
5. **Generic-component packaging** (recipe + setup.sh) mirroring locker-manager; RUNNING via IPC.
6. **Dogfood** on the board with a real IoT Job; iterate.

---

## 9. Dependencies (initial)

- `tokio` (rt, macros, process, time, sync)
- `serde`, `serde_json`
- `greengrass-ipc` (sibling crate; for IPC transport + UpdateState RUNNING)  — path/git dep
- `rumqttc` (fallback direct-MQTT transport)  — feature-gated
- `thiserror`, `tracing`
- dev: mock transport + `tempfile` for handler tests

---

## 10. Open items to confirm at kickoff

- Transport priority: build IPC transport first (needs greengrass-ipc Tier 2), or direct-MQTT
  (`rumqttc`) first so this project can progress independently?
- Exact device-client job-document schema to accept (confirm `operation`/`handler`/`args`/`path`
  field names + allow-list dir semantics from device-client source/docs).
- Whether to also support the "jobs with file download" convenience — **out of scope** for v0.1
  (execution only), but note the boundary in the README.
- Run-user + handler-directory security policy (ownership, perms, allow-list location).
- Final crate/binary name.

---

## 11. References

- Jobs MQTT API: https://docs.aws.amazon.com/iot/latest/developerguide/jobs-mqtt-api.html
- Devices & Jobs workflow: https://docs.aws.amazon.com/iot/latest/developerguide/jobs-devices.html
- Job document format: https://docs.aws.amazon.com/iot/latest/developerguide/iot-jobs.html
- aws-iot-device-client (Apache-2.0) Jobs feature: https://github.com/awslabs/aws-iot-device-client
  (see `source/jobs` and `docs/`)
- Greengrass IPC IoT Core pub/sub: https://docs.aws.amazon.com/greengrass/v2/developerguide/ipc-iot-core-mqtt.html
- Greengrass component types (generic vs plugin): https://docs.aws.amazon.com/greengrass/v2/developerguide/develop-greengrass-components.html
- Nucleus Jobs reference (local Java clone): `~/reps/du7/aws-greengrass-nucleus`
  → `src/main/java/com/aws/greengrass/deployment/IotJobsHelper.java`, `IotJobsClientWrapper.java`
- rumqttc (MQTT client, tokio): https://crates.io/crates/rumqttc
- rustot (reference Jobs state machine, no-std): https://github.com/BlackbirdHQ/rustot
- Sibling SDK: `../greengrass-ipc`
- Component packaging reference: `~/reps/pnl-apl/apl-embedded-locker-manager/greengrass`

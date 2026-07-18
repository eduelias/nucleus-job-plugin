# AWS IoT Jobs MQTT protocol — captured reference for nucleus-job-plugin

> **Source of truth for the Jobs wire protocol.** Transcribed from the AWS IoT docs
> (jobs-mqtt-api.html, reserved-topics). Implement against this; verify with the mock transport and a
> real IoT Job.

## 1. Topics

All prefixed with `$aws/things/{thingName}/jobs/`. Every request accepts an optional `clientToken`
(echoed in the response) for correlation. Responses arrive on `.../accepted` and `.../rejected`.
**The broker publishes the accepted/rejected responses to the requesting client even without an
explicit subscription — but the client must be actively listening (subscribed) to receive them.**

| Operation | Publish (request) | Response topics |
|---|---|---|
| GetPendingJobExecutions | `get` | `get/accepted`, `get/rejected` |
| StartNextPendingJobExecution | `start-next` | `start-next/accepted`, `start-next/rejected` |
| DescribeJobExecution | `{jobId}/get` (use `$next` for the next job) | `{jobId}/get/accepted`, `{jobId}/get/rejected` |
| UpdateJobExecution | `{jobId}/update` | `{jobId}/update/accepted`, `{jobId}/update/rejected` |
| JobExecutionsChanged (notify) | subscribe `notify` | — |
| NextJobExecutionChanged (notify-next) | subscribe `notify-next` | — |

QoS: use **QoS 1 (AT_LEAST_ONCE)** for all Jobs pub/sub (matches the AWS device SDKs).

## 2. Request/response payloads

### GetPendingJobExecutions
Request: `{ "clientToken": "string" }`
Response (`get/accepted`):
```json
{
  "inProgressJobs": [ JobExecutionSummary, ... ],
  "queuedJobs":     [ JobExecutionSummary, ... ],
  "timestamp": 1489096425069,
  "clientToken": "client-001"
}
```

### StartNextPendingJobExecution
Request:
```json
{ "statusDetails": { "key": "value" }, "stepTimeoutInMinutes": 10, "clientToken": "string" }
```
Response (`start-next/accepted`):
```json
{ "execution": JobExecutionData, "timestamp": 1489088524284, "clientToken": "string" }
```
If **no** job is pending, the `execution` field is **omitted**.

### DescribeJobExecution
Request:
```json
{ "jobId": "022", "thingName": "MyThing", "executionNumber": 1,
  "includeJobDocument": true, "clientToken": "string" }
```
`jobId` may be `$next`. `includeJobDocument` defaults to `true`.
Response (`{jobId}/get/accepted`): `{ "execution": JobExecutionData, "timestamp": ..., "clientToken": ... }`

### UpdateJobExecution
Request:
```json
{
  "status": "IN_PROGRESS" | "SUCCEEDED" | "FAILED" | "REJECTED",
  "statusDetails": { "key": "value" },
  "expectedVersion": 1,
  "executionNumber": 1,
  "includeJobExecutionState": false,
  "includeJobDocument": false,
  "stepTimeoutInMinutes": 10,
  "clientToken": "string"
}
```
- `status` is **required on every update**.
- `expectedVersion` gives optimistic concurrency: a mismatch is rejected with a `VersionMismatch`
  error (the rejected response includes the current execution state so you don't need to re-describe).
Response (`{jobId}/update/accepted`):
```json
{ "executionState": JobExecutionState, "jobDocument": {...}, "timestamp": ..., "clientToken": ... }
```

## 3. Notify payloads (subscribe-only)

### JobExecutionsChanged — topic `notify`
```json
{ "jobs": { "IN_PROGRESS": [ JobExecutionSummary, ... ], "QUEUED": [ ... ] }, "timestamp": ... }
```

### NextJobExecutionChanged — topic `notify-next`
```json
{ "execution": JobExecutionData, "timestamp": ... }
```
Sent when the job that `DescribeJobExecution($next)` would return changes. If there is no next job,
`execution` is omitted. **This is the primary trigger** for the runner to pick up work.

## 4. Data shapes

### JobExecutionData (in start-next / describe / notify-next responses)
```json
{
  "jobId": "022",
  "thingName": "MyThing",
  "jobDocument": { ... },           // JSON object over MQTT (string over HTTP)
  "status": "QUEUED"|"IN_PROGRESS"|"SUCCEEDED"|"FAILED"|"TIMED_OUT"|"REJECTED"|"REMOVED"|"CANCELED",
  "statusDetails": { "key": "value" },
  "queuedAt": 1489096123309,        // epoch seconds/millis
  "startedAt": 1489096123309,
  "lastUpdatedAt": 1489096123309,
  "versionNumber": 1,
  "executionNumber": 1234567890
}
```

### JobExecutionState
```json
{ "status": "IN_PROGRESS", "statusDetails": { "key": "value" }, "versionNumber": 2 }
```

### JobExecutionSummary
```json
{ "jobId": "022", "queuedAt": ..., "startedAt": ..., "lastUpdatedAt": ..., "versionNumber": 1, "executionNumber": 1 }
```

### Job statuses
`QUEUED`, `IN_PROGRESS`, `SUCCEEDED`, `FAILED`, `TIMED_OUT`, `REJECTED`, `REMOVED`, `CANCELED`.
Device sets: `IN_PROGRESS` (on pickup), then `SUCCEEDED` or `FAILED` (or `REJECTED` if it declines).

## 5. Error responses (on `.../rejected`)
```json
{ "code": "InvalidRequest"|"InvalidStateTransition"|"ResourceNotFound"|"VersionMismatch"|"InternalError"|"RequestThrottled"|"TerminalStateReached"|"InvalidJson"|"...",
  "message": "string", "clientToken": "string", "timestamp": ...,
  "executionState": JobExecutionState /* present on VersionMismatch */ }
```

## 6. Job document (device-client-compatible)

The **job document** is an arbitrary JSON object defined by whoever creates the job. To interoperate
with the `aws-iot-device-client` Jobs feature, accept its schema — representative fields:

```json
{
  "version": "1.0",
  "steps": [
    {
      "action": {
        "name": "my-handler",
        "type": "runHandler",
        "input": {
          "handler": "my-handler.sh",     // executable name (must be in the allow-list dir)
          "args": ["arg1", "arg2"],
          "path": "default"                // handler dir; "default" = configured allow-list dir
        },
        "runAsUser": "optional-user"
      }
    }
  ]
}
```

> Confirm the exact device-client schema (field names `operation`/`handler`/`args`/`path`, the
> default handler directory, `allowStdErr`/`includeStdOut`) from the device-client source/docs before
> finalizing. Our own struct should be thin and documented; we control it. For our runner, a simpler
> single-action document is also fine — keep parsing lenient.

Handler execution contract:
- Only run handlers found in the configured **allow-list directory** (never an arbitrary path from
  the job document).
- Enforce a bounded timeout (job's `stepTimeoutInMinutes` or a configured default).
- Capture stdout/stderr + exit code. Exit 0 → `SUCCEEDED`; non-zero/timeout → `FAILED`/`TIMED_OUT`,
  with reason + captured stderr placed in `statusDetails`.

## 7. Sources
- Jobs MQTT API: https://docs.aws.amazon.com/iot/latest/developerguide/jobs-mqtt-api.html
- Reserved topics: https://docs.aws.amazon.com/iot/latest/developerguide/reserved-topics.html
- Devices & Jobs: https://docs.aws.amazon.com/iot/latest/developerguide/jobs-devices.html
- Job document: https://docs.aws.amazon.com/iot/latest/developerguide/iot-jobs.html
- aws-iot-device-client (Apache-2.0) Jobs feature: https://github.com/awslabs/aws-iot-device-client
- Nucleus Jobs reference (local Java clone): `~/reps/du7/aws-greengrass-nucleus`
  → `src/main/java/com/aws/greengrass/deployment/IotJobsHelper.java`

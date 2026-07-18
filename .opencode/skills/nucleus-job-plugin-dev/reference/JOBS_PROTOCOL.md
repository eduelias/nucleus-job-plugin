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

## 6. Job document (device-client-compatible + AWS managed templates)

The **job document** is a JSON object. This runner supports the `aws-iot-device-client` Jobs schema,
which is also what **AWS managed job templates** emit. There are two action **types**:

### 6a. `runHandler` (most managed templates + custom handlers)

```json
{
  "version": "1.0",
  "steps": [
    {
      "action": {
        "name": "Download-File",
        "type": "runHandler",
        "input": {
          "handler": "download-file.sh",   // executable file name (resolved in the handler dir)
          "args": ["https://…", "/opt/f"], // positional args
          "path": ""                        // OPTIONAL handler-dir override; empty => configured dir
        },
        "runAsUser": ""                      // OPTIONAL user to drop to; empty => component user
      }
    }
  ]
}
```

Also accepted (flat, non-managed convenience form): `{ "operation"|"handler": "h.sh", "args": [...] }`.

### 6b. `runCommand` (the `AWS-Run-Command` template)

```json
{
  "version": "1.0",
  "steps": [
    {
      "action": {
        "name": "Run-Command",
        "type": "runCommand",
        "input": {
          "command": "sudo,systemctl,restart,my.service"  // COMMA-separated argv; commas in an
                                                            // argument are escaped as "\\,"
        },
        "runAsUser": ""
      }
    }
  ]
}
```

`runCommand.input.command` is a **comma-separated argv list** (per the AWS template spec), NOT a shell
string. Split on unescaped commas; unescape `\,` → `,`. Execute argv[0] with argv[1..] directly (no
shell), so there is no shell-injection surface.

### Parameter substitution

Managed templates contain `${aws:iot:parameter:<name>}` placeholders (e.g. `downloadUrl`,
`pathToHandler`, `runAsUser`). **AWS substitutes these server-side** before the document reaches the
device, so the runner sees concrete values. Unset optional parameters arrive as **empty strings**
(treat empty `path`/`runAsUser` as "unset").

### AWS managed templates → handler scripts

| Template | type | handler / command |
|---|---|---|
| `AWS-Download-File` | runHandler | `download-file.sh <downloadUrl> <filePath>` |
| `AWS-Install-Application` | runHandler | `install-packages.sh <packages>` |
| `AWS-Remove-Application` | runHandler | `remove-packages.sh <packages>` |
| `AWS-Start-Application` | runHandler | `start-services.sh <services>` |
| `AWS-Stop-Application` | runHandler | `stop-services.sh <services>` |
| `AWS-Restart-Application` | runHandler | `restart-services.sh <services>` |
| `AWS-Reboot` | runHandler | `reboot.sh` |
| `AWS-Run-Command` | runCommand | the provided command argv |

The `runHandler` scripts are the AWS "sample job handlers" (Apache-2.0, from `aws-iot-device-client`)
and are shipped in `greengrass/files/handlers/` so managed templates work out of the box.

### Execution contract

- **runHandler**: resolve the handler as a **bare file name inside the handler directory** (the
  configured allow-list dir, or a `path` override only when it is on the configured allow-list of
  permitted directories). Reject names/paths containing `..` or separators that escape the dir.
- **runCommand**: parse the comma-separated argv; run argv directly (no shell). The executable may be
  subject to a configurable allow-list/`deny` policy.
- **runAsUser** (both types): when the runner is running as root and `runAsUser` is non-empty, drop
  privileges to that user's uid/gid for the child process (resolve via the passwd database). If the
  runner is not root, `runAsUser` is ignored with a warning.
- Enforce a bounded timeout (job's `stepTimeoutInMinutes` or the configured default).
- Capture stdout/stderr + exit code. Exit 0 → `SUCCEEDED`; non-zero → `FAILED`; over budget →
  `TIMED_OUT`. Put reason + captured stderr (and optionally stdout) in `statusDetails`.

## 7. Sources
- Jobs MQTT API: https://docs.aws.amazon.com/iot/latest/developerguide/jobs-mqtt-api.html
- Reserved topics: https://docs.aws.amazon.com/iot/latest/developerguide/reserved-topics.html
- Devices & Jobs: https://docs.aws.amazon.com/iot/latest/developerguide/jobs-devices.html
- Job document: https://docs.aws.amazon.com/iot/latest/developerguide/iot-jobs.html
- AWS managed job templates: https://docs.aws.amazon.com/iot/latest/developerguide/job-templates-managed.html
- Sample job handlers (Apache-2.0): https://github.com/awslabs/aws-iot-device-client/tree/main/sample-job-handlers
- aws-iot-device-client (Apache-2.0) Jobs feature: https://github.com/awslabs/aws-iot-device-client
- Nucleus Jobs reference (local Java clone): `~/reps/du7/aws-greengrass-nucleus`
  → `src/main/java/com/aws/greengrass/deployment/IotJobsHelper.java`

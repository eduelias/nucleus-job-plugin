# nucleus-job-plugin

An **unofficial**, community **Rust runner for AWS IoT Jobs** on a Greengrass device. It mirrors the
*Jobs* feature of the [`aws-iot-device-client`](https://github.com/awslabs/aws-iot-device-client) —
**job execution only**.

> ⚠️ **Not affiliated with, endorsed by, or sponsored by Amazon.** "AWS IoT", "Greengrass", and
> "IoT Jobs" are used descriptively/nominatively.

## What it does

- Speaks the **AWS IoT Jobs MQTT protocol** (`$aws/things/{thingName}/jobs/...`).
- Waits for the next queued job (via `notify-next` / `start-next`), marks it `IN_PROGRESS`, runs an
  **allow-listed handler** named by the job document, and reports `SUCCEEDED` / `FAILED` /
  `TIMED_OUT` back to the cloud.
- Runs as a **generic Greengrass component** — its own supervised process.

### Naming note

Despite the folder name, a Rust program **cannot** be a Greengrass nucleus *plugin*: the
`aws.greengrass.plugin` component type runs inside the nucleus JVM and is **Java only**. This project
ships a standalone binary as a **generic component** (`aws.greengrass.generic`).

## Scope

**In scope:** IoT Jobs *execution* — pick up a job, run a handler, report status.

**Out of scope** (by design): OTA / jobs-with-file-download, fleet provisioning, secure tunneling,
device defender, config shadow. This mirrors *only* the device client's Jobs feature.

## Architecture

| Module | Responsibility |
|---|---|
| `jobs::model` | JSON shapes for the Jobs protocol + job-document parsing |
| `jobs::topics` | reserved `$aws/things/{thing}/jobs/...` topic builders |
| `jobs::engine` | the workflow state machine |
| `handler` | allow-listed handler execution (spawn, timeout, capture, status map) |
| `transport` | the `JobsTransport` trait + a direct-MQTT impl (`rumqttc`) and an in-memory mock |

The engine is transport-agnostic, so the whole workflow is unit-tested with a mock transport and fake
handler scripts — no network or AWS account required.

## Job document

Two forms are accepted:

```jsonc
// flat
{ "operation": "my-handler.sh", "args": ["arg1", "arg2"] }

// stepped (aws-iot-device-client style)
{ "steps": [ { "action": { "input": { "handler": "my-handler.sh", "args": ["arg1"] } } } ] }
```

The handler name must be a **bare file name** present in the configured allow-list directory. Names
containing path separators or `..` are rejected. Exit `0` → `SUCCEEDED`; non-zero → `FAILED`; over the
timeout → `TIMED_OUT`, with the reason and captured `stderr` reported in `statusDetails`.

## Configuration (environment)

| Variable | Default | Meaning |
|---|---|---|
| `THING_NAME` | *(required)* | thing name / MQTT client id / topic segment |
| `HANDLER_DIR` | `/var/lib/nucleus-job-plugin/handlers` | allow-list directory of handlers |
| `JOB_TIMEOUT_SECS` | `300` | default per-job handler timeout |
| `INCLUDE_STDOUT` | *(off)* | `1`/`true` to include stdout in `statusDetails` |
| `IOT_ENDPOINT` | *(required for MQTT)* | `xxxx-ats.iot.<region>.amazonaws.com` |
| `IOT_PORT` | `8883` | MQTT port |
| `CERT_PATH` / `KEY_PATH` / `CA_PATH` | *(required for MQTT)* | device cert, key, Amazon Root CA |

## Try it locally (no AWS)

```bash
cargo run --example local_run
```

Feeds a canned job through the mock transport, runs a temp handler, and prints what the engine
publishes.

## Deploying as a Greengrass component

See [`greengrass/recipe.json`](greengrass/recipe.json) and
[`greengrass/files/setup.sh`](greengrass/files/setup.sh). The recipe runs the binary as the component
user and, for the direct-MQTT transport, you supply the device credentials via configuration/env.

## License

Dual-licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at your option.

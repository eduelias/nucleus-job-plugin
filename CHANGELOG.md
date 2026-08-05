# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.7]

### Fixed

- **Component never reached `RUNNING` and every deployment failed.** The recipe declared the
  long-running runner under the `Startup` lifecycle key. Greengrass requires a `Startup` script to
  *exit 0* within its timeout (default 120s) to signal that startup completed; `Run` is the key for
  a foreground long-running process. Because the runner blocks forever in the jobs engine loop, the
  nucleus killed it with `SIGTERM` after 120s (`STARTUP_TIMEOUT`, exit code 143), retried, and then
  marked the service `BROKEN`, failing the deployment with `ServiceUpdateException`. Moved the
  lifecycle step from `Startup` to `Run`.
- **`mqttproxy` authorization never matched, so every subscribe failed with
  `aws.greengrass#UnauthorizedError: Not Authorized`.** The `accessControl` resources used the
  shell-style `${iot:thingName}`; Greengrass recipe variables are `{iot:thingName}`, with no leading
  `$` (the `Setenv` block in the same manifest already used the correct form). The policy resource
  therefore stayed the uninterpolated literal `$aws/things/${iot:thingName}/jobs/*` and never matched
  the real reserved topics. This was masked by the `Startup` bug above, since the runner was killed
  before it ever reached its first subscribe.
- **`greengrass/deploy.sh` silently removed every other component on the target.** A Greengrass
  deployment defines the complete component set for its target, and the script always created one
  containing just this component, so any other component previously deployed to the same thing or
  thing group was uninstalled. It now reads the target's current deployment and merges into it, the
  way the console does when revising a deployment. `MERGE=0` restores the old behaviour.

### Changed

- **The project no longer pins an AWS region.** `gdk-config.json` previously hardcoded
  `publish.region`, which published to that region regardless of where the target core devices
  actually ran. The key is gone; the region is now supplied per invocation via
  `gdk component publish --region`, or through the new `greengrass/publish.sh` wrapper, which reads
  `AWS_REGION` (falling back to the profile's configured region).
- `greengrass/publish.sh` added. It also accepts `GDK_BUCKET` to upload artifacts to an existing
  bucket verbatim, for organisations whose Greengrass artifact bucket is not named the way GDK
  would generate it (GDK appends `-<region>-<account>` to the `publish.bucket` prefix, but uses
  `--bucket` as given).
- `greengrass/deploy.sh` now derives the component name from `gdk-config.json`, derives the account
  and partition from the caller (so it works in `aws`, `aws-us-gov` and `aws-cn`), accepts a thing
  group via `TARGET_TYPE=thinggroup` or a full ARN, and falls back to the profile's region when
  `AWS_REGION` is unset.

### Added

- Initial release: a Rust runner for AWS IoT Jobs execution as a Greengrass generic component.
  - IoT Jobs MQTT protocol model + reserved-topic builders.
  - Workflow engine (state machine): `notify-next` / `start-next` → `IN_PROGRESS` → run action →
    terminal status, with optimistic-concurrency (`expectedVersion`) updates.
  - **AWS managed job templates** support: all `AWS-*` managed templates work out of the box.
    - `runHandler` action with `handler`/`args`/`path`/`runAsUser` (device-client convention:
      `runAsUser` is passed as the handler's first argument).
    - `runCommand` action (the `AWS-Run-Command` template): comma-separated argv, run without a shell,
      with an optional executable allow-list and native uid/gid privilege drop when running as root.
    - Bundled AWS sample job handlers (Apache-2.0) installed by the component so managed templates
      run immediately.
  - Allow-listed handler execution: bare-name resolution inside a configured directory, path-traversal
    rejection, allow-listed `path` overrides, bounded timeout, output capture, exit-code → status
    mapping.
  - `JobsTransport` trait with a Greengrass-IPC implementation (default, feature `ipc`, via the
    `greengrass-ipc` SDK — reuses the nucleus's MQTT connection, no device cert), a direct-MQTT
    implementation (`rumqttc`, feature `mqtt`), and an in-memory mock for tests.
  - Engine hardening: per-session job de-duplication so a job is processed exactly once even though
    each status update re-triggers `notify-next`; terminal updates omit `expectedVersion` to avoid
    `VersionMismatch` leaving a job stuck `IN_PROGRESS`.
  - Generic component `dev.du7.nucleus-job-plugin` with a soft nucleus dependency, `mqttproxy`
    authorization for the reserved jobs topics, environment-based configuration, `local_run` example,
    and an Install script that provisions the binary + handler directory + sample handlers.
  - GDK CLI packaging (`gdk-config.json`, root `recipe.json` template,
    `greengrass/build-custom.sh` for containerized aarch64 cross-compilation) plus a
    `greengrass/deploy.sh` helper for `create-deployment` + on-device verification.

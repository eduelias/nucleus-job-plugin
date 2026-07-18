# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
  - `JobsTransport` trait with a direct-MQTT implementation (`rumqttc`, feature `mqtt`) and an
    in-memory mock for tests.
  - Generic component `dev.du7.nucleus-job-plugin` with a soft nucleus dependency, `mqttproxy`
    authorization for the reserved jobs topics, environment-based configuration, `local_run` example,
    and an Install script that provisions the binary + handler directory + sample handlers.

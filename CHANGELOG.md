# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Initial release: a Rust runner for AWS IoT Jobs execution as a Greengrass generic component.
  - IoT Jobs MQTT protocol model + reserved-topic builders.
  - Workflow engine (state machine): `notify-next` / `start-next` → `IN_PROGRESS` → run handler →
    terminal status, with optimistic-concurrency (`expectedVersion`) updates.
  - Allow-listed handler execution: bare-name resolution inside a configured directory, path-traversal
    rejection, bounded timeout, output capture, exit-code → status mapping.
  - `JobsTransport` trait with a direct-MQTT implementation (`rumqttc`, feature `mqtt`) and an
    in-memory mock for tests.
  - Environment-based configuration, `local_run` example, and a generic-component recipe + setup script.

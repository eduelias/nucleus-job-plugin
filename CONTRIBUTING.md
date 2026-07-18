# Contributing to nucleus-job-plugin

Thanks for your interest! This is an unofficial, community project. Contributions are welcome.

## Development

- Rust stable (MSRV 1.94.1). Install via [rustup](https://rustup.rs/).
- Run all checks locally before opening a PR:

  ```bash
  ./scripts/check.sh
  ```

  This runs `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, and `cargo doc` — the same
  gates as CI.

## Guidelines

- **Do not guess Jobs wire details.** The IoT Jobs MQTT topics and payload shapes are captured, with
  sources, in `.opencode/skills/nucleus-job-plugin-dev/reference/JOBS_PROTOCOL.md`. Implement against
  it and match JSON field names exactly.
- **Keep the scope tight:** IoT Jobs *execution* only. No OTA/file-download jobs, provisioning,
  tunneling, or defender.
- **Handler execution must stay safe:** allow-list directory only, bounded timeout, captured output,
  never run an arbitrary path from a job document. Add tests for any change here.
- Cover engine changes with tests driven by the **mock transport**; cover handler changes with fake
  handler scripts in a temp dir. No test may require network or an AWS account.
- Keep the public API documented; `cargo doc` must be warning-free.

## Adding a transport

Implement the `JobsTransport` trait in `src/transport/` behind a cargo feature. The engine, model, and
handler are transport-agnostic and should not change.

## Licensing

By contributing you agree your contribution is dual-licensed under Apache-2.0 OR MIT.

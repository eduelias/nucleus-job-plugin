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

## Component build / deploy (GDK)

Packaging uses the [GDK CLI](https://docs.aws.amazon.com/greengrass/v2/developerguide/greengrass-development-kit-cli.html)
(`gdk-config.json`). The custom build (`greengrass/build-custom.sh`) cross-compiles the aarch64 binary
in a container (`podman` by default; `CONTAINER_ENGINE=docker` to override) and stages the binary,
`setup.sh`, and the sample-handlers zip into `greengrass-build/`.

```bash
gdk component build                                   # cross-compile + stage
gdk component publish --bucket <artifact-bucket>      # upload + create next version
greengrass/deploy.sh <version> <thing-name>           # deploy + wait for RUNNING
```

`recipe.json` at the repo root is the GDK recipe template (`{COMPONENT_VERSION}` and the S3
`BUCKET_NAME`/`COMPONENT_VERSION` tokens are filled in at publish). `greengrass-build/` is generated
and git-ignored.

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

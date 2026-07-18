# nucleus-job-plugin

A **Rust runner for AWS IoT Jobs** on a Greengrass device — mimicking the AWS
`aws-iot-device-client` **Jobs feature**, but *only* job execution (no OTA, no secure tunneling, no
device defender). Community, unofficial (not affiliated with or endorsed by Amazon).

> ⚠️ **Naming note:** the folder is called `nucleus-job-plugin`, but in Greengrass terms a Rust
> program **cannot** be a nucleus "plugin" — the `aws.greengrass.plugin` component type runs *inside
> the nucleus JVM* and is Java-only. This project is a **generic Greengrass component**
> (`aws.greengrass.generic`) that the nucleus launches as its own process. See the plan for details.
> Keep the folder name, but the artifact is a standalone Rust binary + a generic-component recipe.

## For AI agents / new sessions

- Read [`.opencode/PLAN.md`](.opencode/PLAN.md) first — authoritative implementation plan.
- **Greenfield.** Nothing implemented yet.
- **Depends conceptually on the sibling project** `../greengrass-ipc`: the recommended way for this
  runner to reach AWS IoT Core (to speak the Jobs MQTT protocol) is **through Greengrass IPC**
  (`SubscribeToIoTCore` / `PublishToIoTCore`), i.e. reuse the nucleus's existing MQTT connection
  instead of opening its own. That functionality is Tier 2 of `greengrass-ipc`. Build order therefore
  favors doing `greengrass-ipc` (at least Tier 2 IoT-Core ops) first, OR temporarily using a direct
  MQTT connection with the device's own cert while `greengrass-ipc` matures.

## Ground rules

- **License:** dual `Apache-2.0 OR MIT`. Ship `LICENSE-APACHE` + `LICENSE-MIT`.
- **Naming/trademark:** no `aws-` crate prefix; state "unofficial, not affiliated with Amazon" in the
  README; "IoT Jobs"/"Greengrass" used descriptively.
- **Scope discipline:** ONLY IoT Jobs execution. Explicitly out of scope: OTA/jobs-with-file-download,
  fleet provisioning, secure tunneling, device defender, config shadow — mirror only the device
  client's *Jobs* feature.
- Open-source standards: CI, CONTRIBUTING, CoC, CHANGELOG, examples, docs.

//! # nucleus-job-plugin
//!
//! An **unofficial**, community Rust runner for **AWS IoT Jobs** on a Greengrass
//! device. It mirrors the *Jobs* feature of the `aws-iot-device-client` — **job
//! execution only** (no OTA/file-download jobs, fleet provisioning, secure
//! tunneling, or device defender).
//!
//! > Not affiliated with, endorsed by, or sponsored by Amazon. "AWS IoT",
//! > "Greengrass", and "IoT Jobs" are used descriptively.
//!
//! ## Naming
//!
//! Despite the name, a Rust program **cannot** be a Greengrass nucleus *plugin*
//! (the `aws.greengrass.plugin` type runs inside the nucleus JVM and is Java
//! only). This crate builds a standalone binary shipped as a **generic Greengrass
//! component** (`aws.greengrass.generic`).
//!
//! ## Architecture
//!
//! * [`jobs::model`] — JSON shapes for the IoT Jobs MQTT protocol + job document.
//! * [`jobs::topics`] — reserved `$aws/things/{thing}/jobs/...` topic builders.
//! * [`jobs::engine`] — the workflow state machine.
//! * [`handler`] — allow-listed handler execution (spawn, timeout, status map).
//! * [`transport`] — the [`transport::JobsTransport`] trait and implementations
//!   (direct MQTT via `rumqttc`, plus an in-memory mock for tests).

pub mod config;
pub mod error;
pub mod handler;
pub mod jobs;
pub mod transport;

pub use config::Config;
pub use error::{Error, Result};
pub use jobs::Engine;

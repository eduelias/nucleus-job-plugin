//! Runtime configuration for the jobs runner.

use crate::error::{Error, Result};
use std::path::PathBuf;
use std::time::Duration;

/// Which transport to use to reach AWS IoT Core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportKind {
    /// Direct MQTT over TLS using the device certificate.
    Mqtt,
}

/// Direct-MQTT connection settings.
#[derive(Debug, Clone)]
pub struct MqttConfig {
    /// AWS IoT Core data endpoint (`xxxx-ats.iot.<region>.amazonaws.com`).
    pub endpoint: String,
    /// MQTT port (usually 8883).
    pub port: u16,
    /// Path to the device certificate (PEM).
    pub cert_path: PathBuf,
    /// Path to the device private key (PEM).
    pub key_path: PathBuf,
    /// Path to the Amazon Root CA (PEM).
    pub ca_path: PathBuf,
}

/// Full runner configuration.
#[derive(Debug, Clone)]
pub struct Config {
    /// The thing name (MQTT client id and Jobs topic segment).
    pub thing_name: String,
    /// Directory of allow-listed handler executables.
    pub handler_dir: PathBuf,
    /// Default per-job handler timeout.
    pub default_timeout: Duration,
    /// Whether captured stdout is reported in `statusDetails`.
    pub include_stdout: bool,
    /// Transport selection.
    pub transport: TransportKind,
    /// MQTT settings (required when `transport == Mqtt`).
    pub mqtt: Option<MqttConfig>,
}

impl Config {
    /// Build configuration from environment variables.
    ///
    /// * `THING_NAME` (required)
    /// * `HANDLER_DIR` (default `/var/lib/nucleus-job-plugin/handlers`)
    /// * `JOB_TIMEOUT_SECS` (default 300)
    /// * `INCLUDE_STDOUT` (`1`/`true` to enable)
    /// * MQTT: `IOT_ENDPOINT`, `IOT_PORT` (default 8883), `CERT_PATH`, `KEY_PATH`, `CA_PATH`
    pub fn from_env() -> Result<Self> {
        let thing_name =
            env("THING_NAME").ok_or_else(|| Error::Config("THING_NAME is required".into()))?;
        let handler_dir = env("HANDLER_DIR")
            .unwrap_or_else(|| "/var/lib/nucleus-job-plugin/handlers".to_string())
            .into();
        let default_timeout = Duration::from_secs(
            env("JOB_TIMEOUT_SECS")
                .and_then(|s| s.parse().ok())
                .unwrap_or(300),
        );
        let include_stdout = matches!(env("INCLUDE_STDOUT").as_deref(), Some("1") | Some("true"));

        let mqtt = match env("IOT_ENDPOINT") {
            Some(endpoint) => Some(MqttConfig {
                endpoint,
                port: env("IOT_PORT").and_then(|s| s.parse().ok()).unwrap_or(8883),
                cert_path: require_env("CERT_PATH")?.into(),
                key_path: require_env("KEY_PATH")?.into(),
                ca_path: require_env("CA_PATH")?.into(),
            }),
            None => None,
        };

        Ok(Self {
            thing_name,
            handler_dir,
            default_timeout,
            include_stdout,
            transport: TransportKind::Mqtt,
            mqtt,
        })
    }
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn require_env(key: &str) -> Result<String> {
    env(key).ok_or_else(|| Error::Config(format!("{key} is required")))
}

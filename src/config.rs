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
    /// Extra directories a job's `path` override may point at (comma-separated in
    /// `HANDLER_PATH_OVERRIDES`). The configured `handler_dir` is always allowed.
    pub allowed_path_overrides: Vec<PathBuf>,
    /// Optional allow-list of `runCommand` executables (comma-separated in
    /// `COMMAND_ALLOW_LIST`). When unset, any command is permitted.
    pub command_allow_list: Option<Vec<String>>,
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
    /// * `HANDLER_PATH_OVERRIDES` (comma-separated extra dirs a job `path` may use)
    /// * `COMMAND_ALLOW_LIST` (comma-separated allow-list for `runCommand`)
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
        let allowed_path_overrides = env("HANDLER_PATH_OVERRIDES")
            .map(|s| {
                s.split(',')
                    .map(|p| p.trim())
                    .filter(|p| !p.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        let command_allow_list = env("COMMAND_ALLOW_LIST").map(|s| {
            s.split(',')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect()
        });

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
            allowed_path_overrides,
            command_allow_list,
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

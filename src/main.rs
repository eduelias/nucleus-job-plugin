//! Binary entrypoint: load config, connect the transport, run the jobs engine.

use nucleus_job_plugin::config::{Config, TransportKind};
use nucleus_job_plugin::error::{Error, Result};
use nucleus_job_plugin::handler::HandlerRunner;
use nucleus_job_plugin::jobs::Engine;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nucleus_job_plugin=info".into()),
        )
        .init();

    let cfg = Config::from_env()?;
    info!(
        thing = %cfg.thing_name,
        handler_dir = %cfg.handler_dir.display(),
        "starting nucleus-job-plugin"
    );

    let mut runner = HandlerRunner::new(cfg.handler_dir.clone(), cfg.default_timeout);
    runner.include_stdout = cfg.include_stdout;
    runner.allowed_path_overrides = cfg.allowed_path_overrides.clone();
    runner.command_allow_list = cfg.command_allow_list.clone();

    match cfg.transport {
        TransportKind::Ipc => {
            #[cfg(feature = "ipc")]
            {
                let transport = nucleus_job_plugin::transport::ipc::IpcTransport::connect().await?;
                let engine = Engine::new(transport, &cfg.thing_name, runner);
                engine.run().await?;
            }
            #[cfg(not(feature = "ipc"))]
            {
                let _ = runner;
                return Err(Error::Config(
                    "IPC transport selected but the `ipc` feature is disabled".into(),
                ));
            }
        }
        TransportKind::Mqtt => {
            #[cfg(feature = "mqtt")]
            {
                let mqtt = cfg
                    .mqtt
                    .as_ref()
                    .ok_or_else(|| Error::Config("MQTT transport selected but IOT_ENDPOINT/CERT_PATH/KEY_PATH/CA_PATH not set".into()))?;
                let transport = nucleus_job_plugin::transport::mqtt::MqttTransport::connect(
                    mqtt,
                    &mqtt.client_id,
                )?;
                let engine = Engine::new(transport, &cfg.thing_name, runner);
                engine.run().await?;
            }
            #[cfg(not(feature = "mqtt"))]
            {
                let _ = runner;
                return Err(Error::Config(
                    "MQTT transport selected but the `mqtt` feature is disabled".into(),
                ));
            }
        }
    }

    Ok(())
}

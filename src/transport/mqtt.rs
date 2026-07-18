//! Direct MQTT transport to AWS IoT Core using the device certificate.
//!
//! Backed by [`rumqttc`]. TLS uses the device cert/key and Amazon Root CA.
//! Enabled by the `mqtt` feature.

use super::{Incoming, JobsTransport};
use crate::config::MqttConfig;
use crate::error::{Error, Result};
use async_trait::async_trait;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, TlsConfiguration, Transport};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

/// A direct-MQTT [`JobsTransport`].
pub struct MqttTransport {
    client: AsyncClient,
    rx: Option<mpsc::Receiver<Incoming>>,
}

impl MqttTransport {
    /// Connect to AWS IoT Core using the settings in `cfg`, with `client_id`
    /// (typically the thing name).
    pub fn connect(cfg: &MqttConfig, client_id: &str) -> Result<Self> {
        let ca = std::fs::read(&cfg.ca_path)?;
        let cert = std::fs::read(&cfg.cert_path)?;
        let key = std::fs::read(&cfg.key_path)?;

        let mut opts = MqttOptions::new(client_id, &cfg.endpoint, cfg.port);
        opts.set_keep_alive(Duration::from_secs(30));
        opts.set_transport(Transport::Tls(TlsConfiguration::Simple {
            ca,
            alpn: None,
            client_auth: Some((cert, key)),
        }));

        let (client, mut eventloop) = AsyncClient::new(opts, 32);
        let (tx, rx) = mpsc::channel(64);

        // Drive the event loop; forward publishes to the engine.
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(Event::Incoming(Packet::Publish(p))) => {
                        let msg = Incoming {
                            topic: p.topic,
                            payload: p.payload.to_vec(),
                        };
                        if tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                    Ok(ev) => debug!(?ev, "mqtt event"),
                    Err(e) => {
                        error!(error = %e, "mqtt event loop error; retrying");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
            warn!("mqtt event loop ended");
        });

        Ok(Self {
            client,
            rx: Some(rx),
        })
    }
}

#[async_trait]
impl JobsTransport for MqttTransport {
    async fn subscribe(&self, topic: &str) -> Result<()> {
        self.client
            .subscribe(topic, QoS::AtLeastOnce)
            .await
            .map_err(|e| Error::Transport(e.to_string()))
    }

    async fn publish(&self, topic: &str, payload: Vec<u8>) -> Result<()> {
        self.client
            .publish(topic, QoS::AtLeastOnce, false, payload)
            .await
            .map_err(|e| Error::Transport(e.to_string()))
    }

    fn incoming(&mut self) -> Option<mpsc::Receiver<Incoming>> {
        self.rx.take()
    }
}

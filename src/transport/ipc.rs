//! Greengrass IPC transport: reuse the nucleus's MQTT connection to AWS IoT Core
//! via `SubscribeToIoTCore` / `PublishToIoTCore` (the `greengrass-ipc` SDK).
//!
//! Unlike the direct-MQTT transport this needs **no device certificate** and does
//! not open a second MQTT connection — the component is authorized for the Jobs
//! topics through an `aws.greengrass.ipc.mqttproxy` policy in its recipe. Enabled
//! by the `ipc` feature.

use super::{Incoming, JobsTransport};
use crate::error::{Error, Result};
use async_trait::async_trait;
use futures::StreamExt;
use greengrass_ipc::{Client, QoS};
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

/// A Greengrass-IPC-backed [`JobsTransport`].
pub struct IpcTransport {
    client: Client,
    tx: mpsc::Sender<Incoming>,
    rx: Option<mpsc::Receiver<Incoming>>,
}

impl IpcTransport {
    /// Connect to the Greengrass IPC service using the environment provided by the
    /// nucleus (`AWS_GG_NUCLEUS_DOMAIN_SOCKET_FILEPATH_FOR_COMPONENT` + `SVCUID`).
    pub async fn connect() -> Result<Self> {
        let client = Client::connect_from_env()
            .await
            .map_err(|e| Error::Transport(format!("greengrass ipc connect: {e}")))?;
        let (tx, rx) = mpsc::channel(64);
        Ok(Self {
            client,
            tx,
            rx: Some(rx),
        })
    }
}

#[async_trait]
impl JobsTransport for IpcTransport {
    async fn subscribe(&self, topic: &str) -> Result<()> {
        let stream = self
            .client
            .subscribe_to_iot_core(topic.to_string(), QoS::AtLeastOnce)
            .await
            .map_err(|e| Error::Transport(format!("subscribe_to_iot_core({topic}): {e}")))?;

        // Forward IoT Core messages to the engine's incoming channel.
        let tx = self.tx.clone();
        let topic_owned = topic.to_string();
        tokio::spawn(async move {
            let mut stream = stream;
            while let Some(item) = stream.next().await {
                match item {
                    Ok(msg) => {
                        if let Some(m) = msg.message {
                            let payload = m.payload.map(|b| b.0).unwrap_or_default();
                            if tx
                                .send(Incoming {
                                    topic: m.topic_name,
                                    payload,
                                })
                                .await
                                .is_err()
                            {
                                break; // engine dropped the receiver
                            }
                        }
                    }
                    Err(e) => {
                        error!(topic = %topic_owned, error = %e, "iot core subscription error");
                    }
                }
            }
            warn!(topic = %topic_owned, "iot core subscription stream ended");
        });
        Ok(())
    }

    async fn publish(&self, topic: &str, payload: Vec<u8>) -> Result<()> {
        debug!(topic = %topic, "publishing via ipc");
        self.client
            .publish_to_iot_core(topic.to_string(), QoS::AtLeastOnce, payload)
            .await
            .map_err(|e| Error::Transport(format!("publish_to_iot_core({topic}): {e}")))
    }

    fn incoming(&mut self) -> Option<mpsc::Receiver<Incoming>> {
        self.rx.take()
    }
}

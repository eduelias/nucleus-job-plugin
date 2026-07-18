//! Transport abstraction: how the runner reaches AWS IoT Core to speak the Jobs
//! MQTT protocol.
//!
//! The engine is written entirely against the [`JobsTransport`] trait so it can be
//! driven by a real MQTT connection, a Greengrass IPC IoT-Core connection, or a
//! mock in tests.

use crate::error::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

#[cfg(feature = "mqtt")]
pub mod mqtt;

#[cfg(feature = "ipc")]
pub mod ipc;

pub mod mock;

/// An MQTT message received from a subscribed topic.
#[derive(Debug, Clone)]
pub struct Incoming {
    /// Topic the message arrived on.
    pub topic: String,
    /// Raw payload bytes (JSON).
    pub payload: Vec<u8>,
}

/// A pub/sub transport to AWS IoT Core.
///
/// Implementations must deliver every message for subscribed topics to the
/// receiver returned by [`JobsTransport::incoming`]. All Jobs traffic uses QoS 1.
#[async_trait]
pub trait JobsTransport: Send + Sync {
    /// Subscribe to a topic (QoS 1).
    async fn subscribe(&self, topic: &str) -> Result<()>;

    /// Publish `payload` to `topic` (QoS 1).
    async fn publish(&self, topic: &str, payload: Vec<u8>) -> Result<()>;

    /// Take the receiver of incoming messages.
    ///
    /// Called once by the engine after construction. Returns `None` if already taken.
    fn incoming(&mut self) -> Option<mpsc::Receiver<Incoming>>;
}

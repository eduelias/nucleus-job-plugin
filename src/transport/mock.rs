//! In-memory mock transport for tests and the `local_run` example.
//!
//! It records published messages and lets a test inject incoming messages,
//! optionally auto-responding to Jobs requests with canned payloads.

use super::{Incoming, JobsTransport};
use crate::error::Result;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// A single recorded publish.
#[derive(Debug, Clone)]
pub struct Published {
    /// Topic published to.
    pub topic: String,
    /// Payload bytes.
    pub payload: Vec<u8>,
}

/// Shared handle to inspect/drive a [`MockTransport`].
#[derive(Clone, Default)]
pub struct MockHandle {
    published: Arc<Mutex<Vec<Published>>>,
    subscribed: Arc<Mutex<Vec<String>>>,
    injector: Arc<Mutex<Option<mpsc::Sender<Incoming>>>>,
    fail_publishes: Arc<Mutex<usize>>,
}

impl MockHandle {
    /// All publishes recorded so far.
    pub fn published(&self) -> Vec<Published> {
        self.published.lock().unwrap().clone()
    }

    /// All topics subscribed so far.
    pub fn subscribed(&self) -> Vec<String> {
        self.subscribed.lock().unwrap().clone()
    }

    /// Make the next `n` publishes fail with a transport error.
    pub fn fail_next_publishes(&self, n: usize) {
        *self.fail_publishes.lock().unwrap() = n;
    }

    /// Inject an incoming message to the engine.
    pub async fn inject(&self, topic: &str, payload: serde_json::Value) {
        let tx = self.injector.lock().unwrap().clone();
        if let Some(tx) = tx {
            let _ = tx
                .send(Incoming {
                    topic: topic.to_string(),
                    payload: serde_json::to_vec(&payload).unwrap(),
                })
                .await;
        }
    }
}

/// A mock [`JobsTransport`].
pub struct MockTransport {
    handle: MockHandle,
    rx: Option<mpsc::Receiver<Incoming>>,
}

impl MockTransport {
    /// Create a new mock transport and a handle to drive/inspect it.
    pub fn new() -> (Self, MockHandle) {
        let (tx, rx) = mpsc::channel(64);
        let handle = MockHandle::default();
        *handle.injector.lock().unwrap() = Some(tx);
        (
            Self {
                handle: handle.clone(),
                rx: Some(rx),
            },
            handle,
        )
    }
}

#[async_trait]
impl JobsTransport for MockTransport {
    async fn subscribe(&self, topic: &str) -> Result<()> {
        self.handle
            .subscribed
            .lock()
            .unwrap()
            .push(topic.to_string());
        Ok(())
    }

    async fn publish(&self, topic: &str, payload: Vec<u8>) -> Result<()> {
        {
            let mut fail = self.handle.fail_publishes.lock().unwrap();
            if *fail > 0 {
                *fail -= 1;
                return Err(crate::error::Error::Transport(
                    "mock publish failure".into(),
                ));
            }
        }
        self.handle.published.lock().unwrap().push(Published {
            topic: topic.to_string(),
            payload,
        });
        Ok(())
    }

    fn incoming(&mut self) -> Option<mpsc::Receiver<Incoming>> {
        self.rx.take()
    }
}

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunEvent {
    pub event_id: u64,
    pub run_id: String,
    pub event_type: String,
    pub data: serde_json::Value,
}

struct RunChannel {
    tx: broadcast::Sender<RunEvent>,
    buffer: Vec<RunEvent>,
    next_id: u64,
}

#[derive(Clone)]
pub struct EventBus {
    channels: Arc<RwLock<HashMap<String, RunChannel>>>,
    buffer_size: usize,
}

impl EventBus {
    pub fn new(buffer_size: usize) -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            buffer_size,
        }
    }

    pub async fn publish(&self, run_id: &str, event_type: String, data: serde_json::Value) -> u64 {
        let mut channels = self.channels.write().await;
        let channel = channels.entry(run_id.to_string()).or_insert_with(|| {
            let (tx, _) = broadcast::channel(256);
            RunChannel { tx, buffer: Vec::new(), next_id: 1 }
        });

        let event = RunEvent {
            event_id: channel.next_id,
            run_id: run_id.to_string(),
            event_type,
            data,
        };
        channel.next_id += 1;

        // Ring buffer
        channel.buffer.push(event.clone());
        if channel.buffer.len() > self.buffer_size {
            channel.buffer.remove(0);
        }

        let _ = channel.tx.send(event);
        channel.next_id - 1
    }

    pub async fn subscribe(&self, run_id: &str, last_event_id: Option<u64>) -> (Vec<RunEvent>, broadcast::Receiver<RunEvent>) {
        let mut channels = self.channels.write().await;
        let channel = channels.entry(run_id.to_string()).or_insert_with(|| {
            let (tx, _) = broadcast::channel(256);
            RunChannel { tx, buffer: Vec::new(), next_id: 1 }
        });

        let replay = match last_event_id {
            Some(id) => channel.buffer.iter().filter(|e| e.event_id > id).cloned().collect(),
            None => channel.buffer.clone(),
        };

        let rx = channel.tx.subscribe();
        (replay, rx)
    }
}

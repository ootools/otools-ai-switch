use serde::Serialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const MAX_EVENTS: usize = 2048;
const MAX_QUEUED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
struct Queue {
    events: VecDeque<(Value, usize)>,
    bytes: usize,
}

/// The queue exists before any task starts. It does not depend on a mounted page
/// or an asynchronous broadcast subscriber, and bounds both count and memory.
#[derive(Default)]
pub struct WebEventBroadcaster {
    queue: Mutex<Queue>,
}

impl WebEventBroadcaster {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn send(&self, channel: &str, payload: &impl Serialize) {
        let Ok(payload) = serde_json::to_value(payload) else {
            return;
        };
        let event = json!({"topic": channel, "payload": payload});
        let Ok(encoded) = serde_json::to_vec(&event) else {
            return;
        };
        let size = encoded.len();
        if size > MAX_QUEUED_BYTES {
            return;
        }
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        while queue.events.len() >= MAX_EVENTS || queue.bytes + size > MAX_QUEUED_BYTES {
            if let Some((_, old_size)) = queue.events.pop_front() {
                queue.bytes -= old_size;
            } else {
                break;
            }
        }
        queue.bytes += size;
        queue.events.push_back((event, size));
    }

    pub fn drain(&self) -> Vec<Value> {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.bytes = 0;
        queue.events.drain(..).map(|(value, _)| value).collect()
    }
}

#[derive(Clone)]
pub enum EventEmitter {
    Web(Arc<WebEventBroadcaster>),
    Noop,
}
impl EventEmitter {
    pub fn emit(&self, channel: &str, payload: &impl Serialize) {
        if let Self::Web(queue) = self {
            queue.send(channel, payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn events_survive_before_polling_and_drop_oldest_at_capacity() {
        let queue = WebEventBroadcaster::new();
        for n in 0..2050 {
            queue.send("progress", &n);
        }
        let events = queue.drain();
        assert_eq!(events.len(), 2048);
        assert_eq!(events[0], json!({"topic":"progress","payload":2}));
        assert_eq!(events.last().unwrap()["payload"], 2049);
        assert!(queue.drain().is_empty());
    }
    #[test]
    fn an_oversized_event_cannot_displace_all_normal_events() {
        let queue = WebEventBroadcaster::new();
        queue.send("ready", &true);
        queue.send("huge", &"x".repeat(MAX_QUEUED_BYTES));
        assert_eq!(queue.drain(), vec![json!({"topic":"ready","payload":true})]);
    }
}

//! The recorder runs inside the WSL agent (`rec.*` methods). This bridge keeps the message
//! shapes of the former `recordings.py` worker: `{kind: result|log|activity|closed, …}`.
use crate::agent::{AgentClient, AgentEvent};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::Duration,
};

/// `rec.check` sources ROS and runs two checks of up to 20 s each.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
/// `rec.stop` waits up to 35 s for rosbag2 to finalize the MCAP file.
const STOP_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) struct Bridge {
    pub distro: String,
    client: Arc<AgentClient>,
    tx: SyncSender<Value>,
    pub events: Receiver<Value>,
    done: Arc<AtomicBool>,
    /// The agent exited under this bridge; a recording it ran was already cut off.
    lost: Arc<AtomicBool>,
}
impl Bridge {
    pub fn spawn(distro: &str) -> Result<Self, String> {
        let client =
            crate::agent::shared(distro).ok_or("The Linux agent is not available in this mode.")?;
        Self::with_client(client)
    }
    pub(super) fn with_client(client: Arc<AgentClient>) -> Result<Self, String> {
        let subscription = client.subscribe_filtered(|e| {
            matches!(e, AgentEvent::Rec(_) | AgentEvent::Disconnected { .. })
        });
        client.connect().map_err(|e| e.to_string())?;
        let (tx, events) = mpsc::sync_channel(256);
        let done = Arc::new(AtomicBool::new(false));
        let lost = Arc::new(AtomicBool::new(false));
        let (forward, stop, gone) = (tx.clone(), done.clone(), lost.clone());
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                for event in subscription.wait(Duration::from_millis(250), 100) {
                    let message = match event {
                        AgentEvent::Rec(value) => value,
                        AgentEvent::Disconnected {
                            expected: false, ..
                        } => {
                            gone.store(true, Ordering::SeqCst);
                            json!({"kind":"closed"})
                        }
                        _ => continue,
                    };
                    if forward.send(message).is_err() {
                        return;
                    }
                }
            }
        });
        Ok(Self {
            distro: client.distro.clone(),
            client,
            tx,
            events,
            done,
            lost,
        })
    }
    /// Sends `{action, …}` as `rec.<action>`; the reply arrives as a `result` event.
    pub fn send(&mut self, mut value: Value) -> Result<(), String> {
        let action = value["action"].as_str().unwrap_or("").to_owned();
        if let Some(object) = value.as_object_mut() {
            object.remove("action");
        }
        let timeout = if action == "stop" {
            STOP_TIMEOUT
        } else {
            REQUEST_TIMEOUT
        };
        let ticket = self
            .client
            .request(&format!("rec.{action}"), value, timeout);
        let tx = self.tx.clone();
        thread::spawn(move || {
            let event = match ticket.wait() {
                Ok(data) => json!({"kind":"result","action":action,"ok":true,"data":data}),
                Err(e) => {
                    json!({"kind":"result","action":action,"ok":false,"error":e.to_string()})
                }
            };
            let _ = tx.send(event);
        });
        Ok(())
    }
}
impl Drop for Bridge {
    /// Finalizes an active recording or replay, as the old worker did when it was closed.
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
        // After an agent restart the new agent has nothing to finalize; do not block on it.
        if !self.lost.load(Ordering::SeqCst) && self.client.connected() {
            let _ = self.client.call("rec.stop", json!({}), STOP_TIMEOUT);
        }
    }
}

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;

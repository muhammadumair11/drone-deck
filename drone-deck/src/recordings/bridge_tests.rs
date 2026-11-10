use super::*;
use crate::agent::fake::{self, eventually};

#[test]
fn recorder_requests_become_rec_methods_with_the_old_message_shapes() {
    let (connector, seen) =
        fake::connector(Arc::new(|request, out| match request["method"].as_str() {
            Some("rec.check") => {
                out.send(json!({"event":"rec","data":{"kind":"log","message":"sourcing ROS"}}));
                out.reply(request, json!({"ready":true,"root":"/home/pilot/rec"}));
            }
            Some("rec.replay") => {
                out.error(request, "Select a finalized, non-empty MCAP recording.")
            }
            Some("die") => out.close(),
            _ => out.reply(request, json!({})),
        }));
    let client = crate::agent::AgentClient::new("Fake-Rec", connector);
    let mut bridge = Bridge::with_client(client.clone()).unwrap();
    assert_eq!(bridge.distro, "Fake-Rec");
    bridge.send(json!({"action":"check","domain":0})).unwrap();
    bridge
        .send(json!({"action":"replay","domain":77,"session":"x"}))
        .unwrap();
    let mut messages = Vec::new();
    eventually(|| {
        messages.extend(bridge.events.try_iter());
        (messages.iter().filter(|m| m["kind"] == "result").count() == 2).then_some(())
    });
    assert!(messages.contains(&json!({"kind":"log","message":"sourcing ROS"})));
    assert!(
        messages.contains(&json!({"kind":"result","action":"check","ok":true,
        "data":{"ready":true,"root":"/home/pilot/rec"}}))
    );
    assert!(
        messages.contains(&json!({"kind":"result","action":"replay","ok":false,
        "error":"Select a finalized, non-empty MCAP recording."}))
    );
    let replay = seen
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|r| r["method"] == "rec.replay")
        .cloned()
        .unwrap();
    assert_eq!(replay["params"], json!({"domain":77,"session":"x"}));

    drop(client.request("die", json!({}), Duration::from_secs(1)));
    eventually(|| {
        bridge
            .events
            .try_iter()
            .any(|m| m["kind"] == "closed")
            .then_some(())
    });
}

#[test]
fn closing_the_bridge_finalizes_an_active_recording() {
    let (connector, seen) = fake::connector(Arc::new(|request, out| out.reply(request, json!({}))));
    let client = crate::agent::AgentClient::new("Fake-Rec-Close", connector);
    drop(Bridge::with_client(client.clone()).unwrap());
    assert!(seen.methods().contains(&"rec.stop".to_owned()));
}

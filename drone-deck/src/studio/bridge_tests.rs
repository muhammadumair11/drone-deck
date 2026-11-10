use super::*;
use crate::agent::fake::{self, eventually};
use std::sync::Arc;

#[test]
fn data_actions_map_to_agent_methods() {
    assert_eq!(data_method("data_library"), "data.library");
    assert_eq!(data_method("project_create"), "data.project_create");
}

#[test]
fn linux_requests_run_in_the_agent_with_progress_then_one_result() {
    let (connector, seen) = fake::connector(Arc::new(|request, out| {
        out.send(json!({"event":"progress","id":request["id"],"message":"Reading projects"}));
        match request["method"].as_str() {
            Some("data.project_create") => out.error(request, "That project already exists."),
            _ => out.reply(request, json!({"message":"Library loaded.","projects":[]})),
        }
    }));
    let distro = "Fake-Studio";
    crate::agent::install(distro, connector);
    let (tx, rx) = mpsc::channel();
    request(
        json!({"action":"data_library"}),
        distro.into(),
        true,
        tx.clone(),
    );
    let mut events = Vec::new();
    eventually(|| {
        events.extend(rx.try_iter());
        events.iter().any(|e| e.get("ok").is_some()).then_some(())
    });
    assert_eq!(events[0], json!({"progress":"Reading projects"}));
    assert_eq!(
        events[1],
        json!({"ok":true,"action":"data_library","data":{"message":"Library loaded.","projects":[]}})
    );
    request(
        json!({"action":"project_create","name":"d","kind":"onnx"}),
        distro.into(),
        true,
        tx,
    );
    let failed = eventually(|| rx.try_iter().find(|e| e.get("ok").is_some()));
    assert_eq!(failed["ok"], false);
    assert_eq!(failed["error"], "That project already exists.");
    let create = seen
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|r| r["method"] == "data.project_create")
        .cloned()
        .unwrap();
    assert_eq!(create["params"], json!({"name":"d","kind":"onnx"}));
}

#[test]
fn linux_requests_fail_cleanly_without_an_agent() {
    let (tx, rx) = mpsc::channel();
    request(
        json!({"action":"data_library"}),
        "Never-Enabled-Studio".into(),
        true,
        tx,
    );
    let reply = eventually(|| rx.try_iter().next());
    assert_eq!(reply["ok"], false);
    assert!(reply["error"].as_str().unwrap().contains("not available"));
}

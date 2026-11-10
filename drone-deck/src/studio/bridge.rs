use crate::*;
use serde_json::{Value, json};
use std::{
    io::{BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc::{self, Sender},
    thread,
};
/// `data.library` and `data.project_create` read and write small manifests only.
const DATA_TIMEOUT: Duration = Duration::from_secs(300);

/// Runs one Studio action: Linux data actions as `data.*` agent methods, everything else
/// through `studio/control.ps1`. Progress and the result arrive on `tx` as
/// `{progress}` and `{ok, action, data|error}`.
pub(super) fn request(value: Value, distro: String, linux: bool, tx: Sender<Value>) {
    thread::spawn(move || {
        let result = if linux {
            agent_request(value, &distro, &tx)
        } else {
            windows_request(value, &tx)
        };
        if let Err(error) = result {
            let _ = tx.send(json!({"ok":false,"error":error}));
        }
    });
}

/// `data_library` → `data.library`; other actions keep their names under `data.`.
pub(super) fn data_method(action: &str) -> String {
    match action {
        "data_library" => "data.library".into(),
        other => format!("data.{other}"),
    }
}

fn agent_request(mut value: Value, distro: &str, tx: &Sender<Value>) -> Result<(), String> {
    let client =
        crate::agent::shared(distro).ok_or("The Linux agent is not available in this mode.")?;
    let action = value["action"].as_str().unwrap_or("").to_owned();
    if let Some(object) = value.as_object_mut() {
        object.remove("action");
    }
    let (progress, messages) = mpsc::channel();
    let mut ticket =
        client.request_with_progress(&data_method(&action), value, DATA_TIMEOUT, Some(progress));
    loop {
        if let Ok(message) = messages.recv_timeout(Duration::from_millis(100)) {
            let _ = tx.send(json!({"progress":message}));
        }
        if let Some(reply) = ticket.try_take() {
            for message in messages.try_iter() {
                let _ = tx.send(json!({"progress":message}));
            }
            let _ = tx.send(match reply {
                Ok(data) => json!({"ok":true,"action":action,"data":data}),
                Err(e) => json!({"ok":false,"action":action,"error":e.to_string()}),
            });
            return Ok(());
        }
    }
}

fn windows_request(value: Value, tx: &Sender<Value>) -> Result<(), String> {
    let directory = crate::backend::scripts().join("studio");
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(directory.join("control.ps1"))
        .arg("-Root")
        .arg(backend::root());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(value.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    let mut received = false;
    for line in crate::process_output::BoundedLines::with_limit(
        BufReader::new(child.stdout.take().unwrap()),
        16_000_000,
    ) {
        let line = line.map_err(|e| e.to_string())?;
        if let Ok(event) = serde_json::from_str::<Value>(&line) {
            received = true;
            let _ = tx.send(event);
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() || !received {
        return Err("Studio helper did not return a valid response. Check its deployment and selected environment.".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "bridge_tests.rs"]
mod tests;

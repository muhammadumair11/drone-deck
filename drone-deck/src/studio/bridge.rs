use crate::*;
use std::{
    io::{BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc::Sender,
    thread,
};
pub(super) fn request(
    value: serde_json::Value,
    distro: String,
    linux: bool,
    tx: Sender<serde_json::Value>,
) {
    thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let directory = std::env::current_exe()
                .map_err(|e| e.to_string())?
                .parent()
                .unwrap()
                .join("runtime/studio");
            let mut command = if linux {
                let mut c = Command::new("wsl.exe");
                let script = directory
                    .join("data.py")
                    .to_string_lossy()
                    .replace('\\', "/");
                let script = format!("/mnt/{}/{}", script[..1].to_lowercase(), &script[3..]);
                c.args(["-d", &distro, "--exec", "python3", "-u", &script]);
                c
            } else {
                let mut c = Command::new("powershell.exe");
                c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                    .arg(directory.join("control.ps1"))
                    .arg("-Root")
                    .arg(backend::root());
                c
            };
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
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) {
                    received = true;
                    let _ = tx.send(event);
                }
            }
            let status = child.wait().map_err(|e| e.to_string())?;
            if !status.success() || !received {
                return Err("Studio helper did not return a valid response. Check its deployment and selected environment.".into());
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = tx.send(serde_json::json!({"ok":false,"error":error}));
        }
    });
}

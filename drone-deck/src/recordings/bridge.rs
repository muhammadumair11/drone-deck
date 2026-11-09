use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

pub(super) struct Bridge {
    pub distro: String,
    child: Child,
    input: ChildStdin,
    pub events: Receiver<Value>,
}
impl Bridge {
    pub fn spawn(distro: &str) -> Result<Self, String> {
        let script = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .unwrap()
            .join("runtime/recordings.py");
        if !script.is_file() {
            return Err(format!("Recording worker is missing: {}", script.display()));
        }
        let path = script.to_string_lossy().replace('\\', "/");
        if path.len() < 3 || path.as_bytes()[1] != b':' {
            return Err("Recording worker needs a Windows drive path.".into());
        }
        let linux = format!("/mnt/{}/{}", path[..1].to_lowercase(), &path[3..]);
        let mut command = Command::new("wsl.exe");
        command
            .args(["-d", distro, "--", "python3", "-u", &linux])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, events) = mpsc::sync_channel(256);
        let errors = tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let event = serde_json::from_str(&line)
                    .unwrap_or_else(|_| json!({"kind":"log","message":line}));
                if tx.send(event).is_err() {
                    break;
                }
            }
            let _ = tx.send(json!({"kind":"closed"}));
        });
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if errors.send(json!({"kind":"log","message":line})).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            distro: distro.into(),
            child,
            input,
            events,
        })
    }
    pub fn send(&mut self, value: Value) -> Result<(), String> {
        writeln!(self.input, "{value}")
            .and_then(|_| self.input.flush())
            .map_err(|e| e.to_string())
    }
}
impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.send(json!({"action":"shutdown"}));
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            while self.events.try_recv().is_ok() {}
            if self.child.try_wait().ok().flatten().is_some() {
                break;
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

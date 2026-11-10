//! Foxglove: open a recording's `.mcap` with the default app, and "Live in Foxglove".
//!
//! Opening reads the file in place through `\\wsl.localhost\<distro>\…` (or `\\wsl$\…`). When
//! Windows cannot reach that path, or no app opens it from there, the file is copied to
//! `<data root>\exports\` (directly, or by the agent's `pipe.export` when the share is not
//! reachable) and the copy is opened. Live mode starts the owned `foxglove` profile
//! (`foxglove_bridge` on port 8765, reached through WSL localhost forwarding) and asks Windows to
//! open Foxglove on its websocket once the profile runs and port 8765 accepts a TCP connection.
use super::pipelines::Msg;
use crate::*;
use serde_json::json;
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver, TryRecvError},
};

pub(crate) const PROFILE: &str = "foxglove";
pub(crate) const LIVE_URL: &str =
    "foxglove://open?ds=foxglove-websocket&ds.url=ws://localhost:8765";
/// The bridge's websocket as Windows reaches it (WSL localhost forwarding).
pub(crate) const LIVE_ADDR: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 8765));
/// How long a running bridge may take to accept a connection before Live gives up.
pub(crate) const LIVE_PORT_TIMEOUT: Duration = Duration::from_secs(20);
/// Pause between connect attempts, and the limit of one attempt.
const PROBE_EVERY: Duration = Duration::from_millis(250);
const PROBE_CONNECT: Duration = Duration::from_millis(500);

/// "Live in Foxglove" waiting for the bridge: first for the profile to run, then for its port
/// to accept a TCP connection. Connect attempts run on a worker thread, one at a time.
pub(crate) struct LiveWait {
    pub addr: SocketAddr,
    /// When the profile was first seen Running.
    pub running: Option<Instant>,
    probe: Option<Receiver<bool>>,
    next_probe: Instant,
}
impl LiveWait {
    pub(crate) fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            running: None,
            probe: None,
            next_probe: Instant::now(),
        }
    }
}

/// One connect attempt to `addr` on a worker thread; true when it connected.
fn probe(addr: SocketAddr) -> Receiver<bool> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(TcpStream::connect_timeout(&addr, PROBE_CONNECT).is_ok());
    });
    rx
}

/// `\\wsl.localhost\<distro>\…` then `\\wsl$\<distro>\…` for a Linux path.
pub(crate) fn unc_candidates(distro: &str, linux: &str) -> Vec<PathBuf> {
    [r"\\wsl.localhost\", r"\\wsl$\"]
        .iter()
        .map(|prefix| under(&PathBuf::from(format!("{prefix}{distro}")), linux))
        .collect()
}

fn under(root: &Path, linux: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for part in linux.split('/').filter(|p| !p.is_empty()) {
        path.push(part);
    }
    path
}

/// The export copy's name: `<recording or run>-<file>`, as `pipe.export` names it.
pub(crate) fn export_name(linux: &str) -> String {
    let parts: Vec<&str> = linux.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [.., owner, _bag, file] => format!("{owner}-{file}"),
        [.., file] => (*file).to_string(),
        [] => "recording.mcap".into(),
    }
}

/// How a file was opened, for the status line.
#[derive(Debug, PartialEq)]
pub(crate) enum Opened {
    InPlace(PathBuf),
    Copy(PathBuf),
}

/// Opens `linux` in place from the first reachable candidate, else exports a copy and opens it.
/// `open` launches the default app; `export(linux, dest)` copies through the agent.
pub(crate) fn open_or_copy(
    candidates: &[PathBuf],
    linux: &str,
    exports: &Path,
    open: &dyn Fn(&Path) -> Result<(), String>,
    export: &dyn Fn(&str, &Path) -> Result<PathBuf, String>,
) -> Result<Opened, String> {
    let reachable = candidates.iter().find(|p| p.is_file());
    let mut reason = match reachable {
        Some(path) => match open(path) {
            Ok(()) => return Ok(Opened::InPlace(path.clone())),
            Err(e) => e,
        },
        None => "the WSL share is not reachable from Windows".into(),
    };
    std::fs::create_dir_all(exports)
        .map_err(|e| format!("Could not create {}: {e}", exports.display()))?;
    let copy = match reachable {
        Some(path) => {
            let target = exports.join(export_name(linux));
            std::fs::copy(path, &target).map(|_| target).or_else(|e| {
                reason = format!("{reason}; copying from the share failed ({e})");
                export(linux, exports)
            })?
        }
        None => export(linux, exports)?,
    };
    open(&copy).map_err(|e| {
        format!(
            "No app opened the .mcap file ({e}). Install Foxglove, or open {} from Foxglove. ({reason})",
            copy.display()
        )
    })?;
    Ok(Opened::Copy(copy))
}

/// Opens a file with its default app; fails when Windows reports no association or no access.
fn start_process(path: &Path) -> Result<(), String> {
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference='Stop'; Start-Process -FilePath $env:DRONE_DECK_OPEN",
        ])
        .env("DRONE_DECK_OPEN", path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    // The helper dies with Drone Deck; the app it opens breaks away and stays open.
    let (child, _) =
        crate::winjob::spawn_helper(&mut command, 0x08000000).map_err(|e| e.to_string())?;
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let text = String::from_utf8_lossy(&output.stderr);
        let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        Err(first.trim().chars().take(300).collect())
    }
}

/// Opens a recording or run in Foxglove: `mcap` is a `.mcap` path, or a recording/run folder
/// whose `bag/` holds the file (listed through the agent).
pub(crate) fn open(s: &mut State, mcap: String) {
    if s.headless {
        s.notice = "Headless preview · opening Foxglove is disabled.".into();
        return;
    }
    if !s.wsl_ready() {
        s.notice = "Select a working WSL 2 distribution in Settings → Environment first.".into();
        return;
    }
    let distro = s.config.distro.clone();
    let exports = backend::root().join("exports");
    let tx = s.studio.pipes.tx.clone();
    s.studio.pipes.message = "Opening in Foxglove…".into();
    activity::record(s, format!("Opening {mcap} in Foxglove"));
    std::thread::spawn(move || {
        let client = crate::agent::shared(&distro);
        let file = if mcap.ends_with(".mcap") {
            Ok(mcap)
        } else {
            first_mcap(client.as_deref(), &mcap)
        };
        let result = file.and_then(|linux| {
            let export = |linux: &str, dest: &Path| {
                let dest_dir = dest.to_path_buf();
                let client = client
                    .as_deref()
                    .ok_or("The Linux agent is not available in this mode.")?;
                let mount = client.hello().map(|h| h.automount_root).unwrap_or_default();
                // `pipe.export` takes its destination as the distro sees it.
                let dest = crate::agent::linux_path(dest, &mount)
                    .ok_or("The data folder is not on a drive letter.")?;
                let reply = client
                    .call(
                        "pipe.export",
                        json!({"path": linux, "dest": dest}),
                        Duration::from_secs(600),
                    )
                    .map_err(|e| e.to_string())?;
                let copied = reply["path"].as_str().unwrap_or("");
                let name = copied.rsplit('/').next().unwrap_or("");
                Ok(dest_dir.join(name))
            };
            open_or_copy(
                &unc_candidates(&distro, &linux),
                &linux,
                &exports,
                &start_process,
                &export,
            )
        });
        let text = match result {
            Ok(Opened::InPlace(path)) => {
                format!("Opened {} with the default .mcap app.", path.display())
            }
            Ok(Opened::Copy(path)) => format!(
                "Copied to {} and opened it (the WSL path could not be opened directly).",
                path.display()
            ),
            Err(e) => format!("Could not open in Foxglove: {e}"),
        };
        let _ = tx.send(Msg::Notice(text));
    });
}

fn first_mcap(client: Option<&crate::agent::AgentClient>, folder: &str) -> Result<String, String> {
    let client = client.ok_or("The Linux agent is not available in this mode.")?;
    let reply = client
        .call(
            "files.list",
            json!({"dir": format!("{}/bag", folder.trim_end_matches('/')), "pattern": "*.mcap"}),
            Duration::from_secs(20),
        )
        .map_err(|e| e.to_string())?;
    reply
        .as_array()
        .and_then(|rows| rows.iter().filter_map(|r| r["path"].as_str()).min())
        .map(str::to_owned)
        .ok_or_else(|| "This recording has no .mcap file yet.".into())
}

/// Whether the bridge profile is running or starting.
pub(crate) fn live(s: &State) -> bool {
    s.status
        .get(PROFILE)
        .is_some_and(|v| v == "Running" || v == "Starting")
}

/// Starts or stops the owned `foxglove` bridge; Windows is asked to open Foxglove once port 8765 accepts connections.
pub(crate) fn toggle_live(s: &mut State) {
    if s.headless {
        s.notice = "Headless preview · the Foxglove bridge does not start.".into();
        return;
    }
    if live(s) {
        s.studio.pipes.live_pending = None;
        s.send(Request::Stop(PROFILE.into()));
        s.studio.pipes.message = "Stopping the Foxglove bridge.".into();
        activity::record(s, "Stopped the Foxglove bridge");
        return;
    }
    let Some(index) = s.config.profiles.iter().position(|p| p.id == PROFILE) else {
        s.studio.pipes.message = "The Foxglove bridge is not configured. Tools & config → All registered tools → Foxglove bridge → Review installation (ros-jazzy-foxglove-bridge).".into();
        return;
    };
    s.start(index);
    if live(s) {
        s.studio.pipes.live_pending = Some(LiveWait::new(LIVE_ADDR));
        s.studio.pipes.message = "Starting the Foxglove bridge on port 8765; Drone Deck asks Windows to open Foxglove once the port accepts connections.".into();
        activity::record(s, "Started the Foxglove bridge for Live in Foxglove");
    }
}

/// Shown when the port check passes. The check proves only that something on `127.0.0.1:8765`
/// accepts a TCP connection, and `open` only hands the `foxglove://` link to Windows.
pub(crate) const LIVE_OPENED: &str = "Port 8765 accepts connections; asked Windows to open Foxglove on ws://localhost:8765. Any program listening on that port passes this check, and Drone Deck does not check that Foxglove started.";

/// What one poll of a Live wait decided.
enum Step {
    Wait,
    /// The profile runs and the port check started: say so once.
    Probing,
    Open,
    Failed(String),
}

/// Opens Foxglove once a requested bridge runs and its port accepts a connection; gives up
/// when the bridge stops, or when the port stays closed for [`LIVE_PORT_TIMEOUT`]. Never
/// blocks: connect attempts run on a worker thread and their results are read here.
pub(crate) fn poll(s: &mut State) -> bool {
    let status = s.status.get(PROFILE).cloned();
    let Some(wait) = &mut s.studio.pipes.live_pending else {
        return false;
    };
    let step = match status.as_deref() {
        Some("Starting") => Step::Wait,
        Some("Running") => live_step(wait, Instant::now()),
        other => Step::Failed(format!(
            "The Foxglove bridge did not start ({}). See Activity → Process output.",
            other.unwrap_or("not running")
        )),
    };
    let p = &mut s.studio.pipes;
    match step {
        Step::Wait => false,
        Step::Probing => {
            p.message = "foxglove_bridge runs; waiting for port 8765 to accept a connection before opening Foxglove…".into();
            true
        }
        Step::Open => {
            p.live_pending = None;
            p.message = LIVE_OPENED.into();
            s.open(LIVE_URL.into());
            true
        }
        Step::Failed(text) => {
            p.live_pending = None;
            p.message = text;
            true
        }
    }
}

fn live_step(wait: &mut LiveWait, now: Instant) -> Step {
    let first = wait.running.is_none();
    let since = *wait.running.get_or_insert(now);
    if let Some(rx) = &wait.probe {
        match rx.try_recv() {
            Ok(true) => return Step::Open,
            Ok(false) | Err(TryRecvError::Disconnected) => {
                wait.probe = None;
                wait.next_probe = now + PROBE_EVERY;
            }
            Err(TryRecvError::Empty) => return Step::Wait,
        }
    }
    if now.duration_since(since) >= LIVE_PORT_TIMEOUT {
        return Step::Failed(format!(
            "foxglove_bridge runs, but port {} did not accept a connection within {}s, so Foxglove was not opened. The bridge may have failed to bind the port, or WSL localhost forwarding is off. See Activity → Process output.",
            wait.addr.port(),
            LIVE_PORT_TIMEOUT.as_secs()
        ));
    }
    if now >= wait.next_probe {
        wait.probe = Some(probe(wait.addr));
    }
    if first { Step::Probing } else { Step::Wait }
}

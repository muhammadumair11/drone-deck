//! Foxglove: open a recording's `.mcap` with the default app, and "Live in Foxglove".
//!
//! Opening reads the file in place through `\\wsl.localhost\<distro>\…` (or `\\wsl$\…`). When
//! Windows cannot reach that path, or no app opens it from there, the file is copied to
//! `<data root>\exports\` (directly, or by the agent's `pipe.export` when the share is not
//! reachable) and the copy is opened. Live mode starts the owned `foxglove` profile
//! (`foxglove_bridge` on port 8765, reached through WSL localhost forwarding) and opens Foxglove
//! on its websocket once the profile runs.
use super::pipelines::Msg;
use crate::*;
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) const PROFILE: &str = "foxglove";
pub(crate) const LIVE_URL: &str =
    "foxglove://open?ds=foxglove-websocket&ds.url=ws://localhost:8765";
/// The websocket accepts connections shortly after the process starts.
const LIVE_DELAY: Duration = Duration::from_millis(1500);

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
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().map_err(|e| e.to_string())?;
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

/// Starts or stops the owned `foxglove` bridge; Foxglove opens once it runs.
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
        s.studio.pipes.live_pending = Some(Instant::now());
        s.studio.pipes.message =
            "Starting the Foxglove bridge on port 8765; Foxglove opens when it runs.".into();
        activity::record(s, "Started the Foxglove bridge for Live in Foxglove");
    }
}

/// Opens Foxglove once a requested bridge runs; drops the request if the bridge stops.
pub(crate) fn poll(s: &mut State) -> bool {
    let Some(since) = s.studio.pipes.live_pending else {
        return false;
    };
    match s.status.get(PROFILE).map(String::as_str) {
        Some("Running") if since.elapsed() >= LIVE_DELAY => {
            s.studio.pipes.live_pending = None;
            s.open(LIVE_URL.into());
            s.studio.pipes.message =
                "Foxglove opened on ws://localhost:8765 (the owned foxglove_bridge).".into();
            true
        }
        Some("Running") | Some("Starting") => false,
        other => {
            s.studio.pipes.live_pending = None;
            s.studio.pipes.message = format!(
                "The Foxglove bridge did not start ({}). See Activity → Process output.",
                other.unwrap_or("not running")
            );
            true
        }
    }
}

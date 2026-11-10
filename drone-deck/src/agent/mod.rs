//! The WSL agent: one long-lived Python process per distribution (`scripts/agent/agent.py`)
//! that runs owned processes, probes, the MAVLink router and the recorder/data helpers.
//!
//! [`shared`] hands out the [`AgentClient`] for a distribution (spawned lazily, never in
//! headless mode). [`AgentState`] is the UI's view: the agent's `hello`, router status, and
//! processes left running by an earlier session, which the user adopts or stops.
mod client;
#[cfg(test)]
pub(crate) mod fake;
mod launch;
mod state;
#[cfg(test)]
mod tests;
mod view;

pub(crate) use client::{AgentClient, Connector, Spawned, Subscription, Ticket};
pub(crate) use launch::{Launcher, probe};
pub(crate) use state::{AgentState, Leftover, adopt, poll, stop_leftover};
pub(crate) use view::leftovers;

use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    io,
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

/// The agent's `hello` reply.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Hello {
    pub version: u64,
    pub distro: String,
    pub user: String,
    pub home: String,
    pub automount_root: String,
    pub python: String,
}

/// The MAVLink router's `router` event and `router.status` reply.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct RouterStatus {
    /// `stopped`, `listening` (no vehicle datagram in the last 3 s), `forwarding` or `failed`.
    pub state: String,
    pub clients: u64,
    /// Datagrams received from the vehicle.
    pub packets: u64,
    /// Datagrams (at most 1400 bytes each) sent on to the vehicle.
    pub to_vehicle: u64,
    /// Client bytes dropped because no vehicle address is known yet.
    pub dropped: u64,
    /// Datagrams from a UDP sender other than the first one (the vehicle), never forwarded.
    /// Another flight stack or a forwarded real flight controller shares the port.
    pub foreign: u64,
    pub vehicle: Option<String>,
    pub udp_listen: Option<u16>,
    pub tcp_port: Option<u16>,
}

/// What the agent and the client report, in arrival order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AgentEvent {
    /// One line (at most 4096 bytes) from an owned process; `stream` is `stdout` or `stderr`.
    Output {
        key: String,
        stream: String,
        line: String,
    },
    /// An owned process ended. `code` is `None` for an adopted process (no wait status).
    Exited {
        key: String,
        code: Option<i64>,
    },
    Readiness {
        key: String,
        state: String,
        detail: String,
    },
    Router(RouterStatus),
    Log {
        level: String,
        message: String,
    },
    /// A recorder message in the old `recordings.py` shape (`kind`: log, activity, result).
    Rec(Value),
    /// Any other event, kept whole for newer agents.
    Other(String, Value),
    /// A new agent process started (first start or reconnect).
    Connected,
    /// The agent answered `hello`.
    Hello(Hello),
    /// The agent exited. `expected` is set when [`AgentClient::close`] asked it to.
    Disconnected {
        code: Option<i32>,
        expected: bool,
    },
    /// Output lines this subscription dropped because it was full.
    Dropped(u64),
}
impl AgentEvent {
    pub(crate) fn parse(name: &str, v: &Value) -> Self {
        let text = |field: &str| v[field].as_str().unwrap_or("").to_owned();
        match name {
            "output" => Self::Output {
                key: text("key"),
                stream: text("stream"),
                line: text("line"),
            },
            "exited" => Self::Exited {
                key: text("key"),
                code: v["code"].as_i64(),
            },
            "readiness" => Self::Readiness {
                key: text("key"),
                state: text("state"),
                detail: text("detail"),
            },
            "router" => Self::Router(serde_json::from_value(v.clone()).unwrap_or_default()),
            "log" => Self::Log {
                level: text("level"),
                message: text("message"),
            },
            "rec" => Self::Rec(v["data"].clone()),
            _ => Self::Other(name.to_owned(), v.clone()),
        }
    }
}

/// `<automount_root><drive>/<rest>` for a Windows drive path, also in its verbatim `\\?\C:\…`
/// form; `None` for UNC or relative paths. `automount_root` is the agent's `hello` value (where
/// the distro mounts Windows drives); `/mnt/` when it is empty or not absolute.
pub(crate) fn linux_path(path: &Path, automount_root: &str) -> Option<String> {
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s);
    let bytes = s.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'/' {
        return None;
    }
    let root = Some(automount_root.trim())
        .filter(|r| r.starts_with('/'))
        .unwrap_or("/mnt/")
        .trim_end_matches('/');
    Some(format!("{root}/{}/{}", s[..1].to_lowercase(), &s[3..]))
}

/// The `wsl.exe` arguments that start the agent in `dir`, the Windows `scripts\agent` folder.
/// wsl.exe maps a Windows `--cd` path through the distro's own drive mount, so a custom
/// `[automount] root` works; `agent.py` finds its siblings from its own location.
pub(crate) fn agent_args(distro: &str, dir: &Path) -> Vec<String> {
    let dir = dir.to_string_lossy();
    let dir = dir.strip_prefix(r"\\?\").unwrap_or(&dir);
    [
        "-d", distro, "--cd", dir, "--exec", "python3", "-u", "agent.py",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Starts `wsl.exe -d <distro> --cd <runtime>\agent --exec python3 -u agent.py`.
pub(crate) fn wsl_connector(distro: String) -> Connector {
    Arc::new(move || {
        let dir = crate::backend::scripts().join("agent");
        let script = dir.join("agent.py");
        if !script.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("the agent script is missing: {}", script.display()),
            ));
        }
        let mut command = Command::new("wsl.exe");
        command
            .args(agent_args(&distro, &dir))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn()?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(io::Error::other("the agent's pipes were not created"));
        };
        Ok(Spawned {
            reader: Box::new(stdout),
            writer: Box::new(stdin),
            errors: Some(Box::new(stderr)),
            process: Box::new(child),
        })
    })
}

#[derive(Default)]
struct Registry {
    enabled: bool,
    clients: HashMap<String, Arc<AgentClient>>,
}
fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}
/// Allows [`shared`] to start agents. The app calls it once outside headless mode, so previews
/// and tests never launch WSL.
pub(crate) fn enable() {
    registry().lock().unwrap().enabled = true;
}
/// The agent client for `distro`, created on first use. `None` until [`enable`] (headless).
pub(crate) fn shared(distro: &str) -> Option<Arc<AgentClient>> {
    let mut registry = registry().lock().unwrap();
    if let Some(client) = registry.clients.get(distro) {
        return Some(client.clone());
    }
    if !registry.enabled || distro.is_empty() {
        return None;
    }
    let client = AgentClient::new(distro, wsl_connector(distro.to_owned()));
    registry.clients.insert(distro.to_owned(), client.clone());
    Some(client)
}
/// Registers a client with its own connector (tests: a fake agent on a thread).
#[cfg(test)]
pub(crate) fn install(distro: &str, connector: Connector) -> Arc<AgentClient> {
    let client = AgentClient::new(distro, connector);
    registry()
        .lock()
        .unwrap()
        .clients
        .insert(distro.to_owned(), client.clone());
    client
}
/// Closes every agent (on app exit, after owned processes were stopped).
pub(crate) fn shutdown_all() {
    let clients: Vec<_> = registry().lock().unwrap().clients.drain().collect();
    for (_, client) in clients {
        client.close(Duration::from_secs(5));
    }
}

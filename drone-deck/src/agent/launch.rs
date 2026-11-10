//! WSL launch profiles through the agent, for the backend worker thread: `run`, `stop`,
//! `input`, `adopt` and `probe`, with agent events turned into backend [`Event`]s.
use super::{AgentClient, AgentEvent, Subscription, shared};
use crate::backend::{Event, Profile, forward_line};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread,
    time::Duration,
};

const RUN_TIMEOUT: Duration = Duration::from_secs(90);
const STOP_TIMEOUT: Duration = Duration::from_secs(20);

struct Owned {
    distro: String,
    exclusive: bool,
    generation: u64,
    /// Re-adopted after an agent restart: a failed adopt means it ended while the agent was down.
    reattach: bool,
}
/// The reply to a `run` or `adopt` sent from a helper thread: key, generation, reply.
type Started = (String, u64, Result<Value, String>);

/// WSL processes the backend owns, keyed by profile id.
pub(crate) struct Launcher {
    owned: HashMap<String, Owned>,
    agents: HashMap<String, (Arc<AgentClient>, Subscription)>,
    started_tx: Sender<Started>,
    started: Receiver<Started>,
    generation: u64,
    resolve: fn(&str) -> Option<Arc<AgentClient>>,
    /// Distributions whose agent exited unexpectedly; their processes are re-adopted on `hello`.
    lost: HashSet<String>,
}
impl Default for Launcher {
    fn default() -> Self {
        Self::with_resolver(shared)
    }
}
impl Launcher {
    /// `resolve` finds a distribution's agent ([`shared`] in the app).
    pub(crate) fn with_resolver(resolve: fn(&str) -> Option<Arc<AgentClient>>) -> Self {
        let (started_tx, started) = mpsc::channel();
        Self {
            owned: HashMap::new(),
            agents: HashMap::new(),
            started_tx,
            started,
            generation: 0,
            resolve,
            lost: HashSet::new(),
        }
    }
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.owned.contains_key(id)
    }
    pub(crate) fn exclusive_running(&self) -> bool {
        self.owned.values().any(|o| o.exclusive)
    }
    pub(crate) fn active(&self) -> bool {
        !self.owned.is_empty()
    }
    fn agent(&mut self, distro: &str) -> Option<Arc<AgentClient>> {
        if let Some((client, _)) = self.agents.get(distro) {
            return Some(client.clone());
        }
        let client = (self.resolve)(distro)?;
        // Subscribe before the first request so no event of the new process is missed.
        let subscription = client.subscribe_filtered(|e| {
            matches!(
                e,
                AgentEvent::Output { .. }
                    | AgentEvent::Exited { .. }
                    | AgentEvent::Readiness { .. }
                    | AgentEvent::Hello(_)
                    | AgentEvent::Disconnected { .. }
            )
        });
        self.agents
            .insert(distro.to_owned(), (client.clone(), subscription));
        Some(client)
    }
    fn begin(
        &mut self,
        id: &str,
        distro: &str,
        exclusive: bool,
        out: &SyncSender<Event>,
    ) -> Option<(Arc<AgentClient>, u64)> {
        let Some(client) = self.agent(distro) else {
            let _ = out.send(Event::Status(id.into(), "Failed".into()));
            let _ = out.send(Event::Log(
                id.into(),
                "The Linux agent is not available in this mode.".into(),
            ));
            return None;
        };
        self.generation += 1;
        self.owned.insert(
            id.to_owned(),
            Owned {
                distro: distro.to_owned(),
                exclusive,
                generation: self.generation,
                reattach: false,
            },
        );
        Some((client, self.generation))
    }
    /// Starts `p` (a `wsl` profile) as `bash -lc <args>` in an owned process group. The agent
    /// sets `DRONE_DECK_SCRIPTS` to the runtime folder as the distro sees it.
    #[cfg(test)]
    pub(crate) fn start(&mut self, p: &Profile, distro: &str, out: &SyncSender<Event>) {
        self.start_env(p, distro, &[], out);
    }
    /// Like `start`, with extra environment variables for the process.
    pub(crate) fn start_env(
        &mut self,
        p: &Profile,
        distro: &str,
        extra: &[(String, String)],
        out: &SyncSender<Event>,
    ) {
        let Some((client, generation)) = self.begin(&p.id, distro, p.exclusive, out) else {
            return;
        };
        let mut env = serde_json::Map::new();
        for (name, value) in extra {
            env.insert(name.clone(), json!(value));
        }
        let params = json!({
            "key": p.id,
            "bash": p.args.join(" "),
            "env": env,
            "readiness": if p.id == "sih" { json!("sih") } else { Value::Null },
        });
        let (tx, id) = (self.started_tx.clone(), p.id.clone());
        thread::spawn(move || {
            let reply = client
                .call("run", params, RUN_TIMEOUT)
                .map_err(|e| e.to_string());
            let _ = tx.send((id, generation, reply));
        });
    }
    /// Follows a process an earlier agent started: output, exit and stop.
    pub(crate) fn adopt(
        &mut self,
        id: &str,
        distro: &str,
        exclusive: bool,
        out: &SyncSender<Event>,
    ) {
        let Some((client, generation)) = self.begin(id, distro, exclusive, out) else {
            return;
        };
        let (tx, id) = (self.started_tx.clone(), id.to_owned());
        thread::spawn(move || {
            let reply = client
                .call("adopt", json!({"key":id}), STOP_TIMEOUT)
                .map_err(|e| e.to_string());
            let _ = tx.send((id, generation, reply));
        });
    }
    /// Stops an owned WSL process group. Returns false when `id` is not a WSL process.
    pub(crate) fn stop(&mut self, id: &str, out: &SyncSender<Event>) -> bool {
        let Some(owned) = self.owned.remove(id) else {
            return false;
        };
        let reply = self
            .agent(&owned.distro)
            .ok_or_else(|| "The Linux agent is not available.".to_owned())
            .and_then(|c| {
                c.call("stop", json!({"key":id,"grace_s":3}), STOP_TIMEOUT)
                    .map_err(|e| e.to_string())
            });
        match reply {
            Ok(value) if value["running"].as_bool() == Some(true) => {
                let _ = out.send(Event::Log(
                    id.into(),
                    "Still running after SIGTERM and SIGKILL; stop it from a WSL shell.".into(),
                ));
                let _ = out.send(Event::Status(id.into(), "Unknown".into()));
            }
            Ok(_) => {
                let _ = out.send(Event::Status(id.into(), "Stopped".into()));
            }
            Err(e) => {
                let _ = out.send(Event::Log(
                    id.into(),
                    format!("Stop was not confirmed: {e}"),
                ));
                let _ = out.send(Event::Status(id.into(), "Unknown".into()));
            }
        }
        true
    }
    /// Writes one line to the process's stdin. Returns false when `id` is not a WSL process.
    #[cfg(test)]
    pub(crate) fn input(&mut self, id: &str, line: &str, out: &SyncSender<Event>) -> bool {
        let Some(distro) = self.owned.get(id).map(|o| o.distro.clone()) else {
            return false;
        };
        if let Some(client) = self.agent(&distro)
            && let Err(e) = client.call(
                "input",
                json!({"key":id,"line":line}),
                Duration::from_secs(5),
            )
        {
            let _ = out.send(Event::Log(id.into(), format!("Input not delivered: {e}")));
        }
        true
    }
    pub(crate) fn shutdown(&mut self, out: &SyncSender<Event>) {
        let ids: Vec<String> = self.owned.keys().cloned().collect();
        for id in ids {
            self.stop(&id, out);
        }
    }
    /// Applies `run`/`adopt` replies and agent events. Call it every worker iteration.
    pub(crate) fn poll(&mut self, out: &SyncSender<Event>) {
        for (id, generation, reply) in self.started.try_iter().collect::<Vec<_>>() {
            if self
                .owned
                .get(&id)
                .is_none_or(|o| o.generation != generation)
            {
                continue;
            }
            match reply {
                Ok(_) => {
                    let _ = out.send(Event::Status(id, "Running".into()));
                }
                Err(e) => {
                    let reattach = self.owned.remove(&id).is_some_and(|o| o.reattach);
                    if reattach {
                        let _ = out.send(Event::Log(
                            id.clone(),
                            format!("Ended while the Linux agent was restarting ({e})."),
                        ));
                        let _ = out.send(Event::Status(id, "Exited".into()));
                    } else {
                        let _ = out.send(Event::Status(id.clone(), "Failed".into()));
                        let _ = out.send(Event::Log(id, e));
                    }
                }
            }
        }
        let mut events = Vec::new();
        for (distro, (_, subscription)) in &self.agents {
            for event in subscription.drain(500) {
                events.push((distro.clone(), event));
            }
        }
        for (distro, event) in events {
            self.apply(&distro, event, out);
        }
    }
    fn apply(&mut self, distro: &str, event: AgentEvent, out: &SyncSender<Event>) {
        let ours = |key: &str| self.owned.get(key).is_some_and(|o| o.distro == distro);
        match event {
            AgentEvent::Output { key, line, .. } if ours(&key) => forward_line(&key, line, out),
            AgentEvent::Readiness { key, state, detail } if ours(&key) => {
                let _ = out.send(Event::Readiness(key.clone(), state));
                let _ = out.send(Event::Log(key, detail));
            }
            AgentEvent::Exited { key, code } if ours(&key) => {
                self.owned.remove(&key);
                let status = code.map_or_else(|| "Exited".to_owned(), |c| format!("Exited {c}"));
                let _ = out.send(Event::Status(key, status));
            }
            AgentEvent::Disconnected {
                expected: false, ..
            } => {
                self.lost.insert(distro.to_owned());
                for (key, owned) in &self.owned {
                    if owned.distro == distro {
                        let _ = out.send(Event::Log(
                            key.clone(),
                            "The Linux agent stopped; this process is reattached when it reconnects.".into(),
                        ));
                    }
                }
            }
            AgentEvent::Hello(_) if self.lost.remove(distro) => self.reattach(distro, out),
            AgentEvent::Dropped(n) => {
                for (key, owned) in &self.owned {
                    if owned.distro == distro {
                        let _ = out.send(Event::Log(
                            key.clone(),
                            format!("[{n} output lines of the Linux processes were dropped while the app was busy]"),
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    /// After an agent restart: follows every process this session owned in `distro` again.
    /// One that ended meanwhile is reported as exited when its adopt fails.
    fn reattach(&mut self, distro: &str, out: &SyncSender<Event>) {
        let keys: Vec<(String, bool)> = self
            .owned
            .iter()
            .filter(|(_, o)| o.distro == distro)
            .map(|(k, o)| (k.clone(), o.exclusive))
            .collect();
        for (key, exclusive) in keys {
            self.adopt(&key, distro, exclusive, out);
            if let Some(owned) = self.owned.get_mut(&key) {
                owned.reattach = true;
            }
        }
    }
}

/// Runs each profile's `probe` inside WSL through the agent; `false` when it cannot run.
pub(crate) fn probe(distro: &str, profiles: &[Profile]) -> Vec<(String, bool)> {
    let checks: Vec<Value> = profiles
        .iter()
        .filter(|p| !p.probe.is_empty())
        .map(|p| json!({"id":p.id,"bash":p.probe}))
        .collect();
    let found: HashMap<String, bool> = match (checks.is_empty(), shared(distro)) {
        (false, Some(client)) => client
            .call(
                "probe",
                json!({"checks":checks,"timeout_s":20}),
                Duration::from_secs(60),
            )
            .ok()
            .and_then(|rows| rows.as_array().cloned())
            .into_iter()
            .flatten()
            .filter_map(|row| Some((row["id"].as_str()?.to_owned(), row["ok"].as_bool()?)))
            .collect(),
        _ => HashMap::new(),
    };
    profiles
        .iter()
        .map(|p| (p.id.clone(), found.get(&p.id).copied().unwrap_or(false)))
        .collect()
}

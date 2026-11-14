//! Re-simulation: a saved flight flown again in a fresh simulator. A recording of a simulated
//! system keeps a recipe beside its MCAP (`recipe`), so a later run can start the same
//! autopilot, world and parameters and replay the commands it recorded (`run`), shown in
//! Data & models → Re-simulation (`view`).
use crate::systems::adapter::Adapter;
use crate::{State, activity};
use rune::TextEdit;
use serde::Deserialize;
use serde_json::Value;
use std::sync::mpsc::{self, Receiver, Sender};

pub(crate) mod compare;
pub(crate) mod import;
pub(crate) mod recipe;
pub(crate) mod report;
pub(crate) mod run;
#[cfg(test)]
mod tests;
pub(crate) mod transfer;
pub(crate) mod view;

/// `ui.data_tab` of Data & models → Re-simulation.
pub(crate) const TAB: usize = 5;
/// The world parameter of a run's temporary system that carries the recorded home position
/// ("lat,lon,alt"); `launch_env` turns it into the adapter's home variables.
pub(crate) const HOME: &str = "resim_home";
/// The run's own localhost-only ROS domain (capture 0, replay 77, inference 78).
pub(crate) const DOMAIN: u32 = 79;
/// The fresh PX4's DDS namespace: only the arbiter publishes into it.
pub(crate) const NAMESPACE: &str = "resim";

/// A previous re-simulation of one flight, as `resim.list` reports it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Run {
    pub name: String,
    pub path: String,
    pub status: String,
    pub created: String,
    pub commands: u64,
    pub sent: u64,
    pub error: String,
    /// The project that ran in shadow, if any.
    pub code: String,
    /// "<t> s, <topic>: <reason>" when your code took over.
    pub takeover: String,
    /// The adapter the run flew in, and the conditions it changed.
    pub target: String,
    pub conditions: Vec<String>,
    /// `same`, `changed` or `no data` once compared.
    pub verdict: String,
    pub split_t: Option<f64>,
    pub compare_error: String,
}

/// A recording that keeps a recipe.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Flight {
    pub id: String,
    pub label: String,
    pub path: String,
    /// The recipe summary (`recipes_summary` in `scripts/recordings.py`).
    pub recipe: Value,
    pub runs: Vec<Run>,
}
impl Flight {
    pub fn adapter(&self) -> Option<&'static crate::systems::adapter::Adapter> {
        let id = self.recipe["adapter"].as_str()?;
        crate::systems::adapter::ALL.iter().find(|a| a.id == id)
    }
}

pub(crate) enum Msg {
    Listed(Result<Value, String>),
    Prepared(Result<Value, String>),
    Progress(String),
    Done(Result<Value, String>),
    /// A real flight filed in the library (`resim.import`).
    Imported(Result<Value, String>),
    /// A run's comparison (`resim.report`), by run name.
    Report(String, Result<Value, String>),
}

pub(crate) struct Resim {
    pub flights: Vec<Flight>,
    pub listed: bool,
    pub listing: bool,
    pub selected: Option<String>,
    pub run: Option<run::Active>,
    pub message: String,
    /// Your code: the ROS 2 project started in shadow on the run's domain, or none.
    pub code: Option<String>,
    /// The takeover tolerance is on (off: the recording steers the whole flight).
    pub takeover: bool,
    /// Percent your code may differ before it takes over.
    pub tolerance: TextEdit,
    /// Absolute difference always allowed, in the field's units.
    pub floor: TextEdit,
    /// Seconds a difference must last.
    pub hold: TextEdit,
    /// The world to fly in (an adapter id with the recorded autopilot); `None`: as recorded.
    pub target: Option<String>,
    /// Conditions turned on (`adapter::CONDITIONS` keys).
    pub conditions: Vec<String>,
    /// AI topics to compare (your nodes' output), space or comma separated.
    pub ai_topics: TextEdit,
    /// The comparison on screen: run name and the report, or why it is not available.
    pub report: Option<(String, Result<compare::Report, String>)>,
    pub reporting: bool,
    pub importing: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}
impl Default for Resim {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            flights: Vec::new(),
            listed: false,
            listing: false,
            selected: None,
            run: None,
            message: String::new(),
            code: None,
            takeover: false,
            tolerance: TextEdit::new(DEFAULT_TOLERANCE),
            floor: TextEdit::new("0.2"),
            hold: TextEdit::new("0.5"),
            target: None,
            conditions: Vec::new(),
            ai_topics: TextEdit::new(""),
            report: None,
            reporting: false,
            importing: false,
            tx,
            rx,
        }
    }
}
impl Resim {
    pub(crate) fn flight(&self) -> Option<&Flight> {
        let id = self.selected.as_deref()?;
        self.flights.iter().find(|f| f.id == id)
    }
    /// Where the next run flies.
    pub(crate) fn target(&self) -> run::Target {
        run::Target {
            adapter: self
                .target
                .as_deref()
                .and_then(|id| crate::systems::adapter::ALL.iter().find(|a| a.id == id)),
            conditions: self.conditions.clone(),
        }
    }
    /// (tolerance in percent or `None` for off, floor, hold seconds), or why the fields are not
    /// usable.
    pub(crate) fn settings(&self) -> Result<(Option<f64>, f64, f64), String> {
        let read = |edit: &TextEdit, name: &str, max: f64| {
            let text = edit.text.trim();
            text.parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && (0.0..=max).contains(v))
                .ok_or_else(|| format!("{name} must be a number from 0 to {max}."))
        };
        if !self.takeover {
            return Ok((None, 0.0, 0.5));
        }
        Ok((
            Some(read(&self.tolerance, "Tolerance (%)", 100.0)?),
            read(&self.floor, "The floor", 1000.0)?,
            read(&self.hold, "The hold time (s)", 60.0)?,
        ))
    }
}

/// The AI topics to compare, or why one is not a topic name.
pub(crate) fn ai_topics(text: &str) -> Result<Vec<String>, String> {
    let topics: Vec<String> = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    if let Some(bad) = topics.iter().find(|t| !t.starts_with('/') || t.len() < 2) {
        return Err(format!(
            "AI topics: {bad} is not an absolute topic name such as /drone_deck/detections."
        ));
    }
    if topics.len() > 20 {
        return Err("AI topics: up to 20.".into());
    }
    Ok(topics)
}

/// Loads the comparison of `run` of the selected flight.
pub(crate) fn open_report(s: &mut State, run: &str) {
    let Some(flight) = s.resim.flight().map(|f| f.id.clone()) else {
        return;
    };
    s.resim.report = Some((run.to_string(), Err("Loading the comparison…".into())));
    request_report(s, "resim.report", flight, run.to_string());
}

/// Compares the run on screen again, then shows the new report.
pub(crate) fn compare_again(s: &mut State) {
    let (Some(flight), Some((run, _))) = (
        s.resim.flight().map(|f| f.id.clone()),
        s.resim.report.clone(),
    ) else {
        return;
    };
    s.resim.report = Some((run.clone(), Err("Comparing the two flights…".into())));
    request_report(s, "resim.compare", flight, run);
}

fn request_report(s: &mut State, method: &'static str, flight: String, run: String) {
    if s.headless || !s.wsl_ready() {
        s.resim.report = Some((run, Err("The Linux environment is not available.".into())));
        return;
    }
    s.resim.reporting = true;
    let (tx, distro) = (s.resim.tx.clone(), s.config.distro.clone());
    std::thread::spawn(move || {
        let params = serde_json::json!({"recording": flight, "run": run});
        let call = |m: &str| match crate::agent::shared(&distro) {
            Some(client) => client
                .call(m, params.clone(), std::time::Duration::from_secs(660))
                .map_err(|e| e.to_string()),
            None => Err("The Linux agent is not available in this mode.".into()),
        };
        let reply = if method == "resim.compare" {
            call(method).and_then(|_| call("resim.report"))
        } else {
            call(method)
        };
        let _ = tx.send(Msg::Report(run, reply));
    });
}

/// The takeover tolerance a simulated flight starts with (percent).
pub(crate) const DEFAULT_TOLERANCE: &str = "10";

/// The worlds `flight` can fly in: every adapter of its autopilot, recorded one first, each
/// with why it cannot be used, if it cannot.
pub(crate) fn targets(flight: &Flight) -> Vec<(&'static Adapter, Result<(), String>)> {
    let Some(recorded) = flight.adapter() else {
        return Vec::new();
    };
    let commands = recorded.ros.as_ref().map(|r| r.commands);
    let mut out: Vec<_> = crate::systems::adapter::ALL
        .iter()
        .filter(|a| a.autopilot == recorded.autopilot)
        .map(|a| {
            let ok = if a.ros.as_ref().map(|r| r.commands) != commands {
                Err(format!(
                    "{} does not take the recorded ROS 2 commands.",
                    a.name
                ))
            } else {
                Ok(())
            };
            (a, ok)
        })
        .collect();
    out.sort_by_key(|(a, _)| a.id != recorded.id);
    out
}

/// Each condition with whether `adapter` offers it, and why not.
pub(crate) fn conditions(
    adapter: &Adapter,
) -> Vec<(&'static str, &'static str, Result<&'static str, String>)> {
    crate::systems::adapter::CONDITIONS
        .iter()
        .map(|(key, label)| {
            let offered = adapter.condition(key).map(|c| c.desc).ok_or_else(|| {
                format!(
                    "{} has no {} setting Drone Deck can change.",
                    adapter.name,
                    label.to_lowercase()
                )
            });
            (*key, *label, offered)
        })
        .collect()
}

/// ROS 2 projects in the selected distribution: the code a run can start in shadow.
pub(crate) fn projects(s: &State) -> Vec<serde_json::Value> {
    s.studio.library["projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["kind"] == "ros2" && p["distro"].as_str() == Some(s.config.distro.as_str()))
        .cloned()
        .collect()
}

/// Extra environment for profile `id` of a re-simulation's temporary system: the fresh PX4
/// takes the namespace and the isolated domain, and ROS tools stay on this computer.
pub(crate) fn launch_env(id: &str, sys: &crate::systems::System) -> Vec<(String, String)> {
    if !sys.scratch() {
        return Vec::new();
    }
    let mut env = vec![
        ("ROS_AUTOMATIC_DISCOVERY_RANGE".into(), "LOCALHOST".into()),
        ("ROS_LOCALHOST_ONLY".into(), "1".into()),
    ];
    if crate::systems::vehicle_profile(sys) == Some(id) {
        env.push(("PX4_UXRCE_DDS_NS".into(), NAMESPACE.into()));
        env.push(("ROS_DOMAIN_ID".into(), DOMAIN.to_string()));
        let home = sys.param(HOME).and_then(|h| {
            let parts: Vec<&str> = h.split(',').collect();
            (parts.len() == 3 && parts.iter().all(|p| p.parse::<f64>().is_ok())).then_some(parts)
        });
        if let (Some(env_names), Some(parts)) = (sys.adapter().and_then(|a| a.home.as_ref()), home)
        {
            for (name, value) in [env_names.lat, env_names.lon, env_names.alt]
                .into_iter()
                .zip(parts)
            {
                env.push((name.into(), value.into()));
            }
        }
    }
    env
}

/// The takeover tolerance a real flight starts with (percent): it cannot be flown again in
/// the world it was flown in.
pub(crate) const REAL_TOLERANCE: &str = "20";

/// Selects flight `id`; a tolerance still at a default follows the flight's origin.
pub(crate) fn select(s: &mut State, id: &str) {
    s.resim.selected = Some(id.to_string());
    let real = s
        .resim
        .flight()
        .is_some_and(|f| f.recipe["origin"] == "real");
    let text = s.resim.tolerance.text.trim().to_string();
    if text == DEFAULT_TOLERANCE || text == REAL_TOLERANCE {
        s.resim.tolerance = TextEdit::new(if real {
            REAL_TOLERANCE
        } else {
            DEFAULT_TOLERANCE
        });
    }
}

/// Opens Data & models → Re-simulation on the recording `id` (Recordings' shortcut).
pub(crate) fn open(s: &mut State, id: &str) {
    s.resim.selected = Some(id.to_string());
    s.resim.listed = false;
    s.ui.data_tab = TAB;
    s.navigate(crate::ui::Page::Data);
}

/// Requests `resim.list` (recordings with a recipe and their runs).
pub(crate) fn refresh(s: &mut State) {
    if s.resim.listing || s.headless || !s.wsl_ready() {
        return;
    }
    s.resim.listing = true;
    let (tx, distro) = (s.resim.tx.clone(), s.config.distro.clone());
    std::thread::spawn(move || {
        let reply = match crate::agent::shared(&distro) {
            Some(client) => client
                .call(
                    "resim.list",
                    serde_json::json!({}),
                    std::time::Duration::from_secs(30),
                )
                .map_err(|e| e.to_string()),
            None => Err("The Linux agent is not available in this mode.".into()),
        };
        let _ = tx.send(Msg::Listed(reply));
    });
}

/// Applies agent replies and advances a run; true when visible state changed.
pub(crate) fn poll(s: &mut State) -> bool {
    let mut changed = false;
    let on_tab = s.page == crate::ui::Page::Data && s.ui.data_tab == TAB;
    if on_tab && !s.resim.listed && !s.resim.listing {
        refresh(s);
        changed = s.resim.listing;
    }
    // Your code is chosen from the ROS 2 projects (`data.library`).
    if on_tab
        && !s.headless
        && s.wsl_ready()
        && !s.studio.pending
        && !s.studio.loaded.contains(&"data_library")
    {
        s.studio.loaded.push("data_library");
        crate::studio::request(s, "data_library");
        changed = true;
    }
    let messages: Vec<Msg> = s.resim.rx.try_iter().take(100).collect();
    for message in messages {
        changed = true;
        match message {
            Msg::Listed(reply) => {
                let r = &mut s.resim;
                r.listing = false;
                r.listed = true;
                match reply {
                    Ok(v) => {
                        r.flights = v["flights"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|f| serde_json::from_value(f.clone()).ok())
                            .collect();
                        if r.flight().is_none() {
                            r.selected = r.flights.first().map(|f| f.id.clone());
                        }
                    }
                    Err(e) => r.message = format!("Could not list re-simulable flights: {e}"),
                }
                if let Some(id) = s.resim.selected.clone() {
                    select(s, &id);
                }
            }
            Msg::Prepared(reply) => run::prepared(s, reply),
            Msg::Progress(text) => {
                if let Some(active) = &mut s.resim.run {
                    active.progress = text;
                }
            }
            Msg::Done(reply) => run::done(s, reply),
            Msg::Imported(reply) => {
                s.resim.importing = false;
                match reply {
                    Ok(v) => {
                        let id = v["id"].as_str().unwrap_or("").to_string();
                        let found = import::counted(&v["counts"]);
                        s.resim.message = format!(
                            "Imported {} as a real flight{}. It flies in PX4's built-in physics at its home position by default; real wind and the real vehicle cannot be reproduced.",
                            v["label"].as_str().unwrap_or("the flight"),
                            if found.is_empty() {
                                String::new()
                            } else {
                                format!(" ({found})")
                            }
                        );
                        activity::record(s, format!("Imported real flight {id} for re-simulation"));
                        select(s, &id);
                    }
                    Err(e) => s.resim.message = format!("Could not import the flight: {e}"),
                }
                s.resim.listed = false;
            }
            Msg::Report(name, reply) => {
                s.resim.reporting = false;
                let report = reply.and_then(|v| {
                    serde_json::from_value::<compare::Report>(v)
                        .map_err(|e| format!("The comparison is unreadable ({e})."))
                });
                if s.resim.report.as_ref().is_some_and(|(n, _)| *n == name) {
                    s.resim.report = Some((name, report));
                }
                s.resim.listed = false;
            }
        }
    }
    changed | run::poll(s)
}

/// A comparison `compare.py` wrote for two synthetic flights (also the tests' fixture).
pub(crate) const PREVIEW_REPORT: &str = include_str!("compare_fixture.json");

/// Headless preview of a run's comparison, scrolled to it.
pub(crate) fn preview_report(s: &mut State) {
    preview(s);
    let report = serde_json::from_str(PREVIEW_REPORT).map_err(|e| e.to_string());
    s.resim.report = Some(("20261114T120000Z".into(), report));
}

/// Headless preview: two flights, one with a finished run.
pub(crate) fn preview(s: &mut State) {
    s.resim.listed = true;
    s.resim.flights = vec![
        Flight {
            id: "20261114T101500Z-offboard".into(),
            label: "offboard square".into(),
            path: "/home/pilot/.local/share/drone-deck/recordings/20261114T101500Z-offboard".into(),
            recipe: serde_json::json!({"adapter": "px4-gazebo", "autopilot": "px4", "world": "gazebo",
                "system": "PX4 + Gazebo with ROS 2", "commands": 3, "params": true}),
            runs: vec![Run {
                name: "20261114T120000Z".into(),
                path: "/home/pilot/.local/share/drone-deck/recordings/20261114T101500Z-offboard/resim/20261114T120000Z".into(),
                status: "Completed".into(),
                created: "2026-11-14T12:00:00Z".into(),
                commands: 2412,
                sent: 1790,
                error: String::new(),
                code: "avoid-v2".into(),
                takeover: "42.3 s, /fmu/in/trajectory_setpoint: trajectory_setpoint.velocity[0]: recorded 2, yours 2.6.".into(),
                target: "px4-gazebo".into(),
                conditions: vec!["wind".into()],
                verdict: "changed".into(),
                split_t: Some(44.1),
                compare_error: String::new(),
            }],
        },
        Flight {
            id: "20261113T164000Z-takeoff".into(),
            label: "takeoff and hold".into(),
            path: "/home/pilot/.local/share/drone-deck/recordings/20261113T164000Z-takeoff".into(),
            recipe: serde_json::json!({"adapter": "px4-sih", "autopilot": "px4", "world": "internal",
                "system": "Local simulation", "commands": 2, "params": false}),
            runs: Vec::new(),
        },
    ];
    s.resim.selected = Some(s.resim.flights[0].id.clone());
    s.resim.takeover = true;
    s.studio.library = serde_json::json!({"projects": [{"name": "avoid-v2", "kind": "ros2",
        "distro": s.config.distro, "path": "/home/pilot/.local/share/drone-deck/projects/avoid-v2"}]});
    s.resim.code = Some("avoid-v2".into());
    s.ui.data_tab = TAB;
    s.navigate(crate::ui::Page::Data);
}

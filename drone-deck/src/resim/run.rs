//! One re-simulation run, start to finish:
//!
//! 1. `resim.prepare` reads the flight's recipe and kept parameters in WSL.
//! 2. A temporary system (`Systems::add_scratch`) with the recorded autopilot and world goes
//!    through the normal staged launch, on the run's isolated domain and DDS namespace.
//! 3. Once the vehicle link has `OwnedSimulation` authority, the fresh autopilot's parameters
//!    are read and the recorded ones the transfer rules allow are written (`transfer`).
//! 4. `resim.run` starts the arbiter and a recorder of the fresh vehicle, and returns when the
//!    replay ends.
//! 5. The run's processes stop, the temporary system goes and the previous one is active again.
//!
//! One run at a time; it refuses to start while any other work or link runs.
use super::{DOMAIN, HOME, Msg, NAMESPACE, recipe::Recipe, transfer};
use crate::systems::adapter::{Adapter, AutopilotKind};
use crate::systems::model::{Autopilot, Consumers, Ros, SCRATCH, World, template};
use crate::systems::{self, SIMULATION, System, Vehicle};
use crate::{State, activity, vehicle};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const CANCELLED: &str = "cancelled";
const PREPARE_TIMEOUT: Duration = Duration::from_secs(60);
/// The longest whole flight a run waits for.
const RUN_TIMEOUT: Duration = Duration::from_secs(6 * 3600);
/// From the end of the staged launch to the simulation authority, and for each parameter step.
const STEP_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Phase {
    Preparing,
    Launching,
    ReadingParams,
    WritingParams,
    StartingCode,
    Replaying,
    Stopping,
}
impl Phase {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Preparing => "Reading the flight's recipe",
            Self::Launching => "Starting the fresh simulator",
            Self::ReadingParams => "Reading the fresh autopilot's parameters",
            Self::WritingParams => "Writing the recorded parameters",
            Self::StartingCode => "Starting your code",
            Self::Replaying => "Replaying the recorded commands",
            Self::Stopping => "Stopping the run's processes",
        }
    }
}

pub(crate) struct Active {
    pub flight: String,
    pub label: String,
    pub phase: Phase,
    pub progress: String,
    /// When the phase began.
    pub since: Instant,
    /// The system that was active before the run.
    previous: String,
    recorded: Option<crate::mavlink::ParamSet>,
    /// The recorded parameters left out and why, kept in run.json.
    skipped: Vec<(String, &'static str)>,
    cancel: Arc<AtomicBool>,
    /// The final message, kept while the processes stop.
    outcome: String,
    /// The project started in shadow, and the settings the run was started with.
    code: Option<serde_json::Value>,
    settings: (Option<f64>, f64, f64),
    /// Where it flies, and whether that is the recorded world (parameter transfer).
    target: Target,
    same_world: bool,
    /// The run flies the recorded home position.
    home: bool,
}

/// Why a run cannot start now, or `None`.
pub(crate) fn refusal(s: &State) -> Option<String> {
    let r = &s.resim;
    if r.run.is_some() {
        return Some("A re-simulation is already running. One at a time.".into());
    }
    let Some(flight) = r.flight() else {
        return Some("Choose a flight first.".into());
    };
    let Some(a) = flight.adapter() else {
        return Some(
            "This version of Drone Deck has no adapter for the flight's simulator.".into(),
        );
    };
    if a.autopilot != AutopilotKind::Px4 {
        return Some("Re-simulating ArduPilot flights is not supported yet.".into());
    }
    if flight.recipe["commands"].as_u64().unwrap_or(0) == 0 {
        return Some("The recording kept no ROS 2 commands to replay.".into());
    }
    if let Err(e) = r.settings() {
        return Some(e);
    }
    if let Err(e) = super::ai_topics(&r.ai_topics.text) {
        return Some(e);
    }
    if let Some(Err(why)) = r
        .target
        .as_deref()
        .and_then(|id| super::targets(flight).into_iter().find(|(t, _)| t.id == id))
        .map(|(_, ok)| ok)
    {
        return Some(why);
    }
    if let Some(name) = &r.code
        && !super::projects(s)
            .iter()
            .any(|p| p["name"] == name.as_str())
    {
        return Some(format!(
            "Your code: the ROS 2 project {name} is not in this distribution's library."
        ));
    }
    if s.headless {
        return Some("Headless preview · re-simulation runs nothing.".into());
    }
    if !s.wsl_ready() {
        return Some("Select a working WSL 2 distribution in Settings → Environment first.".into());
    }
    if systems::active_work(s) || s.vehicle.connected() {
        return Some(
            "Stop the running session first: a re-simulation starts its own simulator and link."
                .into(),
        );
    }
    None
}

/// Where a run flies: the target world (`None`: as recorded) and the conditions turned on.
#[derive(Clone, Debug, Default)]
pub(crate) struct Target {
    pub adapter: Option<&'static Adapter>,
    pub conditions: Vec<String>,
}
impl Target {
    /// The adapter a run of `r` flies: the target's, or the recorded one.
    pub(crate) fn adapter_for(&self, r: &Recipe) -> Option<&'static Adapter> {
        self.adapter.or_else(|| {
            crate::systems::adapter::ALL
                .iter()
                .find(|a| a.id == r.adapter)
        })
    }
    /// Whether the run flies the world `r` was recorded in.
    pub(crate) fn same_world(&self, r: &Recipe) -> bool {
        self.adapter_for(r).is_some_and(|a| a.id == r.adapter)
    }
}

/// The temporary system that flies `r` in `target`: the recorded autopilot, the target world
/// (with the recorded world's details when it is the same one), the conditions' world
/// parameters, the recorded home position where the world takes one, the live view and ROS 2
/// on the run's domain; no QGroundControl, scripts address, recorder or Foxglove.
pub(crate) fn system_for(r: &Recipe, target: &Target) -> System {
    let mut sys = template(
        SIMULATION,
        SCRATCH,
        &format!("Re-simulation of {}", r.system),
    );
    let autopilot = match r.autopilot {
        AutopilotKind::Px4 => Autopilot::Px4,
        AutopilotKind::ArduPilot => Autopilot::ArduPilot {
            vehicle: r.vehicle.clone().unwrap_or_else(|| "copter".into()),
        },
    };
    let adapter = target.adapter_for(r);
    let mut world: World = match adapter {
        Some(a) if a.id != r.adapter => World {
            id: a.world.into(),
            params: Default::default(),
        },
        _ => r.world.clone(),
    };
    if let Some(a) = adapter {
        for c in target.conditions.iter().filter_map(|k| a.condition(k)) {
            for (key, value) in c.set {
                world.params.insert((*key).into(), (*value).into());
            }
        }
        if let (Some(_), Some(home)) = (&a.home, r.home) {
            world.params.insert(
                HOME.into(),
                format!("{},{},{}", home.lat, home.lon, home.alt),
            );
        }
    }
    sys.vehicle = Vehicle::Simulated { autopilot, world };
    sys.consumers = Consumers {
        live_view: true,
        ros: Ros {
            agent: true,
            recorder: false,
            domain: DOMAIN,
        },
        ..Default::default()
    };
    sys.prepared = true;
    sys
}

fn agent_call(
    s: &State,
    method: &'static str,
    params: Value,
    timeout: Duration,
    wrap: fn(Result<Value, String>) -> Msg,
) {
    let (tx, distro) = (s.resim.tx.clone(), s.config.distro.clone());
    std::thread::spawn(move || {
        let reply = match crate::agent::shared(&distro) {
            Some(client) => client
                .call(method, params, timeout)
                .map_err(|e| e.to_string()),
            None => Err("The Linux agent is not available in this mode.".into()),
        };
        let _ = tx.send(wrap(reply));
    });
}

/// Starts a run of the selected flight.
pub(crate) fn start(s: &mut State) {
    if let Some(why) = refusal(s) {
        s.resim.message = why;
        return;
    }
    let flight = s
        .resim
        .flight()
        .cloned()
        .expect("refusal checks the flight");
    let settings = s.resim.settings().expect("refusal checks the settings");
    let code = s.resim.code.as_ref().and_then(|name| {
        super::projects(s)
            .into_iter()
            .find(|p| p["name"] == name.as_str())
    });
    if let Some(project) = &code
        && let Err(e) = crate::studio::projects::profile_for(s, project)
    {
        s.resim.message = format!("Your code cannot start: {e}");
        return;
    }
    s.resim.run = Some(Active {
        flight: flight.id.clone(),
        label: flight.label.clone(),
        phase: Phase::Preparing,
        progress: String::new(),
        since: Instant::now(),
        previous: s.systems.active_id().to_string(),
        recorded: None,
        skipped: Vec::new(),
        cancel: Arc::new(AtomicBool::new(false)),
        outcome: String::new(),
        code,
        settings,
        target: s.resim.target(),
        same_world: true,
        home: false,
    });
    s.resim.message = format!("Re-simulating {}…", flight.label);
    agent_call(
        s,
        "resim.prepare",
        json!({"recording": flight.id}),
        PREPARE_TIMEOUT,
        Msg::Prepared,
    );
    activity::record(s, format!("Started a re-simulation of {}", flight.label));
}

/// Stops the run: the replay is cancelled, then the processes stop.
pub(crate) fn stop(s: &mut State) {
    let Some(active) = &mut s.resim.run else {
        return;
    };
    match active.phase {
        Phase::Replaying => {
            active.cancel.store(true, Ordering::SeqCst);
            active.progress = "Stopping the replay…".into();
        }
        Phase::Stopping => {}
        _ => finish(s, "Re-simulation stopped before the replay began.".into()),
    }
}

pub(super) fn prepared(s: &mut State, reply: Result<Value, String>) {
    let Some(active) = &s.resim.run else {
        return;
    };
    if active.phase != Phase::Preparing {
        return;
    }
    let recipe = reply.and_then(|v| {
        let recipe: Recipe = serde_json::from_value(v["recipe"].clone())
            .map_err(|e| format!("its recipe is unreadable ({e})"))?;
        let params = v["params"]
            .as_str()
            .map(|t| crate::mavlink::ParamSet::from_qgc(t).0);
        Ok((recipe, params))
    });
    let (recipe, params) = match recipe {
        Ok(x) => x,
        Err(e) => {
            s.resim.run = None;
            s.resim.message = format!("Could not prepare the re-simulation: {e}");
            return;
        }
    };
    let target = s
        .resim
        .run
        .as_ref()
        .map(|a| a.target.clone())
        .unwrap_or_default();
    let sys = system_for(&recipe, &target);
    let home =
        matches!(&sys.vehicle, Vehicle::Simulated { world, .. } if world.params.contains_key(HOME));
    if let Some(active) = s.resim.run.as_mut() {
        active.same_world = target.same_world(&recipe);
        active.home = home;
    }
    s.systems.add_scratch(sys);
    s.systems.set_active(SCRATCH);
    systems::start_workspace(s);
    let active = s.resim.run.as_mut().expect("checked above");
    active.recorded = params;
    if s.session.is_none() {
        // The launch was refused; its reason is in the notice.
        let why = s.notice.clone();
        finish(s, format!("The fresh simulator did not start: {why}"));
        return;
    }
    enter(active, Phase::Launching);
}

fn enter(active: &mut Active, phase: Phase) {
    active.phase = phase;
    active.since = Instant::now();
    active.progress.clear();
}

pub(super) fn done(s: &mut State, reply: Result<Value, String>) {
    let Some(active) = &s.resim.run else {
        return;
    };
    let skipped = active.skipped.len();
    let text = match reply {
        Ok(v) => format!(

            "Re-simulation {}: {} of {} recorded commands replayed{}{}. Saved in {}.",
            v["status"].as_str().unwrap_or("finished").to_lowercase(),
            v["sent"].as_u64().unwrap_or(0),
            v["commands"].as_u64().unwrap_or(0),
            match v["takeover"].as_object() {
                Some(t) => format!(
                    "; your code took over at {} s ({})",
                    t.get("t").and_then(Value::as_f64).unwrap_or(0.0),
                    t.get("reason").and_then(Value::as_str).unwrap_or("")
                ),
                None if v["tolerance"].is_number() => "; your code never differed enough to take over".into(),
                None => String::new(),
            },
            if skipped > 0 {
                format!("; {skipped} recorded parameters were not copied (see run.json)")
            } else {
                String::new()
            },
            v["folder"].as_str().unwrap_or("the recording's resim folder"),
        ),
        Err(e) if e == CANCELLED => {
            "Re-simulation stopped. What the fresh vehicle recorded so far is kept; the run is marked Cancelled.".into()
        }
        Err(e) => format!("Re-simulation failed: {e}"),
    };
    finish(s, text);
}

/// Stops the run's processes and link; the temporary system goes once they have stopped.
fn finish(s: &mut State, outcome: String) {
    s.stop_all();
    if let Some(active) = &mut s.resim.run {
        active.outcome = outcome.clone();
        enter(active, Phase::Stopping);
    }
    s.resim.message = outcome;
}

fn owned_running(s: &State) -> bool {
    s.status.iter().any(|(id, st)| {
        matches!(st.as_str(), "Running" | "Starting") && s.systems.owner(id) == Some(SCRATCH)
    })
}

/// Advances the run; true when visible state changed.
pub(super) fn poll(s: &mut State) -> bool {
    let Some(active) = &s.resim.run else {
        return false;
    };
    let late = active.since.elapsed() > STEP_TIMEOUT;
    match active.phase {
        Phase::Preparing | Phase::Replaying => false,
        Phase::Launching => {
            if s.session.is_some() {
                return false;
            }
            if let Some((stage, why)) = s.launch_failure.clone() {
                finish(s, format!("The fresh simulator stopped at {stage}: {why}"));
            } else if vehicle::authority(s).may_fly() {
                let has_params = s.resim.run.as_ref().is_some_and(|a| a.recorded.is_some());
                if has_params {
                    s.vehicle.params = None;
                    if let Err(e) = vehicle::fetch_params(s) {
                        finish(
                            s,
                            format!("Could not read the fresh autopilot's parameters: {e}"),
                        );
                        return true;
                    }
                    enter(s.resim.run.as_mut().unwrap(), Phase::ReadingParams);
                } else {
                    after_params(s);
                }
            } else if late {
                finish(
                    s,
                    "The vehicle link to the fresh simulator did not come up.".into(),
                );
            } else {
                return false;
            }
            true
        }
        Phase::ReadingParams => {
            if let Some(fresh) = s.vehicle.params.clone() {
                let active = s.resim.run.as_mut().unwrap();
                let recorded = active.recorded.take().unwrap_or_default();
                let plan = transfer::plan(&recorded, Some(&fresh), active.same_world);
                active.skipped = plan
                    .skipped
                    .iter()
                    .map(|(n, why)| (n.clone(), why.reason()))
                    .collect();
                if plan.apply.is_empty() {
                    after_params(s);
                } else {
                    let count = plan.apply.len();
                    if let Err(e) = vehicle::set_params(s, plan.apply) {
                        finish(s, format!("Could not write the recorded parameters: {e}"));
                        return true;
                    }
                    let active = s.resim.run.as_mut().unwrap();
                    enter(active, Phase::WritingParams);
                    active.progress = format!("{count} parameters differ from the fresh autopilot");
                }
                true
            } else if late {
                finish(s, "The fresh autopilot's parameters did not arrive.".into());
                true
            } else {
                false
            }
        }
        Phase::WritingParams => {
            if let Some((_, failed)) = s.vehicle.params_written.clone() {
                if let Some(active) = &mut s.resim.run {
                    active.skipped.extend(
                        failed
                            .into_iter()
                            .map(|n| (n, "not confirmed by the fresh autopilot")),
                    );
                }
                after_params(s);
                true
            } else if late {
                finish(
                    s,
                    "The fresh autopilot did not confirm the parameter writes.".into(),
                );
                true
            } else {
                false
            }
        }
        Phase::StartingCode => {
            let id = s
                .resim
                .run
                .as_ref()
                .and_then(|a| a.code.as_ref())
                .and_then(|p| p["name"].as_str())
                .map(|n| format!("project-{n}"))
                .unwrap_or_default();
            match s.status.get(&id).map(String::as_str) {
                Some("Running") => replay(s),
                Some(st) if st.starts_with("Exited") || st == "Failed" => finish(
                    s,
                    format!(
                        "Your code stopped before the replay began ({st}). See its output in Activity."
                    ),
                ),
                _ if late => finish(s, "Your code did not start.".into()),
                _ => return false,
            }
            true
        }
        Phase::Stopping => {
            // A process that never reports its stop does not keep the temporary system forever.
            if (owned_running(s) || (s.vehicle.connected() && s.vehicle.owned_by(SCRATCH))) && !late
            {
                return false;
            }
            let active = s.resim.run.take().unwrap();
            s.systems.drop_scratch(&active.previous);
            s.resim.message = active.outcome;
            s.resim.listed = false;
            true
        }
    }
}

/// Starts your code (when chosen) on the run's domain, then the replay.
fn after_params(s: &mut State) {
    let Some(project) = s.resim.run.as_ref().and_then(|a| a.code.clone()) else {
        return replay(s);
    };
    let profile = match crate::studio::projects::profile_for(s, &project) {
        Ok(p) => p,
        Err(e) => return finish(s, format!("Your code cannot start: {e}")),
    };
    let env = vec![
        ("ROS_DOMAIN_ID".to_string(), DOMAIN.to_string()),
        (
            "ROS_AUTOMATIC_DISCOVERY_RANGE".to_string(),
            "LOCALHOST".to_string(),
        ),
        ("ROS_LOCALHOST_ONLY".to_string(), "1".to_string()),
    ];
    s.systems.claim_for(&profile.id, SCRATCH);
    s.status.insert(profile.id.clone(), "Starting".into());
    s.log(format!(
        "[resim] Starting {} on ROS domain {DOMAIN}",
        profile.name
    ));
    s.send(crate::backend::Request::StartEnv(
        profile,
        s.config.distro.clone(),
        env,
    ));
    enter(s.resim.run.as_mut().unwrap(), Phase::StartingCode);
}

/// Starts the arbiter and the fresh recorder (`resim.run`); the reply arrives as `Msg::Done`.
fn replay(s: &mut State) {
    let active = s.resim.run.as_mut().unwrap();
    enter(active, Phase::Replaying);
    let skipped: Vec<Value> = active
        .skipped
        .iter()
        .map(|(name, why)| json!({"name": name, "reason": why}))
        .collect();
    let (tolerance, floor, hold) = active.settings;
    let mut params = json!({"recording": active.flight, "namespace": NAMESPACE, "domain": DOMAIN,
        "skipped_params": skipped, "tolerance": tolerance, "floor": floor, "hold": hold});
    if let Some(name) = active.code.as_ref().and_then(|p| p["name"].as_str()) {
        params["code"] = json!(name);
    }
    params["ai_topics"] = json!(super::ai_topics(&s.resim.ai_topics.text).unwrap_or_default());
    params["target"] = json!(active.target.adapter.map(|a| a.id));
    params["conditions"] = json!(active.target.conditions);
    params["home"] = json!(active.home);
    let cancel = active.cancel.clone();
    let (tx, distro) = (s.resim.tx.clone(), s.config.distro.clone());
    std::thread::spawn(move || {
        let Some(client) = crate::agent::shared(&distro) else {
            let _ = tx.send(Msg::Done(Err(
                "The Linux agent is not available in this mode.".into(),
            )));
            return;
        };
        let (progress, messages) = std::sync::mpsc::channel();
        let mut ticket =
            client.request_with_progress("resim.run", params, RUN_TIMEOUT, Some(progress));
        loop {
            if let Ok(message) = messages.recv_timeout(Duration::from_millis(100)) {
                let _ = tx.send(Msg::Progress(message));
            }
            if cancel.load(Ordering::SeqCst) {
                // The agent stops the arbiter and recorder and records the run as Cancelled;
                // wait until it no longer writes the run folder.
                ticket.cancel();
                let _ = tx.send(Msg::Progress("Stopping the replay…".into()));
                let until = Instant::now() + Duration::from_secs(30);
                while Instant::now() < until {
                    match client.call("resim.status", json!({}), Duration::from_secs(5)) {
                        Ok(v) if v["active"].as_bool() == Some(true) => {
                            std::thread::sleep(Duration::from_millis(500))
                        }
                        _ => break,
                    }
                }
                let _ = tx.send(Msg::Done(Err(CANCELLED.into())));
                return;
            }
            if let Some(reply) = ticket.try_take() {
                let _ = tx.send(Msg::Done(reply.map_err(|e| e.to_string())));
                return;
            }
        }
    });
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;

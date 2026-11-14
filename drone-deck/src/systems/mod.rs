//! Systems: user-created launch setups over the shared tool registry.
//!
//! A system names one vehicle and the consumers it feeds (`model`). A simulated vehicle
//! has an adapter (`adapter`): its launch stages, readiness, MAVLink route and ROS bridge. Its
//! components, connection chain and session stages derive from those (`components`); labels come from
//! observed state (`state`). `systems.json` is schema 3 (`file`). Processes belong to the
//! system that started them, so switching systems never stops anything.
use crate::*;
pub(crate) mod adapter;
pub(crate) mod compat;
mod components;
mod edit;
mod file;
pub(crate) mod model;
mod state;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod vehicle_tests;

pub(crate) use components::{
    Backing, Component, all_components, chain, component, components, filter_for, routed,
    session_stages, vehicle_profile,
};
pub(crate) use edit::{Draft, endpoint_ok};
pub(crate) use model::{AI, MAX_NAME, PHYSICAL, ROS, SIMULATION, System, VEHICLES, Vehicle};
pub(crate) use state::{Tone, profile_state, running, state, tone_color};
use std::collections::HashMap;

/// What `current()` answers while no system is saved: a recording vehicle with no listeners,
/// which starts nothing. Pages show "Build your first system" instead of using it.
fn placeholder() -> &'static System {
    static NONE: std::sync::OnceLock<System> = std::sync::OnceLock::new();
    NONE.get_or_init(|| System {
        id: String::new(),
        name: "No system yet".into(),
        prepared: false,
        vehicle: Vehicle::replay(None),
        consumers: model::Consumers::default(),
    })
}

/// Saved systems plus the runtime state that belongs to them.
pub(crate) struct Systems {
    pub list: Vec<System>,
    active_id: String,
    /// Owned work (profile id, `session`, `recorder`) → id of the system that started it.
    owners: HashMap<String, String>,
    /// Settings → Systems: the open editor and the system awaiting delete confirmation.
    pub editor: Option<Draft>,
    pub deleting: Option<String>,
    /// `systems.json` was written by a newer Drone Deck: the defaults run and nothing is saved
    /// over that file.
    newer_file: bool,
}
/// No systems: a fresh install, which opens the wizard.
impl Default for Systems {
    fn default() -> Self {
        Self::from_file(file::File {
            schema: file::SCHEMA,
            active: String::new(),
            systems: Vec::new(),
        })
    }
}
impl Systems {
    /// The four starters, first active: what tests and headless scenes start with.
    pub(crate) fn starters() -> Self {
        Self::from_file(file::starters())
    }
    fn from_file(f: file::File) -> Self {
        let mut s = Self {
            list: f.systems,
            active_id: f.active,
            owners: HashMap::new(),
            editor: None,
            deleting: None,
            newer_file: false,
        };
        s.sync();
        s
    }
    fn to_file(&self) -> file::File {
        let systems: Vec<System> = self.saved().cloned().collect();
        let active = if self.get(&self.active_id).is_some_and(System::scratch) {
            systems.first().map_or_else(String::new, |x| x.id.clone())
        } else {
            self.active_id.clone()
        };
        file::File {
            schema: file::SCHEMA,
            active,
            systems,
        }
    }
    /// The systems the user keeps: every one but a run's temporary system (`add_scratch`).
    pub fn saved(&self) -> impl Iterator<Item = &System> {
        self.list.iter().filter(|x| !x.scratch())
    }
    /// Adds a temporary system that a run (re-simulation) starts and owns. It is never saved
    /// or listed with the user's systems; `drop_scratch` removes it.
    pub(crate) fn add_scratch(&mut self, mut sys: System) {
        sys.id = model::SCRATCH.into();
        self.list.retain(|x| !x.scratch());
        self.list.push(sys);
    }
    /// Removes the temporary system, making `fallback` active if the temporary one was.
    pub(crate) fn drop_scratch(&mut self, fallback: &str) {
        let was_active = self.get(&self.active_id).is_some_and(System::scratch);
        self.list.retain(|x| !x.scratch());
        if was_active && self.get(fallback).is_some() {
            self.active_id = fallback.into();
        }
        self.sync();
    }
    /// Keeps the active id on a system that exists (none while the list is empty).
    fn sync(&mut self) {
        if !self.list.iter().any(|s| s.id == self.active_id) {
            self.active_id = self.list.first().map_or_else(String::new, |s| s.id.clone());
        }
    }
    /// Whether no system is saved (a fresh install): pages offer the wizard instead.
    pub fn is_empty(&self) -> bool {
        self.saved().next().is_none()
    }
    fn path() -> std::path::PathBuf {
        backend::root().join("systems.json")
    }
    /// Reads `systems.json`; an older schema is migrated and written back as the current one.
    /// A headless run (tests, scenes) starts with the four starters.
    pub fn load(headless: bool) -> Self {
        if headless {
            return Self::starters();
        }
        Self::load_at(&Self::path())
    }
    /// [`Systems::load`] from `path`; no file means no systems (a fresh install). A file of a
    /// newer schema is left in place with a `.kept` copy, and no systems are used.
    pub(crate) fn load_at(path: &std::path::Path) -> Self {
        let (f, migrated) = match crate::store::read(path, file::parse) {
            None => (None, false),
            Some(Ok((f, migrated))) => (Some(f), migrated),
            Some(Err(e)) => {
                let kept = crate::store::sibling(path, ".kept");
                let copy = if kept.exists() || std::fs::copy(path, &kept).is_ok() {
                    "; a copy is kept as systems.json.kept"
                } else {
                    ""
                };
                crate::store::note(format!(
                    "{e}. It was left unchanged{copy}; no systems are loaded and new ones are not saved."
                ));
                let mut s = Self::default();
                s.newer_file = true;
                return s;
            }
        };
        let s = f.map(Self::from_file).unwrap_or_default();
        if migrated {
            if let Err(e) = s.save_at(path) {
                crate::store::note(format!(
                    "systems.json was upgraded in memory but could not be saved: {e}"
                ));
            }
        }
        s
    }
    pub fn save(&self, headless: bool) -> Result<(), String> {
        if headless {
            return Ok(());
        }
        self.save_at(&Self::path())
    }
    pub(crate) fn save_at(&self, path: &std::path::Path) -> Result<(), String> {
        if self.newer_file {
            return Err(
                "systems.json was written by a newer Drone Deck and is left unchanged".into(),
            );
        }
        crate::store::write_json(path, &self.to_file())
    }

    /// The active system, or a do-nothing placeholder while none is saved (`is_empty`).
    pub fn current(&self) -> &System {
        self.get(&self.active_id)
            .or_else(|| self.list.first())
            .unwrap_or_else(|| placeholder())
    }
    pub fn active_id(&self) -> &str {
        &self.active_id
    }
    pub fn get(&self, id: &str) -> Option<&System> {
        self.list.iter().find(|s| s.id == id)
    }
    pub fn get_mut(&mut self, id: &str) -> Option<&mut System> {
        self.list.iter_mut().find(|s| s.id == id)
    }
    pub fn position(&self, id: &str) -> Option<usize> {
        self.list.iter().position(|s| s.id == id)
    }
    /// The first saved system that `pick` accepts: the active one if it does.
    pub fn find(&self, pick: impl Fn(&System) -> bool) -> Option<String> {
        if pick(self.current()) && !self.is_empty() {
            return Some(self.active_id.clone());
        }
        self.saved().find(|s| pick(s)).map(|s| s.id.clone())
    }
    /// Makes `id` active. Returns false when no such system exists.
    pub(crate) fn set_active(&mut self, id: &str) -> bool {
        if self.get(id).is_none() {
            return false;
        }
        self.active_id = id.into();
        self.sync();
        true
    }
    /// Called after editing or adding systems, so the active id stays valid.
    pub(crate) fn changed(&mut self) {
        self.sync();
    }
    pub fn any_prepared(&self) -> bool {
        self.list.iter().any(|s| s.prepared)
    }

    /// A free name based on `base` ("Lab", "Lab 2", …).
    pub fn suggest_name(&self, base: &str) -> String {
        let taken = |n: &str| self.list.iter().any(|s| s.name.trim() == n);
        if !taken(base) {
            return base.into();
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|c| !taken(c))
            .unwrap_or_default()
    }
    fn unique_id(&self, name: &str) -> String {
        let taken: Vec<String> = self.list.iter().map(|s| s.id.clone()).collect();
        file::unique(&model::slug(name), &taken)
    }
    /// Adds `sys` under a free id from its name (a blank or invalid name becomes a free
    /// version of its suggested name) and returns that id. It becomes active only when it is
    /// the first system.
    pub fn add(&mut self, mut sys: System) -> String {
        sys.name =
            clean_name(&sys.name).unwrap_or_else(|_| self.suggest_name(&sys.suggested_name()));
        sys.id = self.unique_id(&sys.name);
        let id = sys.id.clone();
        self.list.push(sys);
        self.sync();
        id
    }
    /// Adds starter `kind` named `name` (tests).
    #[cfg(test)]
    pub fn add_starter(&mut self, kind: usize, name: &str) -> String {
        self.add(model::starter(kind, "", name))
    }
    /// Renames `id`; its id stays the same.
    #[cfg(test)]
    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), String> {
        let name = clean_name(name)?;
        let sys = self.get_mut(id).ok_or("That system no longer exists.")?;
        sys.name = name;
        Ok(())
    }
    /// Copies `id` under a free name ("… copy") and returns the new id.
    pub fn duplicate(&mut self, id: &str) -> Option<String> {
        let mut copy = self.get(id)?.clone();
        copy.name = self.suggest_name(&format!("{} copy", copy.label()));
        if copy.name.chars().count() > MAX_NAME {
            copy.name = copy.name.chars().take(MAX_NAME).collect();
        }
        copy.id = self.unique_id(&copy.name);
        let at = self.position(id).map_or(self.list.len(), |i| i + 1);
        let new = copy.id.clone();
        self.list.insert(at, copy);
        Some(new)
    }
    /// Removes `id`; removing the active one activates the first remaining system, and
    /// removing the last leaves none (the wizard is offered).
    pub fn remove(&mut self, id: &str) -> Result<System, String> {
        let i = self.position(id).ok_or("That system no longer exists.")?;
        let removed = self.list.remove(i);
        if self.editor.as_ref().is_some_and(|d| d.id == id) {
            self.editor = None;
        }
        self.sync();
        Ok(removed)
    }

    /// Records that the active system starts `key` (a profile id, `session` or `recorder`).
    /// Call it only for work that is not running yet.
    pub(crate) fn claim(&mut self, key: &str) {
        self.owners.insert(key.to_string(), self.active_id.clone());
    }
    /// Records that system `owner` starts `key` (a run's temporary system starting your code).
    pub(crate) fn claim_for(&mut self, key: &str, owner: &str) {
        self.owners.insert(key.to_string(), owner.to_string());
    }
    /// The system that started `key`, if it is owned.
    pub fn owner(&self, key: &str) -> Option<&str> {
        self.owners.get(key).map(String::as_str)
    }
}

/// A trimmed name of 1..=60 characters.
pub(crate) fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give this system a name so you can return to it.".into());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("Keep the name to {MAX_NAME} characters or fewer."));
    }
    Ok(name.to_string())
}

fn live(s: &State, key: &str) -> bool {
    match key {
        "session" => s.session.is_some(),
        "recorder" => s.recorder.recording || s.recorder.playing,
        id => running(s, id),
    }
}

/// Whether the staged launch in progress lists profile `id`.
pub(crate) fn staged(s: &State, id: &str) -> bool {
    s.session.as_ref().is_some_and(|l| l.stages().contains(&id))
}

/// Brings the vehicle monitor in line with the active system after it changed (switched,
/// edited, or deleted): a physical source enables the monitor and fills its endpoint unless
/// the monitor is running; any other source turns the monitor's mode off. Never stops it.
pub(crate) fn follow_active(s: &mut State) {
    if let Some(endpoint) = s.systems.current().endpoint() {
        s.hardware.mode = 1;
        if !endpoint.is_empty() && !s.vehicle.connected() {
            s.hardware.endpoint = rune::TextEdit::new(endpoint);
        }
    } else if s.hardware.mode == 1 {
        s.hardware.mode = 0;
    }
    crate::recordings::follow_system(s);
    crate::studio::pipelines::follow_system(s);
}

/// Whether work started by system `id` still runs (processes, a staged launch, a capture).
pub(crate) fn owns_work(s: &State, id: &str) -> bool {
    s.systems
        .owners
        .iter()
        .any(|(key, owner)| owner == id && live(s, key))
}

/// Assigns running work nobody claimed to the active system and forgets finished work.
/// Called once per frame; never changes what is shown on its own.
pub(crate) fn poll(s: &mut State) -> bool {
    let active = s.systems.active_id.clone();
    let mut keys: Vec<String> = s
        .status
        .iter()
        .filter(|(_, v)| v.as_str() == "Running" || v.as_str() == "Starting")
        .map(|(k, _)| k.clone())
        .collect();
    if s.session.is_some() {
        keys.push("session".into());
    }
    if s.recorder.recording || s.recorder.playing {
        keys.push("recorder".into());
    }
    for key in keys {
        s.systems
            .owners
            .entry(key)
            .or_insert_with(|| active.clone());
    }
    if let (true, Some(owner)) = (s.vehicle.connected(), s.vehicle.owner.clone()) {
        s.systems.owners.insert("link".into(), owner);
    }
    let finished: Vec<String> = s
        .systems
        .owners
        .keys()
        .filter(|k| match k.as_str() {
            "session" | "recorder" => !live(s, k),
            // A stage the staged launch has not reached yet is still pending. Anything else
            // that is not running is finished, including stages of a launch that stopped
            // early (they never report a status), so a stale claim never outlives its launch.
            id => !running(s, id) && !staged(s, id),
        })
        .cloned()
        .collect();
    for key in finished {
        s.systems.owners.remove(&key);
    }
    false
}

/// Whether `sys` is a physical vehicle with QGroundControl or scripts enabled. The vehicle link
/// then bridges those programs to the vehicle both ways, so they can command it; Drone Deck
/// itself still only sends read requests.
pub(crate) fn physical_bridge(sys: &System) -> bool {
    sys.is_physical() && (sys.consumers.qgc || sys.consumers.scripts_udp.is_some())
}

/// Owned work running now, for any system, as short labels: owned processes, the app's
/// MAVLink router and vehicle link (a physical one too), recording or replay, and a pipeline
/// job. `Stop session` stops all of it.
pub(crate) fn work_labels(s: &State) -> Vec<String> {
    let mut parts = Vec::new();
    let processes = s
        .status
        .values()
        .filter(|v| v.as_str() == "Running" || v.as_str() == "Starting")
        .count();
    if processes > 0 {
        parts.push(format!(
            "{processes} owned process{}",
            if processes == 1 { "" } else { "es" }
        ));
    }
    if vehicle::router_running(s) {
        parts.push("MAVLink router".into());
    }
    // A headless scene's preview link counts, so its page reads like the live one.
    if s.vehicle.link.is_some() || (s.headless && s.vehicle.preview.is_some()) {
        let owner = s.vehicle.owner.as_deref().and_then(|id| s.systems.get(id));
        let link = match owner {
            Some(x) if physical_bridge(x) => "vehicle link (bridged)",
            Some(x) if x.is_physical() => "read-only vehicle link",
            _ => "vehicle link",
        };
        parts.push(link.into());
    }
    if s.recorder.recording {
        parts.push("recording".into());
    }
    if s.recorder.playing {
        parts.push("replay".into());
    }
    if s.studio.pipes.job.is_some() {
        parts.push("pipeline job".into());
    }
    parts
}

/// Whether any owned work runs for any system (`work_labels`), or a workspace launch is under
/// way. The System page then offers `Stop session` and the rail names what it would stop.
pub(crate) fn active_work(s: &State) -> bool {
    s.session.is_some() || !work_labels(s).is_empty()
}

/// Starts the active system's workspace: a staged profile launch, the vehicle monitor, or
/// (for a replay source) the project or recording list.
pub(crate) fn start_workspace(s: &mut State) {
    let sys = s.systems.current();
    let name = sys.label().to_string();
    match &sys.vehicle {
        // The monitor records its own start and refusal.
        Vehicle::Physical { .. } => hardware::toggle(s),
        Vehicle::Replay { .. } => {
            // The recording and project this system chose are the ones selected.
            crate::recordings::follow_system(s);
            crate::studio::pipelines::follow_system(s);
            let sys = s.systems.current();
            let inference = sys.consumers.inference.is_some();
            s.ui.data_tab = if inference { 3 } else { 0 };
            s.navigate(ui::Page::Data);
            s.notice = if inference {
                "Run a project from Data & models → Projects. No model is bundled.".into()
            } else {
                "Choose a recording to replay in Data & models → Recordings.".into()
            };
        }
        Vehicle::Simulated { world, .. } if sys.adapter().is_none() => {
            s.notice = format!(
                "{name} flies in the {} world, which this version of Drone Deck has no adapter for. Choose another world in Settings → Systems.",
                world.id
            );
        }
        Vehicle::Simulated { .. } => {
            let stages = session_stages(sys);
            if s.bundle(stages) {
                activity::record(s, format!("Requested {name} workspace startup"));
            }
        }
    }
}

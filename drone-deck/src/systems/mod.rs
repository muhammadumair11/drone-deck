//! Systems: user-created launch setups over the shared tool registry.
//!
//! A system names one vehicle source and the consumers it feeds (`model`). Its components,
//! connection chain and session stages derive from those (`components`); labels come from
//! observed state (`state`). `systems.json` is schema 2 (`file`). Processes belong to the
//! system that started them, so switching systems never stops anything.
use crate::*;
mod components;
mod edit;
mod file;
pub(crate) mod model;
mod state;
#[cfg(test)]
mod tests;

pub(crate) use components::{
    Backing, Component, SESSIONS, all_components, chain, component, components, filter_for,
    profile_ids, routed, session_stages, vehicle_profile,
};
pub(crate) use edit::Draft;
pub(crate) use model::{
    AI, KINDS, MAX_NAME, PHYSICAL, ROS, SIMULATION, SOURCES, Source, System, preset,
};
pub(crate) use state::{Tone, profile_state, running, state, tone_color};
use std::collections::HashMap;

/// How `State::select_system` names a system: by id, or by kind (the active system when it is
/// of that kind, else the first system of that kind).
pub(crate) enum Pick<'a> {
    Kind(usize),
    Id(&'a str),
}
impl From<usize> for Pick<'_> {
    fn from(kind: usize) -> Self {
        Pick::Kind(kind)
    }
}
impl<'a> From<&'a str> for Pick<'a> {
    fn from(id: &'a str) -> Self {
        Pick::Id(id)
    }
}
impl<'a> From<&'a String> for Pick<'a> {
    fn from(id: &'a String) -> Self {
        Pick::Id(id)
    }
}

/// Saved systems plus the runtime state that belongs to them.
pub(crate) struct Systems {
    pub list: Vec<System>,
    /// Kind (`SIMULATION`, `ROS`, `AI`, `PHYSICAL`) of the active system, kept in sync by
    /// every change. Read it; change the active system with `State::select_system`.
    pub active: usize,
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
impl Default for Systems {
    fn default() -> Self {
        Self::from_file(file::defaults())
    }
}
impl Systems {
    fn from_file(f: file::File) -> Self {
        let mut s = Self {
            list: f.systems,
            active: SIMULATION,
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
        file::File {
            schema: file::SCHEMA,
            active: self.active_id.clone(),
            systems: self.list.clone(),
        }
    }
    /// Keeps the list non-empty, the active id valid and `active` equal to its kind.
    fn sync(&mut self) {
        if self.list.is_empty() {
            self.list = model::defaults();
        }
        if !self.list.iter().any(|s| s.id == self.active_id) {
            self.active_id = self.list[0].id.clone();
        }
        self.active = self.current().kind();
    }
    fn path() -> std::path::PathBuf {
        backend::root().join("systems.json")
    }
    /// Reads `systems.json`; a schema 1 file is migrated and written back as schema 2.
    pub fn load(headless: bool) -> Self {
        if headless {
            return Self::default();
        }
        Self::load_at(&Self::path())
    }
    /// [`Systems::load`] from `path`. A file of a newer schema is left in place with a
    /// `.kept` copy, and the defaults run without replacing it.
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
                    "{e}. It was left unchanged{copy}; the default systems are in use and changes to them are not saved."
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

    /// The active system.
    pub fn current(&self) -> &System {
        self.get(&self.active_id).unwrap_or(&self.list[0])
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
    /// The id `pick` names, if such a system exists.
    pub fn resolve(&self, pick: Pick) -> Option<String> {
        match pick {
            Pick::Id(id) => self.get(id).map(|s| s.id.clone()),
            Pick::Kind(k) if self.current().kind() == k => Some(self.active_id.clone()),
            Pick::Kind(k) => self
                .list
                .iter()
                .find(|s| s.kind() == k)
                .map(|s| s.id.clone()),
        }
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
    /// Called after editing a system in place, so `active` follows its kind.
    pub(crate) fn changed(&mut self) {
        self.sync();
    }

    /// The active system's name when `kind` is the active kind, else the first system of that
    /// kind, else the template name. Prefer `current().label()`.
    pub fn name(&self, kind: usize) -> &str {
        if kind == self.active {
            return self.current().label();
        }
        self.list
            .iter()
            .find(|s| s.kind() == kind)
            .map(System::label)
            .unwrap_or(KINDS[kind.min(3)])
    }
    /// Like [`Self::name`], for the prepared flag.
    pub fn prepared(&self, kind: usize) -> bool {
        if kind == self.active {
            return self.current().prepared;
        }
        self.list.iter().any(|s| s.kind() == kind && s.prepared)
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
    /// Adds a system from template `kind` and returns its id. It does not become active.
    pub fn create(&mut self, kind: usize, name: &str) -> String {
        let name = clean_name(name).unwrap_or_else(|_| self.suggest_name(KINDS[kind.min(3)]));
        let id = self.unique_id(&name);
        self.list.push(model::template(kind, &id, &name));
        id
    }
    /// Renames `id`; its id stays the same.
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
    /// Removes `id`. The last system cannot be removed; removing the active one activates the
    /// first remaining system.
    pub fn remove(&mut self, id: &str) -> Result<System, String> {
        if self.list.len() <= 1 {
            return Err(
                "Keep at least one system. Create another before deleting this one.".into(),
            );
        }
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
    if let Source::Physical { endpoint } = &s.systems.current().source {
        s.hardware.mode = 1;
        if !endpoint.is_empty() && !s.vehicle.connected() {
            s.hardware.endpoint = rune::TextEdit::new(endpoint);
        }
    } else if s.hardware.mode == 1 {
        s.hardware.mode = 0;
    }
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
    matches!(sys.source, Source::Physical { .. })
        && (sys.consumers.qgc || sys.consumers.scripts_udp.is_some())
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
            Some(x) if matches!(x.source, Source::Physical { .. }) => "read-only vehicle link",
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
    match &sys.source {
        // The monitor records its own start and refusal.
        Source::Physical { .. } => hardware::toggle(s),
        Source::Replay { .. } => {
            let inference = sys.consumers.inference.is_some();
            s.ui.data_tab = if inference { 3 } else { 0 };
            s.navigate(ui::Page::Data);
            s.notice = if inference {
                "Run a project from Data & models → Projects. No model is bundled.".into()
            } else {
                "Choose a recording to replay in Data & models → Recordings.".into()
            };
        }
        _ => {
            let stages = session_stages(sys);
            if s.bundle(stages) {
                activity::record(s, format!("Requested {name} workspace startup"));
            }
        }
    }
}

//! A physical vehicle: the endpoint being edited, the `vehicle` link on it, and the state of
//! the physical tools (parameter comparison, exports, onboard logs). Drone Deck's side of the
//! link sends no GCS heartbeat and no flight command (`LinkOrigin::Physical`); nothing here
//! writes to the vehicle: parameters are read and compared, never set. QGroundControl or
//! scripts enabled for the system are bridged both ways and can (`systems::physical_bridge`).
use crate::mavlink::ParamSet;
use crate::*;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
mod preview;
#[cfg(test)]
mod tests;
mod view;
#[cfg(test)]
pub(crate) use preview::{fake_baseline, fake_params};
pub(crate) use preview::{preview_logs, preview_params};
pub(crate) use view::{monitor, size};

/// A `.params` file loaded to compare with the vehicle's parameters.
pub(crate) struct Baseline {
    pub path: PathBuf,
    pub set: ParamSet,
    /// Line numbers that did not parse.
    pub bad: Vec<usize>,
}

pub(crate) struct Hardware {
    /// 1 while the active system is a physical vehicle (`systems::follow_active`).
    pub mode: usize,
    pub endpoint: TextEdit,
    pub message: String,
    /// Parameter table search (name substring, any case).
    pub params_search: TextEdit,
    /// Show only parameters that differ from the loaded file.
    pub params_changed: bool,
    /// Path of the `.params` file to compare with.
    pub params_file: TextEdit,
    /// The loaded comparison of each system, by the system it was loaded for.
    pub baselines: HashMap<String, Baseline>,
    /// `.params` files being read on a worker for **Load and compare** (`poll` lands them),
    /// by the system each was started for.
    pub loads: HashMap<String, Loading>,
    /// The last `.params` export of this session, by the system whose parameters it holds.
    pub exports: HashMap<String, PathBuf>,
}
/// A `.params` file read off the UI thread: a path on an offline network share can block
/// until Windows gives up.
pub(crate) struct Loading {
    pub path: PathBuf,
    rx: Receiver<Result<Baseline, String>>,
}
impl Hardware {
    /// The last export made for system `id` this session.
    pub(crate) fn exported(&self, id: &str) -> Option<&PathBuf> {
        self.exports.get(id)
    }
    /// The comparison loaded for system `id`.
    pub(crate) fn baseline(&self, id: &str) -> Option<&Baseline> {
        self.baselines.get(id)
    }
    /// The **Load and compare** still reading for system `id`.
    pub(crate) fn loading(&self, id: &str) -> Option<&Loading> {
        self.loads.get(id)
    }
}
impl Default for Hardware {
    fn default() -> Self {
        Self {
            mode: 0,
            endpoint: TextEdit::new("udp:127.0.0.1:14555"),
            message: DEFAULT_MESSAGE.into(),
            params_search: TextEdit::new(""),
            params_changed: false,
            params_file: TextEdit::new(""),
            baselines: HashMap::new(),
            loads: HashMap::new(),
            exports: HashMap::new(),
        }
    }
}
pub(crate) const DEFAULT_MESSAGE: &str = "Enter a dedicated endpoint, then connect read-only. Drone Deck sends only read requests (the home position, and the parameter and log lists you ask for): no flight commands, parameter writes or heartbeat.";

/// Added wherever the panel says what reaches the vehicle, while the active system has
/// QGroundControl or scripts enabled (`systems::physical_bridge`).
const BRIDGE_CAVEAT: &str = "QGroundControl and scripts enabled for this system are bridged both ways and can command the vehicle.";
/// A `.params` file larger than this is not a parameter file.
const PARAMS_FILE_LIMIT: u64 = 8 * 1024 * 1024;

/// Whether the open link is this physical system's read-only link.
/// A headless scene's preview link (`vehicle::preview`) counts, so the page matches it.
pub(crate) fn connected(s: &State) -> bool {
    let active = s.systems.active_id();
    let preview = s.headless
        && s.vehicle.preview.is_some()
        && s.vehicle.owner.as_deref() == Some(active)
        && matches!(s.systems.current().source, systems::Source::Physical { .. });
    (s.vehicle.owned_by(active) || preview) && !s.vehicle.sim
}

/// Opens the read-only link on the endpoint for the active system.
pub(crate) fn start(s: &mut State) {
    if s.headless {
        s.hardware.message = "Preview: native connections disabled.".into();
        return;
    }
    if s.hardware.mode != 1 || s.setup.batch.is_some() || s.setup.jobs.iter().any(|j| j.alive) {
        s.hardware.message = "Finish active installations before connecting.".into();
        return;
    }
    let endpoint = s.hardware.endpoint.text.trim().to_string();
    if endpoint.len() > 160 || endpoint.contains(['\n', '\r']) {
        s.hardware.message = "Invalid endpoint.".into();
        return;
    }
    let owner = s.systems.active_id().to_string();
    s.hardware.message = match vehicle::connect_physical(s, &owner, &endpoint) {
        Ok(()) if systems::physical_bridge(s.systems.current()) => {
            format!("Connected on {endpoint}. Waiting for an autopilot heartbeat. {BRIDGE_CAVEAT}")
        }
        Ok(()) => format!("Connected read-only on {endpoint}. Waiting for an autopilot heartbeat."),
        Err(e) => e,
    };
}

/// Connects read-only, or closes the link (a connection stop, not a vehicle command).
pub(crate) fn toggle(s: &mut State) {
    if s.headless {
        s.hardware.message = "Preview: native connections disabled.".into();
        return;
    }
    if connected(s) {
        vehicle::disconnect(s);
        s.hardware.message =
            "Disconnected. This only closed the link; nothing was commanded.".into();
        s.notice = s.hardware.message.clone();
        return;
    }
    if s.setup.batch.is_some() || s.setup.jobs.iter().any(|j| j.alive) {
        s.hardware.message = "Finish or cancel the running installation before connecting.".into();
        return;
    }
    s.hardware.mode = 1;
    start(s);
}

/// The endpoint saved with the active physical system (`None` for other sources).
pub(crate) fn saved_endpoint(s: &State) -> Option<&str> {
    match &s.systems.current().source {
        systems::Source::Physical { endpoint } => Some(endpoint.trim()),
        _ => None,
    }
}

/// Saves the endpoint being edited as the active physical system's endpoint.
pub(crate) fn save_endpoint(s: &mut State) {
    let endpoint = s.hardware.endpoint.text.trim().to_string();
    if endpoint.is_empty()
        || endpoint.len() > 160
        || endpoint.contains(['\n', '\r'])
        || endpoint.parse::<crate::mavlink::Endpoint>().is_err()
    {
        s.hardware.message = format!(
            "{endpoint:?} is not an endpoint. Use serial:COM4:57600, udp:0.0.0.0:14550 or tcp:host:5760."
        );
        return;
    }
    let id = s.systems.active_id().to_string();
    let Some(sys) = s.systems.get_mut(&id) else {
        return;
    };
    let systems::Source::Physical { endpoint: saved } = &mut sys.source else {
        return;
    };
    *saved = endpoint.clone();
    let name = sys.label().to_string();
    s.hardware.message = match s.systems.save(s.headless) {
        Ok(()) => {
            activity::record(s, format!("Saved endpoint {endpoint} with {name}"));
            format!("Saved {endpoint} as the endpoint of {name}.")
        }
        Err(e) => format!("The endpoint could not be saved with the system: {e}"),
    };
}

fn say(s: &mut State, result: Result<(), String>, done: &str) {
    match result {
        Ok(()) => s.notice = done.into(),
        Err(e) => s.notice = e,
    }
}

/// Reads every parameter from the vehicle (PARAM_REQUEST_LIST; nothing is written).
pub(crate) fn fetch(s: &mut State) {
    let r = vehicle::fetch_params(s);
    if r.is_ok() {
        activity::record(s, "Requested the vehicle parameters (read-only)");
    }
    say(
        s,
        r,
        "Reading parameters. Nothing is written to the vehicle.",
    );
}

/// Writes the fetched parameters as a QGroundControl `.params` file under the data root.
pub(crate) fn export(s: &mut State) {
    if s.headless {
        s.notice = "Headless preview: exports are disabled.".into();
        return;
    }
    match vehicle::export_params(s) {
        Ok(path) => {
            s.notice = format!("Exported the parameters to {}.", path.display());
            activity::record(s, format!("Exported parameters to {}", path.display()));
            let system = s
                .vehicle
                .owner
                .clone()
                .unwrap_or_else(|| s.systems.active_id().to_string());
            s.hardware.exports.insert(system, path);
        }
        Err(e) => s.notice = e,
    }
}

/// `<data root>\exports\params`, where exports go.
pub(crate) fn export_dir() -> PathBuf {
    backend::root().join("exports").join("params")
}

/// Opens the exports folder, creating it first: Explorer opens Documents for a missing path.
pub(crate) fn open_exports(s: &mut State) {
    if s.headless {
        s.notice = "Headless preview: opening folders is disabled.".into();
        return;
    }
    let dir = export_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        s.notice = format!("Could not create {}: {e}", dir.display());
        return;
    }
    s.open(dir.display().to_string());
}

/// Starts reading the `.params` file named in `params_file` on a worker, to compare with the
/// active system's vehicle; [`poll`] lands it for that system. A request while one reads for
/// the active system does nothing. A headless scene reads it at once: its update loop returns
/// before [`poll`], so a worker's result would never land.
pub(crate) fn load_baseline(s: &mut State) {
    let system = s.systems.active_id().to_string();
    if s.hardware.loads.contains_key(&system) {
        return;
    }
    let text = s.hardware.params_file.text.trim().trim_matches('"').trim();
    if text.is_empty() {
        s.notice = "Enter the path of a .params file to compare with.".into();
        return;
    }
    let path = PathBuf::from(text);
    if s.headless {
        let result = read_baseline(&path);
        land_baseline(s, &system, &path, result);
        return;
    }
    let (tx, rx) = mpsc::channel();
    let file = path.clone();
    let spawned = std::thread::Builder::new()
        .name("params-load".into())
        .spawn(move || {
            let _ = tx.send(read_baseline(&file));
        });
    match spawned {
        Ok(_) => {
            s.notice = format!("Loading {}…", path.display());
            s.hardware.loads.insert(system, Loading { path, rx });
        }
        Err(e) => s.notice = format!("Could not read {}: {e}", path.display()),
    }
}

/// Lands each finished **Load and compare** for the system it was started for. True only
/// when a result landed.
pub(crate) fn poll(s: &mut State) -> bool {
    if s.hardware.loads.is_empty() {
        return false;
    }
    let mut done = Vec::new();
    for (system, loading) in &s.hardware.loads {
        match loading.rx.try_recv() {
            Ok(result) => done.push((system.clone(), result)),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => done.push((
                system.clone(),
                Err(format!(
                    "Reading {} stopped before it finished.",
                    loading.path.display()
                )),
            )),
        }
    }
    let landed = !done.is_empty();
    for (system, result) in done {
        if let Some(loading) = s.hardware.loads.remove(&system) {
            land_baseline(s, &system, &loading.path, result);
        }
    }
    landed
}

/// Waits for the active system's running load (tests drive the worker without the app's
/// poll loop).
#[cfg(test)]
pub(crate) fn finish_loading(s: &mut State) {
    let system = s.systems.active_id().to_string();
    let Some(loading) = s.hardware.loads.remove(&system) else {
        return;
    };
    let result = loading
        .rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| e.to_string())
        .and_then(|r| r);
    land_baseline(s, &system, &loading.path, result);
}

/// A load of `path` for the active system that stays pending until the returned sender
/// answers.
#[cfg(test)]
pub(crate) fn hold_loading(s: &mut State, path: &str) -> mpsc::Sender<Result<Baseline, String>> {
    let (tx, rx) = mpsc::channel();
    let system = s.systems.active_id().to_string();
    s.hardware.loads.insert(
        system,
        Loading {
            path: path.into(),
            rx,
        },
    );
    tx
}

/// Keeps a finished load as `system`'s comparison. A load for a system that is not active
/// says whose it is; one for a system deleted meanwhile is dropped with a notice.
fn land_baseline(s: &mut State, system: &str, path: &Path, result: Result<Baseline, String>) {
    let Some(name) = s.systems.get(system).map(|sys| sys.label().to_string()) else {
        s.notice = format!(
            "Dropped {}: the system it was loaded for no longer exists.",
            path.display()
        );
        return;
    };
    let active = system == s.systems.active_id();
    match result {
        Ok(b) => {
            let n = b.set.params.len();
            let whose = if active {
                String::new()
            } else {
                format!(" for {name}; it shows when {name} is the active system")
            };
            s.notice = if b.bad.is_empty() {
                format!("Loaded {n} parameters from {}{whose}.", path.display())
            } else {
                format!(
                    "Loaded {n} parameters from {}{whose}; {} line(s) did not parse.",
                    path.display(),
                    b.bad.len()
                )
            };
            activity::record(
                s,
                format!(
                    "Loaded {} to compare the parameters of {name}",
                    path.display()
                ),
            );
            if active {
                s.hardware.params_changed = s.vehicle.params.is_some();
            }
            s.hardware.baselines.insert(system.to_string(), b);
        }
        Err(e) if active => s.notice = e,
        Err(e) => s.notice = format!("{name}: {e}"),
    }
}

fn read_baseline(path: &Path) -> Result<Baseline, String> {
    let meta =
        std::fs::metadata(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file.", path.display()));
    }
    if meta.len() > PARAMS_FILE_LIMIT {
        return Err(format!(
            "{} is larger than 8 MiB; it is not a .params file.",
            path.display()
        ));
    }
    let (set, bad) = vehicle::load_params_file(path)?;
    if set.params.is_empty() {
        return Err(format!(
            "No parameters found in {} ({} line(s) did not parse).",
            path.display(),
            bad.len()
        ));
    }
    Ok(Baseline {
        path: path.to_path_buf(),
        set,
        bad,
    })
}

/// Forgets the comparison, a load still reading and the last export of system `id` when it is
/// deleted: a later system can take the same id, and it starts with none of them.
pub(crate) fn forget(s: &mut State, id: &str) {
    s.hardware.baselines.remove(id);
    s.hardware.loads.remove(id);
    s.hardware.exports.remove(id);
}

/// Clears the active system's comparison. Its load still reading is abandoned: the result is
/// dropped. Other systems keep theirs.
pub(crate) fn clear_baseline(s: &mut State) {
    let system = s.systems.active_id().to_string();
    if let Some(l) = s.hardware.loads.remove(&system) {
        s.notice = format!("Stopped waiting for {}.", l.path.display());
    }
    s.hardware.baselines.remove(&system);
    s.hardware.params_changed = false;
}

/// Lists the onboard logs (LOG_REQUEST_LIST).
pub(crate) fn list_logs(s: &mut State) {
    let r = vehicle::list_logs(s);
    say(s, r, "Asking the vehicle for its log list.");
}

/// Downloads onboard log `id` from the last list into `<data root>\logs\vehicle\`.
pub(crate) fn download(s: &mut State, id: u16) {
    let Some(entry) = s.vehicle.logs.iter().find(|e| e.id == id).cloned() else {
        s.notice = format!("Log {id} is no longer listed. List the logs again.");
        return;
    };
    let r = vehicle::download_log(s, entry);
    if r.is_ok() {
        activity::record(s, format!("Started downloading onboard log {id}"));
    }
    say(
        s,
        r,
        &format!("Downloading log {id}. A partial file resumes where it stopped."),
    );
}

pub(crate) fn cancel(s: &mut State) {
    let r = vehicle::cancel_log(s);
    say(
        s,
        r,
        "Download cancelled. The partial file stays and resumes next time.",
    );
}

/// Opens Data & models → Flight logs with `path` selected (after a rescan finds it).
pub(crate) fn open_in_flight_logs(s: &mut State, path: &Path) {
    s.ui.data_tab = crate::ulog::LOGS_TAB;
    s.navigate(ui::Page::Data);
    crate::ulog::select(s, path);
    if !s.headless {
        crate::ulog::refresh(s);
    }
}

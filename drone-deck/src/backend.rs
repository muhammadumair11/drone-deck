use crate::agent::Launcher;
use crate::store::Read;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    io::BufReader,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        OnceLock,
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread,
    time::Duration,
};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub probe: String,
    pub docs: String,
    /// Retired: old profiles may carry it; it is neither shown nor written.
    #[serde(default, skip_serializing)]
    #[allow(dead_code)]
    pub download_url: String,
    #[serde(default)]
    pub exclusive: bool,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Config {
    pub distro: String,
    pub profiles: Vec<Profile>,
}
/// The current `profiles.json` schema. A file without `"schema"` is schema 0.
pub const SCHEMA: u64 = 2;
#[derive(Serialize)]
struct Stored<'a> {
    schema: u64,
    #[serde(flatten)]
    config: &'a Config,
}
pub(crate) const DATA_DIR_ENV: &str = "DRONE_DECK_DATA_DIR";
fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}
/// Where the data root lives: `$DRONE_DECK_DATA_DIR`; `<exe dir>\workspace` when
/// `portable.txt` sits beside the exe; else `%LOCALAPPDATA%\DroneDeck\workspace`.
pub(crate) fn resolve_root(
    data_dir: Option<OsString>,
    exe_dir: &Path,
    local_app_data: Option<OsString>,
) -> PathBuf {
    let data_dir = data_dir
        .map(|d| {
            d.to_string_lossy()
                .trim()
                .trim_matches('"')
                .trim()
                .to_owned()
        })
        .filter(|d| !d.is_empty());
    if let Some(dir) = data_dir {
        // Helper processes run in other folders, so a relative override is made absolute.
        let dir = PathBuf::from(dir);
        return std::path::absolute(&dir).unwrap_or(dir);
    }
    if exe_dir.join("portable.txt").is_file() {
        return exe_dir.join("workspace");
    }
    match local_app_data.filter(|d| !d.is_empty()) {
        Some(base) => PathBuf::from(base).join("DroneDeck").join("workspace"),
        None => exe_dir.join("workspace"),
    }
}
/// The data root. Tests use `<exe dir>\workspace` and never the real user folders.
pub fn root() -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        if cfg!(test) {
            exe_dir().join("workspace")
        } else {
            resolve_root(
                std::env::var_os(DATA_DIR_ENV),
                &exe_dir(),
                std::env::var_os("LOCALAPPDATA"),
            )
        }
    })
    .clone()
}
fn same_place(a: &Path, b: &Path) -> bool {
    a == b || matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}
/// Copies `from` into `to` without replacing anything already there. Keeps going past an
/// entry that fails and adds it to `failed`, so one locked file cannot strand the rest.
fn copy_tree(from: &Path, to: &Path, skip: &[&str], failed: &mut Vec<PathBuf>) {
    let entries = match fs::create_dir_all(to).and_then(|_| fs::read_dir(from)) {
        Ok(entries) => entries,
        Err(_) => return failed.push(from.to_path_buf()),
    };
    for entry in entries {
        let Ok(entry) = entry else {
            failed.push(from.to_path_buf());
            continue;
        };
        let name = entry.file_name();
        if skip.iter().any(|s| name == *s) {
            continue;
        }
        let target = to.join(&name);
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => copy_tree(&entry.path(), &target, &[], failed),
            Ok(kind) if kind.is_file() && !target.exists() => {
                if copy_file(&entry.path(), &target).is_err() {
                    failed.push(entry.path());
                }
            }
            Ok(_) => {}
            Err(_) => failed.push(entry.path()),
        }
    }
}
/// Copies through `<name>.part`, so a target never holds half a file.
fn copy_file(from: &Path, to: &Path) -> std::io::Result<()> {
    let part = crate::store::sibling(to, ".part");
    let result = fs::copy(from, &part).and_then(|_| fs::rename(&part, to));
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}
const MIGRATED: &str = "MIGRATED-TO.txt";
/// Copies an exe-relative `workspace` folder into `root` once: only while `root` has no
/// `profiles.json`, `legacy` has one, and `legacy` has not already been copied to `root`.
/// `profiles.json` is copied last, so an interrupted copy is resumed on the next start.
/// Leaves `MIGRATED-TO.txt` in `legacy`. `Some` lists the entries that could not be copied.
pub(crate) fn migrate_legacy(legacy: &Path, root: &Path) -> std::io::Result<Option<Vec<PathBuf>>> {
    let copied_here = fs::read_to_string(legacy.join(MIGRATED))
        .is_ok_and(|marker| marker.lines().any(|l| Path::new(l.trim()) == root));
    if same_place(legacy, root)
        || root.starts_with(legacy)
        || copied_here
        || root.join("profiles.json").exists()
        || !legacy.join("profiles.json").is_file()
    {
        return Ok(None);
    }
    let mut failed = Vec::new();
    copy_tree(legacy, root, &[MIGRATED, "profiles.json"], &mut failed);
    copy_file(&legacy.join("profiles.json"), &root.join("profiles.json"))?;
    // The data is already in place, so a read-only legacy folder only loses the marker.
    let _ = crate::store::write(
        &legacy.join(MIGRATED),
        format!(
            "Drone Deck now keeps this workspace at:\r\n{}\r\n\r\nThis folder was copied there and is no longer used. \
             Put a portable.txt beside Drone Deck.exe to use an exe-relative workspace instead.\r\n",
            root.display()
        )
        .as_bytes(),
    );
    Ok(Some(failed))
}
/// Runs the one-time legacy migration and queues a notice describing it.
pub fn prepare_root() {
    let root = root();
    let legacy = exe_dir().join("workspace");
    match migrate_legacy(&legacy, &root) {
        Ok(Some(failed)) if failed.is_empty() => crate::store::note(format!(
            "Workspace data moved to {}. The old folder {} is unchanged.",
            root.display(),
            legacy.display()
        )),
        Ok(Some(failed)) => crate::store::note(format!(
            "Workspace data moved to {}, except {} item(s) that could not be copied (first: {}). \
             They are still in {}.",
            root.display(),
            failed.len(),
            failed[0].display(),
            legacy.display()
        )),
        Ok(None) => {}
        Err(e) => crate::store::note(format!(
            "The old workspace in {} could not be copied to {} ({e}); starting with what is there.",
            legacy.display(),
            root.display()
        )),
    }
}
/// Helper scripts: `<exe dir>\runtime`, or the source `scripts` folder under `cargo run`.
pub fn scripts() -> PathBuf {
    scripts_in(&exe_dir())
}
pub(crate) fn scripts_in(exe_dir: &Path) -> PathBuf {
    let runtime = exe_dir.join("runtime");
    if runtime.is_dir() {
        runtime
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts")
    }
}
pub fn config_path() -> PathBuf {
    root().join("profiles.json")
}
/// Removes what schema 0 shipped and no longer exists: the empty `ai` profile.
fn migrate_v0(value: &mut Value) {
    if let Some(profiles) = value.get_mut("profiles").and_then(Value::as_array_mut) {
        profiles.retain(|p| {
            !(p["id"] == "ai"
                && p["program"] == "wsl"
                && p["args"].as_array().is_none_or(|a| a.is_empty()))
        });
    }
}
/// Schema 1 → 2: drops the retired built-in profiles (relay, jMAVSim, MAVSDK telemetry, the
/// PowerShell monitor) and moves unedited Gazebo / ArduPilot commands to the system-driven
/// ones. An adopted MAVSDK interpreter (written as `telemetry`) becomes the `mavsdk` profile
/// that adopting it now writes. Profiles the user created are untouched. Returns the names
/// of dropped profiles and whether an interpreter was kept.
fn migrate_v1(value: &mut Value) -> (Vec<String>, bool) {
    let mut dropped = Vec::new();
    let Some(profiles) = value.get_mut("profiles").and_then(Value::as_array_mut) else {
        return (dropped, false);
    };
    let kept = !profiles.iter().any(|p| p["id"] == "mavsdk")
        && profiles.iter_mut().any(|p| {
            let exe = p["args"][0]
                .as_str()
                .filter(|_| p["id"] == "telemetry")
                .and_then(|a| a.strip_suffix(" -u \"$DRONE_DECK_SCRIPTS/telemetry.py\""))
                .filter(|exe| *exe != OLD_MAVSDK_PYTHON)
                .map(str::to_owned);
            let Some(exe) = exe else { return false };
            let (args, probe) = crate::setup::profiles::mavsdk_command(&exe);
            p["id"] = "mavsdk".into();
            p["name"] = "MAVSDK Python".into();
            p["description"] = "MAVSDK Python interpreter adopted before this version".into();
            p["args"] = serde_json::json!(args);
            p["probe"] = probe.into();
            true
        });
    profiles.retain(|p| {
        let retired = p["id"].as_str().is_some_and(|id| RETIRED.contains(&id));
        if retired {
            dropped.push(
                p["name"]
                    .as_str()
                    .or(p["id"].as_str())
                    .unwrap_or("")
                    .to_owned(),
            );
        }
        !retired
    });
    for p in profiles.iter_mut() {
        let new = match (p["id"].as_str(), p["args"][0].as_str()) {
            (Some("gazebo"), Some(OLD_GAZEBO)) => GAZEBO_ARGS,
            (Some("ardupilot"), Some(OLD_ARDUPILOT)) => ARDUPILOT_ARGS,
            _ => continue,
        };
        p["args"] = serde_json::json!([new]);
    }
    // New built-ins that system sessions start.
    for new in defaults()
        .profiles
        .into_iter()
        .filter(|p| ["rosgz", "foxglove"].contains(&p.id.as_str()))
    {
        if !profiles.iter().any(|p| p["id"] == new.id.as_str()) {
            profiles.extend(serde_json::to_value(new));
        }
    }
    (dropped, kept)
}
/// Parses `profiles.json`. The outer error marks a damaged file; the inner one a readable
/// file this version must not use (a newer schema, or no profiles). `Some` is the notice of
/// a migration (the file is then written back).
pub(crate) fn parse_config(
    bytes: &[u8],
) -> Result<Result<(Config, Option<String>), String>, String> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let schema = match value.get("schema") {
        None => 0,
        Some(v) => v.as_u64().ok_or("\"schema\" is not a number")?,
    };
    if schema > SCHEMA {
        return Ok(Err(format!(
            "profiles.json uses schema {schema}, newer than this version reads ({SCHEMA}); it was left unchanged"
        )));
    }
    if schema == 0 {
        migrate_v0(&mut value);
    }
    let (dropped, kept) = if schema < 2 {
        migrate_v1(&mut value)
    } else {
        (Vec::new(), false)
    };
    let config: Config = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if config.profiles.is_empty() || config.distro.is_empty() {
        return Ok(Err(
            "At least one profile and a WSL distribution are required".into(),
        ));
    }
    let notice = (schema < SCHEMA).then(|| {
        let mut text = format!(
            "Launch profiles upgraded to schema {SCHEMA}; the previous file is kept as profiles.json.bak."
        );
        if !dropped.is_empty() {
            text.push_str(&format!(
                " Removed retired profiles ({}): the native vehicle link replaces them.",
                dropped.join(", ")
            ));
        }
        if kept {
            text.push_str(" The adopted MAVSDK interpreter is kept as the MAVSDK Python profile.");
        }
        text
    });
    Ok(Ok((config, notice)))
}
pub fn load() -> Result<Config, String> {
    load_at(&config_path())
}
pub(crate) fn load_at(path: &Path) -> Result<Config, String> {
    let parsed = match crate::store::load(path, parse_config) {
        Read::Missing => {
            let c = defaults();
            save_at(path, &c)?;
            return Ok(c);
        }
        Read::Lost(notice) => {
            crate::store::note(notice);
            if path.exists() {
                return Err("profiles.json could not be read".into());
            }
            let c = defaults();
            save_at(path, &c)?;
            return Ok(c);
        }
        Read::Loaded(parsed) => parsed,
        Read::Recovered(parsed, notice) => {
            crate::store::note(notice);
            parsed
        }
    };
    let (config, migrated) = match parsed {
        Ok(parsed) => parsed,
        Err(e) => {
            // The app runs on defaults now, and a later save replaces this readable file,
            // so a copy is kept first (an earlier copy is never overwritten).
            let kept = crate::store::sibling(path, ".kept");
            if !kept.exists() && fs::copy(path, &kept).is_ok() {
                return Err(format!("{e}; a copy is kept as profiles.json.kept"));
            }
            return Err(e);
        }
    };
    if let Some(notice) = migrated {
        save_at(path, &config)?;
        crate::store::note(notice);
    }
    Ok(config)
}
pub fn save(c: &Config) -> Result<(), String> {
    save_at(&config_path(), c)
}
pub(crate) fn save_at(path: &Path, c: &Config) -> Result<(), String> {
    crate::store::write_json(
        path,
        &Stored {
            schema: SCHEMA,
            config: c,
        },
    )
}
const PX4_DIR: &str = "cd \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\"";
/// The PX4 + Gazebo command; world and model come from the system (`launch_env`).
const GAZEBO_ARGS: &str = "cd \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\" && test -z \"$(pgrep -x px4)\" && PX4_GZ_WORLD=\"${DRONE_DECK_GZ_WORLD:-default}\" make px4_sitl \"gz_${DRONE_DECK_GZ_MODEL:-x500_mono_cam}\"";
/// ArduPilot SITL serves MAVLink on TCP 5760 itself (`--no-mavproxy`).
const ARDUPILOT_ARGS: &str = "cd ~/ardupilot && Tools/autotest/sim_vehicle.py -v \"${DRONE_DECK_AP_VEHICLE:-ArduCopter}\" --no-mavproxy";
/// Camera image and camera_info of PX4's `x500_mono_cam` (sensor `camera` on `camera_link`,
/// instance `_0`) into ROS as `/camera/image_raw` and `/camera/camera_info`.
const ROSGZ_ARGS: &str = "source /opt/ros/jazzy/setup.bash && G=\"/world/${DRONE_DECK_GZ_WORLD:-default}/model/${DRONE_DECK_GZ_MODEL:-x500_mono_cam}_0/link/camera_link/sensor/camera\" && exec ros2 run ros_gz_bridge parameter_bridge \"$G/image@sensor_msgs/msg/Image[gz.msgs.Image\" \"$G/camera_info@sensor_msgs/msg/CameraInfo[gz.msgs.CameraInfo\" --ros-args -r \"$G/image:=/camera/image_raw\" -r \"$G/camera_info:=/camera/camera_info\"";
const FOXGLOVE_ARGS: &str = "source /opt/ros/jazzy/setup.bash && exec ros2 launch foxglove_bridge foxglove_bridge_launch.xml port:=8765";
/// Built-in profiles of earlier versions that the native vehicle link replaced. Stored
/// profiles with these ids are dropped when `profiles.json` is upgraded to schema 2.
pub(crate) const RETIRED: [&str; 4] = ["relay", "jmavsim", "telemetry", "hardware"];
/// Earlier built-in commands that schema 2 replaces when they were never edited.
/// The interpreter the built-in MAVSDK telemetry profile named before adoption.
const OLD_MAVSDK_PYTHON: &str = "\"$HOME/.local/share/drone-deck/tools/mavsdk/bin/python\"";
const OLD_GAZEBO: &str = "cd \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\" && test -z \"$(pgrep -x px4)\" && make px4_sitl gz_x500";
const OLD_ARDUPILOT: &str =
    "cd ~/ardupilot && Tools/autotest/sim_vehicle.py -v ArduCopter --no-mavproxy";

pub fn defaults() -> Config {
    let mut profiles = Vec::new();
    let mut add = |id: &str,
                   name: &str,
                   cat: &str,
                   desc: &str,
                   program: &str,
                   args: Vec<String>,
                   probe: &str,
                   docs: &str,
                   exclusive: bool| {
        profiles.push(Profile {
            id: id.into(),
            name: name.into(),
            category: cat.into(),
            description: desc.into(),
            program: program.into(),
            args,
            cwd: String::new(),
            probe: probe.into(),
            docs: docs.into(),
            download_url: String::new(),
            exclusive,
        })
    };
    add(
        "sih",
        "PX4 / SIH",
        "Simulation",
        "Lightweight internal flight physics · Quad X",
        "wsl",
        vec![format!(
            "{PX4_DIR} && test -z \"$(pgrep -x px4)\" && make px4_sitl_sih sihsim_quadx"
        )],
        "test -x \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot/build/px4_sitl_sih/bin/px4\"",
        "https://docs.px4.io/main/en/sim_sih/",
        true,
    );
    add(
        "gazebo",
        "Gazebo Harmonic",
        "Simulation",
        "PX4 with the system's Gazebo world and model",
        "wsl",
        vec![GAZEBO_ARGS.into()],
        "command -v gz && test -d \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\"",
        "https://docs.px4.io/main/en/sim_gazebo_gz/",
        true,
    );
    add(
        "qgc",
        "QGroundControl",
        "Flight operations",
        "Vehicle setup, missions and flight control",
        r"C:\Program Files\QGroundControl\bin\QGroundControl.exe",
        vec![],
        r"C:\Program Files\QGroundControl\bin\QGroundControl.exe",
        "https://docs.qgroundcontrol.com/master/en/qgc-user-guide/getting_started/download_and_install.html",
        false,
    );
    add(
        "ros2",
        "ROS 2 Jazzy",
        "Development",
        "ROS graph discovery · configure your node command",
        "wsl",
        vec!["source /opt/ros/jazzy/setup.bash && ros2 topic list".into()],
        "test -f /opt/ros/jazzy/setup.bash",
        "https://docs.ros.org/en/jazzy/Installation/Ubuntu-Install-Debs.html",
        false,
    );
    add(
        "dds",
        "Micro XRCE-DDS Agent",
        "Infrastructure",
        "PX4 ↔ ROS 2 transport · UDP 8888",
        "wsl",
        vec!["MicroXRCEAgent udp4 -p 8888".into()],
        "command -v MicroXRCEAgent",
        "https://docs.px4.io/main/en/middleware/uxrce_dds",
        false,
    );
    add(
        "rosgz",
        "ROS–Gazebo camera bridge",
        "Infrastructure",
        "x500_mono_cam image + camera_info → /camera/image_raw, /camera/camera_info",
        "wsl",
        vec![ROSGZ_ARGS.into()],
        "test -f /opt/ros/jazzy/share/ros_gz_bridge/package.xml",
        "https://docs.px4.io/main/en/sim_gazebo_gz/#ros-2-and-gazebo",
        false,
    );
    add(
        "foxglove",
        "Foxglove bridge",
        "Development",
        "Live ROS topics for Foxglove · WebSocket port 8765",
        "wsl",
        vec![FOXGLOVE_ARGS.into()],
        "test -f /opt/ros/jazzy/share/foxglove_bridge/package.xml",
        "https://docs.foxglove.dev/docs/connecting-to-data/ros-foxglove-bridge",
        false,
    );
    add(
        "ardupilot",
        "ArduPilot SITL",
        "Simulation",
        "Alternative flight stack · serves MAVLink on TCP 5760",
        "wsl",
        vec![ARDUPILOT_ARGS.into()],
        "test -x ~/ardupilot/Tools/autotest/sim_vehicle.py",
        "https://ardupilot.org/dev/docs/sitl-simulator-software-in-the-loop.html",
        true,
    );
    Config {
        distro: "Ubuntu-24.04".into(),
        profiles,
    }
}
/// `sim_vehicle.py -v` names for the ArduPilot vehicles a system can name.
fn ardupilot_vehicle(vehicle: &str) -> String {
    match vehicle.trim().to_ascii_lowercase().as_str() {
        "copter" | "arducopter" => "ArduCopter".into(),
        "plane" | "arduplane" => "ArduPlane".into(),
        "rover" => "Rover".into(),
        "sub" | "ardusub" => "ArduSub".into(),
        "heli" | "helicopter" => "Helicopter".into(),
        _ => vehicle.trim().into(),
    }
}
/// Environment for profile `id` started for `sys`: the Gazebo world and model, the ArduPilot
/// vehicle, and the ROS domain for ROS tools. Values reach the process through the agent's
/// environment, never through the command line.
pub(crate) fn launch_env(id: &str, sys: &crate::systems::System) -> Vec<(String, String)> {
    use crate::systems::Source;
    let mut env = Vec::new();
    match &sys.source {
        Source::Gazebo { world, model } if matches!(id, "gazebo" | "rosgz") => {
            env.push(("DRONE_DECK_GZ_WORLD".into(), world.clone()));
            env.push(("DRONE_DECK_GZ_MODEL".into(), model.clone()));
        }
        Source::ArduPilot { vehicle } if id == "ardupilot" => {
            env.push(("DRONE_DECK_AP_VEHICLE".into(), ardupilot_vehicle(vehicle)));
        }
        _ => {}
    }
    if matches!(id, "dds" | "ros2" | "rosgz" | "foxglove" | "ros-workspace") {
        env.push(("ROS_DOMAIN_ID".into(), sys.consumers.ros.domain.to_string()));
    }
    env
}
pub enum Request {
    Start(Profile, String),
    /// Like `Start`, with environment variables for the process (`launch_env`).
    StartEnv(Profile, String, Vec<(String, String)>),
    Stop(String),
    Detect(Config),
    Open(String),
    /// Follow a WSL process an earlier agent started: id, distribution, exclusive.
    Adopt(String, String, bool),
    Shutdown,
}
#[derive(Debug)]
pub enum Event {
    Log(String, String),
    Status(String, String),
    Detected(String, bool),
    Readiness(String, String),
}
pub struct Backend {
    pub tx: Sender<Request>,
    pub rx: Receiver<Event>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Backend {
    pub fn shutdown(mut self) {
        drop(self.rx);
        let _ = self.tx.send(Request::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
/// Windows `CREATE_NO_WINDOW`: a console child gets no console window.
const CREATE_NO_WINDOW: u32 = 0x08000000;
fn hidden(c: &mut Command) {
    #[cfg(windows)]
    {
        c.creation_flags(CREATE_NO_WINDOW);
    }
}
/// A Windows-native process the worker owns. WSL processes belong to the agent ([`Launcher`]).
struct Running {
    child: Child,
    exclusive: bool,
}
fn stop(id: &str, r: &mut Running, tx: &SyncSender<Event>) {
    #[cfg(windows)]
    {
        let mut c = Command::new("taskkill.exe");
        hidden(&mut c);
        let _ = c
            .args(["/PID", &r.child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = r.child.kill();
    let _ = r.child.wait();
    let _ = tx.send(Event::Status(id.into(), "Stopped".into()));
}
/// Sends one output line of process `id` as a log line, truncated to 3,000 characters.
pub(crate) fn forward_line(id: &str, line: String, tx: &SyncSender<Event>) {
    let line: String = line.chars().take(3000).collect();
    let _ = tx.send(Event::Log(id.into(), line));
}
/// Starts a Windows-native profile with its output streamed to `out`.
fn start_native(p: &Profile, out: &SyncSender<Event>) -> Option<Running> {
    let mut c = Command::new(&p.program);
    c.args(&p.args);
    if !p.cwd.is_empty() {
        c.current_dir(&p.cwd);
    }
    if crate::qgc_launch::configure(&mut c, p) {
        let _ = out.send(Event::Log(
            p.id.clone(),
            "GPU-safe launch: Qt Quick CPU renderer; graphics overrides apply only to QGroundControl.".into(),
        ));
    }
    c.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Started suspended and resumed only once it is in the job, so nothing it starts escapes.
    match crate::winjob::spawn(&mut c, CREATE_NO_WINDOW) {
        Ok((mut child, assigned)) => {
            if let Err(e) = assigned {
                let _ = out.send(Event::Log(
                    p.id.clone(),
                    format!("Not added to the job object ({e}); it may outlive Drone Deck."),
                ));
            }
            let a = child.stdout.take().unwrap();
            let b = child.stderr.take().unwrap();
            for reader in [Box::new(a) as Box<dyn std::io::Read + Send>, Box::new(b)] {
                let tx = out.clone();
                let id = p.id.clone();
                thread::spawn(move || {
                    for line in crate::process_output::BoundedLines::new(BufReader::new(reader))
                        .map_while(Result::ok)
                    {
                        forward_line(&id, line, &tx);
                    }
                });
            }
            let _ = out.send(Event::Status(p.id.clone(), "Running".into()));
            Some(Running {
                child,
                exclusive: p.exclusive,
            })
        }
        Err(e) => {
            let _ = out.send(Event::Status(p.id.clone(), "Failed".into()));
            let _ = out.send(Event::Log(p.id.clone(), e.to_string()));
            None
        }
    }
}
/// Starts `p` unless it runs, or refuses it while another exclusive flight stack runs.
fn start(
    p: &Profile,
    distro: &str,
    env: &[(String, String)],
    running: &mut HashMap<String, Running>,
    linux: &mut Launcher,
    out: &SyncSender<Event>,
) {
    if running.contains_key(&p.id) || linux.contains(&p.id) {
        return;
    }
    if p.exclusive && (running.values().any(|r| r.exclusive) || linux.exclusive_running()) {
        let _ = out.send(Event::Status(p.id.clone(), "Blocked".into()));
        let _ = out.send(Event::Log(
            p.id.clone(),
            "Another managed flight stack is running. Stop it first.".into(),
        ));
        return;
    }
    if p.program != "wsl" {
        if let Some(r) = start_native(p, out) {
            running.insert(p.id.clone(), r);
        }
    } else if p.args.is_empty() {
        let _ = out.send(Event::Status(p.id.clone(), "Configure command".into()));
    } else {
        linux.start_env(p, distro, env, out);
    }
}
fn detect(config: Config, tx: SyncSender<Event>) {
    let (linux, native): (Vec<_>, Vec<_>) = config
        .profiles
        .into_iter()
        .partition(|p| p.program == "wsl");
    for p in native {
        let found = !p.probe.is_empty() && Path::new(&p.probe).exists();
        let _ = tx.send(Event::Detected(p.id, found));
    }
    for (id, found) in crate::agent::probe(&config.distro, &linux) {
        let _ = tx.send(Event::Detected(id, found));
    }
}
pub fn spawn() -> Backend {
    let (tx, rx) = mpsc::channel();
    let (out, events) = mpsc::sync_channel(2048);
    let worker = thread::spawn(move || {
        let mut running: HashMap<String, Running> = HashMap::new();
        let mut linux = Launcher::default();
        loop {
            // WSL output arrives through agent events, so poll faster while any runs.
            let wait = Duration::from_millis(if linux.active() { 50 } else { 200 });
            match rx.recv_timeout(wait) {
                Ok(Request::Start(p, distro)) => {
                    start(&p, &distro, &[], &mut running, &mut linux, &out)
                }
                Ok(Request::StartEnv(p, distro, env)) => {
                    start(&p, &distro, &env, &mut running, &mut linux, &out)
                }
                Ok(Request::Adopt(id, distro, exclusive)) => {
                    if !running.contains_key(&id) {
                        linux.adopt(&id, &distro, exclusive, &out);
                    }
                }
                Ok(Request::Stop(id)) => {
                    if let Some(mut r) = running.remove(&id) {
                        stop(&id, &mut r, &out);
                    } else {
                        linux.stop(&id, &out);
                    }
                }
                Ok(Request::Detect(config)) => {
                    let tx = out.clone();
                    thread::spawn(move || detect(config, tx));
                }
                Ok(Request::Open(path)) => {
                    let mut c = Command::new("explorer.exe");
                    let _ = c.arg(path).spawn();
                }
                Ok(Request::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    for (id, mut r) in running.drain() {
                        stop(&id, &mut r, &out);
                    }
                    linux.shutdown(&out);
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            linux.poll(&out);
            let finished: Vec<_> = running
                .iter_mut()
                .filter_map(|(id, r)| r.child.try_wait().ok().flatten().map(|s| (id.clone(), s)))
                .collect();
            for (id, status) in finished {
                running.remove(&id);
                let _ = out.send(Event::Status(
                    id,
                    format!("Exited {}", status.code().unwrap_or(-1)),
                ));
            }
        }
    });
    Backend {
        tx,
        rx: events,
        worker: Some(worker),
    }
}

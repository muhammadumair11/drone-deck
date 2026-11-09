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
pub const SCHEMA: u64 = 1;
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
/// Parses `profiles.json`. The outer error marks a damaged file; the inner one a readable
/// file this version must not use (a newer schema, or no profiles). `true` means migrated.
pub(crate) fn parse_config(bytes: &[u8]) -> Result<Result<(Config, bool), String>, String> {
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
    let config: Config = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if config.profiles.is_empty() || config.distro.is_empty() {
        return Ok(Err(
            "At least one profile and a WSL distribution are required".into(),
        ));
    }
    Ok(Ok((config, schema < SCHEMA)))
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
    if migrated {
        save_at(path, &config)?;
        crate::store::note(format!(
            "Launch profiles upgraded to schema {SCHEMA}; the previous file is kept as profiles.json.bak."
        ));
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
pub fn defaults() -> Config {
    // Where the managed jMAVSim recipe installs (scripts/setup/native-visualizer.ps1).
    let local = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| "C:\\Users\\Public\\AppData\\Local".into());
    let jmavsim = format!("{local}\\DroneDeck\\tools\\jmavsim");
    let mut profiles = Vec::new();
    let mut add = |id: &str,
                   name: &str,
                   cat: &str,
                   desc: &str,
                   program: &str,
                   args: Vec<String>,
                   cwd: &str,
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
            cwd: cwd.into(),
            probe: probe.into(),
            docs: docs.into(),
            download_url: String::new(),
            exclusive,
        })
    };
    add("sih","PX4 / SIH","Simulation","Lightweight internal flight physics · Quad X","wsl",vec!["cd \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\" && test -z \"$(pgrep -x px4)\" && make px4_sitl_sih sihsim_quadx".into()],"","test -x \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot/build/px4_sitl_sih/bin/px4\"","https://docs.px4.io/main/en/sim_sih/",true);
    add(
        "relay",
        "WSL display relay",
        "Infrastructure",
        "SIH state → native Windows visualizer · UDP 19410",
        "wsl",
        vec!["python3 -u \"$DRONE_DECK_SCRIPTS/relay.py\"".into()],
        "",
        "command -v python3",
        "https://docs.px4.io/main/en/sim_sih/",
        false,
    );
    add(
        "jmavsim",
        "jMAVSim",
        "Simulation",
        "Native Java visualizer · display only",
        "java",
        vec![
            "--add-exports".into(),
            "java.base/java.lang=ALL-UNNAMED".into(),
            "--add-exports".into(),
            "java.desktop/sun.awt=ALL-UNNAMED".into(),
            "--add-exports".into(),
            "java.desktop/sun.java2d=ALL-UNNAMED".into(),
            "-XX:GCTimeRatio=20".into(),
            "-Djava.ext.dirs=".into(),
            "-jar".into(),
            "jmavsim_run.jar".into(),
            "-udp".into(),
            "19410".into(),
            "-disponly".into(),
        ],
        &jmavsim,
        &format!("{jmavsim}\\jmavsim_run.jar"),
        "https://docs.px4.io/main/en/sim_jmavsim/",
        false,
    );
    add("gazebo","Gazebo Harmonic","Simulation","PX4 X500 · configure a compatible installed world","wsl",vec!["cd \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\" && test -z \"$(pgrep -x px4)\" && make px4_sitl gz_x500".into()],"","command -v gz && test -d \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\"","https://gazebosim.org/docs/harmonic/",true);
    add(
        "qgc",
        "QGroundControl",
        "Flight operations",
        "Vehicle setup, missions and flight control",
        "C:\\Program Files\\QGroundControl\\bin\\QGroundControl.exe",
        vec![],
        "",
        "C:\\Program Files\\QGroundControl\\bin\\QGroundControl.exe",
        "https://docs.qgroundcontrol.com/master/en/qgc-user-guide/getting_started/download_and_install.html",
        false,
    );
    add(
        "telemetry",
        "MAVSDK telemetry",
        "Development",
        "Read-only live vehicle data · uses UDP 14540",
        "wsl",
        vec!["\"$HOME/.local/share/drone-deck/tools/mavsdk/bin/python\" -u \"$DRONE_DECK_SCRIPTS/telemetry.py\"".into()],
        "",
        "test -x \"$HOME/.local/share/drone-deck/tools/mavsdk/bin/python\"",
        "https://mavsdk.mavlink.io/main/en/python/",
        false,
    );
    add(
        "ros2",
        "ROS 2 Jazzy",
        "Development",
        "ROS graph discovery · configure your node command",
        "wsl",
        vec!["source /opt/ros/jazzy/setup.bash && ros2 topic list".into()],
        "",
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
        "",
        "command -v MicroXRCEAgent",
        "https://docs.px4.io/main/en/middleware/uxrce_dds",
        false,
    );
    add(
        "ardupilot",
        "ArduPilot SITL",
        "Simulation",
        "Alternative flight stack · configure checkout path",
        "wsl",
        vec!["cd ~/ardupilot && Tools/autotest/sim_vehicle.py -v ArduCopter --no-mavproxy".into()],
        "",
        "test -x ~/ardupilot/Tools/autotest/sim_vehicle.py",
        "https://ardupilot.org/dev/docs/sitl-simulator-software-in-the-loop.html",
        true,
    );
    Config {
        distro: "Ubuntu-24.04".into(),
        profiles,
    }
}
pub enum Request {
    Start(Profile, String),
    Stop(String),
    Input(String, String),
    Detect(Config),
    Open(String),
    Shutdown,
}
#[derive(Debug)]
pub enum Event {
    Log(String, String),
    Status(String, String),
    Detected(String, bool),
    Metric(String, String),
    Readiness(String, String),
    Hardware(serde_json::Value),
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
fn hidden(c: &mut Command) {
    #[cfg(windows)]
    {
        c.creation_flags(0x08000000);
    }
}
fn wsl(distro: &str) -> Command {
    let mut c = Command::new("wsl.exe");
    c.args(["-d", distro, "--exec"]);
    hidden(&mut c);
    c
}
fn linux_path(p: &std::path::Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    format!("/mnt/{}/{}", s[..1].to_lowercase(), &s[3..])
}
struct Running {
    child: Child,
    distro: String,
    wsl: bool,
    exclusive: bool,
}
fn stop(id: &str, r: &mut Running, tx: &SyncSender<Event>) {
    if id == "hardware" {
        use std::io::Write;
        if let Some(input) = r.child.stdin.as_mut() {
            let _ = writeln!(input, "{{\"action\":\"stop\"}}");
            let _ = input.flush();
        }
        for _ in 0..30 {
            if r.child.try_wait().ok().flatten().is_some() {
                let _ = tx.send(Event::Status(id.into(), "Stopped".into()));
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    if r.wsl {
        let _ = wsl(&r.distro)
            .args([
                "python3",
                &linux_path(&scripts().join("supervisor.py")),
                "stop",
                id,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
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
pub fn spawn() -> Backend {
    let (tx, rx) = mpsc::channel();
    let (out, events) = mpsc::sync_channel(2048);
    let worker = thread::spawn(move || {
        let mut running: HashMap<String, Running> = HashMap::new();
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(Request::Start(p, distro)) => {
                    if running.contains_key(&p.id) {
                        continue;
                    }
                    if p.exclusive && running.values().any(|r| r.exclusive) {
                        let _ = out.send(Event::Status(p.id.clone(), "Blocked".into()));
                        let _ = out.send(Event::Log(
                            p.id.clone(),
                            "Another managed flight stack is running. Stop it first.".into(),
                        ));
                        continue;
                    }
                    if p.program == "wsl" && p.args.is_empty() {
                        let _ = out.send(Event::Status(p.id, "Configure command".into()));
                        continue;
                    }
                    let mut c = if p.program == "wsl" {
                        let mut c = wsl(&distro);
                        c.args([
                            "python3",
                            &linux_path(&scripts().join("supervisor.py")),
                            "run",
                            &p.id,
                            &linux_path(&scripts()),
                            &p.args.join(" "),
                        ]);
                        c
                    } else if p.program == "java" {
                        let java = fs::read_dir("C:\\Program Files\\Microsoft")
                            .ok()
                            .and_then(|d| {
                                d.filter_map(Result::ok)
                                    .map(|e| e.path().join("bin/java.exe"))
                                    .find(|p| p.exists())
                            })
                            .unwrap_or_else(|| PathBuf::from("java.exe"));
                        let mut c = Command::new(java);
                        c.args(&p.args);
                        c
                    } else {
                        let mut c = Command::new(&p.program);
                        c.args(&p.args);
                        c
                    };
                    if !p.cwd.is_empty() && p.program != "wsl" {
                        c.current_dir(&p.cwd);
                    }
                    if crate::qgc_launch::configure(&mut c, &p) {
                        let _ = out.send(Event::Log(p.id.clone(),
                            "GPU-safe launch: Qt Quick CPU renderer; graphics overrides apply only to QGroundControl.".into()));
                    }
                    hidden(&mut c);
                    c.stdin(if p.id == "hardware" {
                        Stdio::piped()
                    } else {
                        Stdio::null()
                    })
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                    match c.spawn() {
                        Ok(mut child) => {
                            let a = child.stdout.take().unwrap();
                            let b = child.stderr.take().unwrap();
                            for reader in
                                [Box::new(a) as Box<dyn std::io::Read + Send>, Box::new(b)]
                            {
                                let tx = out.clone();
                                let id = p.id.clone();
                                thread::spawn(move || {
                                    for line in crate::process_output::BoundedLines::new(
                                        BufReader::new(reader),
                                    )
                                    .map_while(Result::ok)
                                    {
                                        let line: String = line.chars().take(3000).collect();
                                        if let Some(json) = line.strip_prefix("@hardware ") {
                                            if let Ok(value) = serde_json::from_str(json) {
                                                let _ = tx.send(Event::Hardware(value));
                                            }
                                        } else if let Some(json) = line.strip_prefix("@readiness ")
                                        {
                                            if let Ok(value) =
                                                serde_json::from_str::<serde_json::Value>(json)
                                            {
                                                let state =
                                                    value["state"].as_str().unwrap_or("Unknown");
                                                let _ = tx.send(Event::Readiness(
                                                    id.clone(),
                                                    state.into(),
                                                ));
                                                let _ = tx.send(Event::Log(
                                                    id.clone(),
                                                    value["detail"]
                                                        .as_str()
                                                        .unwrap_or(state)
                                                        .into(),
                                                ));
                                            }
                                        } else if let Some(json) = line.strip_prefix("@telemetry ")
                                        {
                                            if let Ok(values) =
                                                serde_json::from_str::<HashMap<String, String>>(
                                                    json,
                                                )
                                            {
                                                for (k, v) in values {
                                                    let _ = tx.send(Event::Metric(k, v));
                                                }
                                            }
                                        } else {
                                            let _ = tx.send(Event::Log(id.clone(), line));
                                        }
                                    }
                                });
                            }
                            let _ = out.send(Event::Status(p.id.clone(), "Running".into()));
                            running.insert(
                                p.id,
                                Running {
                                    child,
                                    distro,
                                    wsl: p.program == "wsl",
                                    exclusive: p.exclusive,
                                },
                            );
                        }
                        Err(e) => {
                            let _ = out.send(Event::Status(p.id.clone(), "Failed".into()));
                            let _ = out.send(Event::Log(p.id, e.to_string()));
                        }
                    }
                }
                Ok(Request::Stop(id)) => {
                    if let Some(mut r) = running.remove(&id) {
                        stop(&id, &mut r, &out);
                    }
                }
                Ok(Request::Input(id, line)) => {
                    use std::io::Write;
                    if let Some(r) = running.get_mut(&id) {
                        if let Some(input) = r.child.stdin.as_mut() {
                            let _ = writeln!(input, "{line}");
                            let _ = input.flush();
                        }
                    }
                }
                Ok(Request::Detect(config)) => {
                    let tx = out.clone();
                    thread::spawn(move || {
                        for p in config.profiles {
                            let found = if p.probe.is_empty() {
                                false
                            } else if p.program == "wsl" {
                                wsl(&config.distro)
                                    .args(["bash", "-lc", &p.probe])
                                    .stdout(Stdio::null())
                                    .stderr(Stdio::null())
                                    .status()
                                    .is_ok_and(|s| s.success())
                            } else {
                                std::path::Path::new(&p.probe).exists()
                            };
                            let _ = tx.send(Event::Detected(p.id, found));
                        }
                    });
                }
                Ok(Request::Open(path)) => {
                    let mut c = Command::new("explorer.exe");
                    let _ = c.arg(path).spawn();
                }
                Ok(Request::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    for (id, mut r) in running.drain() {
                        stop(&id, &mut r, &out);
                    }
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
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

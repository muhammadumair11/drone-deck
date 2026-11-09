use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::{
    collections::HashMap,
    fs,
    io::BufReader,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender, SyncSender},
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
    pub download_url: String,
    #[serde(default)]
    pub exclusive: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub distro: String,
    pub profiles: Vec<Profile>,
}
pub fn root() -> PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("workspace")
}
pub fn config_path() -> PathBuf {
    root().join("profiles.json")
}
pub fn load() -> Result<Config, String> {
    if config_path().exists() {
        let config: Config =
            serde_json::from_slice(&fs::read(config_path()).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if config.profiles.is_empty() || config.distro.is_empty() {
            return Err("At least one profile and a WSL distribution are required".into());
        }
        return Ok(config);
    }
    let c = defaults();
    save(&c)?;
    Ok(c)
}
pub fn save(c: &Config) -> Result<(), String> {
    fs::create_dir_all(root()).map_err(|e| e.to_string())?;
    fs::write(config_path(), serde_json::to_vec_pretty(c).unwrap()).map_err(|e| e.to_string())
}
pub fn defaults() -> Config {
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\Users\\Public".into());
    let launch = format!("{home}\\Desktop\\Simulator Launchers");
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
        &format!("{launch}\\jMAVSim-Windows"),
        &format!("{launch}\\jMAVSim-Windows\\jmavsim_run.jar"),
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
        vec!["-swrast".into()],
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
    add(
        "ai",
        "AI pipeline",
        "AI & data",
        "Configure your inference, training or dataset command",
        "wsl",
        vec![],
        "",
        "",
        "https://pytorch.org/get-started/locally/",
        false,
    );
    if let Some(p) = profiles.iter_mut().find(|p| p.id == "qgc") {
        p.download_url="https://github.com/mavlink/qgroundcontrol/releases/download/v5.1.5/QGroundControl-installer-AMD64.exe".into();
    }
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
    Download(String),
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
fn scripts() -> PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("runtime")
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
                Ok(Request::Download(url)) => {
                    let tx = out.clone();
                    thread::spawn(move || {
                        if !url.starts_with("https://") {
                            let _ = tx.send(Event::Log(
                                "setup".into(),
                                "Downloads require an HTTPS URL.".into(),
                            ));
                            return;
                        }
                        let dir = root().join("downloads");
                        let _ = fs::create_dir_all(&dir);
                        let name = url
                            .split('?')
                            .next()
                            .unwrap_or("")
                            .rsplit('/')
                            .next()
                            .filter(|name| {
                                !name.is_empty()
                                    && name
                                        .chars()
                                        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
                            })
                            .unwrap_or("tool-download.bin");
                        let file = dir.join(name);
                        let script = format!(
                            "$ErrorActionPreference='Stop'; Invoke-WebRequest -Uri '{}' -OutFile '{}'; Write-Output 'Download complete. Verify publisher before running.'",
                            url.replace('\'', "''"),
                            file.to_string_lossy().replace('\'', "''")
                        );
                        let mut c = Command::new("powershell.exe");
                        hidden(&mut c);
                        let result = c.args(["-NoProfile", "-Command", &script]).output();
                        let msg = match result {
                            Ok(r) if r.status.success() => {
                                format!("Downloaded to {}", file.display())
                            }
                            Ok(r) => String::from_utf8_lossy(&r.stderr).to_string(),
                            Err(e) => e.to_string(),
                        };
                        let _ = tx.send(Event::Log("setup".into(), msg));
                    });
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

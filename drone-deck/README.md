> Version 0.8 reorganises the interface around an active system: see [UI-0.8.md](UI-0.8.md).

> Version 0.7 adds guided workflow setup, Hugging Face/model downloads, datasets/evaluation, editable AI/ROS/MAVSDK projects and native physical monitoring. See [0.7 guide](STUDIO-0.7.md).

# Drone Deck

A native Windows autonomy workspace built with Anomaly Rune, starting from `examples/hello`'s AppSpec/state/view structure. Windows manages the UI; Ubuntu-24.04 runs the Linux flight tools.

## Run

Open `Drone Deck.exe` from the project directory after building/deploying. No terminal windows are opened for managed commands. jMAVSim and QGroundControl open their own GUI windows.

Open **Settings** from the toolbar, sidebar, or **Ctrl+,**. The bottom-right **− / percentage / +** controls resize the entire interface from 50–150%, with an 85% default and a saved preference. **Ctrl+− / Ctrl++ / Ctrl+0** zoom out, zoom in, and reset. See [Settings and zoom](SETTINGS-AND-ZOOM.md).

Open **Recordings** for rosbag2/MCAP dependency checks, topic discovery/selection, graceful recording, local session browsing, and replay on a separate ROS domain. See the [recording guide](RECORDINGS.md). ROS publishers or a PX4-to-ROS bridge must be running to supply sensor topics.

Open **Setup** for Windows/WSL prerequisites, live Linux distribution selection, existing-tool detection/adoption, and reviewed managed installation recipes with persistent jobs/logs. See the [setup guide](SETUP.md) for supported routes and validation limits. Setup jobs continue across app closure; simulator/recording processes retain their existing stop-on-close behavior.

1. Click **Detect tools**. The app checks configured paths and probes inside Ubuntu.
2. Click **Start SIH workspace**. This starts PX4 SIH, the display relay, and native jMAVSim.
3. Inspect logs for PX4 readiness. Start MAVSDK telemetry to see read-only altitude, position and flight mode; start QGroundControl for flight controls.
4. Stop owned processes with **Stop session**; normal app close also stops them. Externally started simulators remain outside this application's ownership. Close the old VBS-launched session before starting a new one here.

## Integrations

Profiles live beside the executable in `workspace/profiles.json`. Edit them inside **Integrations**. Profile IDs are unique letters/numbers/hyphens/underscores. `program: "wsl"` executes the joined `args` as a Bash command inside the configured distribution. Other programs use literal arguments and an optional Windows working directory.

The three setup paths are detection, official guided setup, and manual command/path configuration. `download_url` enables an explicit, confirmed HTTPS download to `workspace/downloads/`, preserving a safe package filename; packages are never executed automatically. QGroundControl includes an official Windows download URL. Package-manager setups such as ROS 2 use official instructions because they require version and repository decisions.

Add profiles for alternative simulators, ROS nodes, inference servers, dataset recorders, training jobs and evaluation commands. Each receives managed lifecycle and log capture. Set `exclusive: true` for an alternative flight-stack session; the default PX4, Gazebo and ArduPilot entries are mutually exclusive. External PX4 processes are detected by the PX4 launch command and block a second start.

## Working integrations

- PX4 SIH and native Windows jMAVSim, using the existing WSL UDP relay arrangement.
- Gazebo Harmonic X500 / `default_low` world in the existing PX4 checkout.
- QGroundControl, with software rendering.
- MAVSDK 4.x read-only telemetry through the existing Ubuntu Python venv. It occupies UDP 14540; stop another MAVSDK listener before starting it.
- ROS 2, Micro XRCE-DDS Agent and ArduPilot command profiles and installation probes. They require installation/configuration where missing.
- AI & data: verified model downloads, dataset versioning, CSV evaluation, and editable ONNX/CPU-training project starters. Optional runtimes require explicit installation; task-specific perception adapters remain user-defined.
- Native physical MAVLink monitoring over Windows serial/UDP/TCP, local tlogs and read-only parameter backups.

The UI does not embed Gazebo, jMAVSim or QGroundControl windows. It does not implement flight arming, mission upload, direct offboard control, ROS graph visualization or automatic universal simulator integration. Flight controls remain in QGroundControl or your project scripts. Telemetry becomes stale after five seconds without updates; never interpret a running process as a vehicle-ready assertion.

The existing firewall rule for UDP 19410 must allow the current WSL gateway. The relay discovers the gateway dynamically but does not modify firewall rules. Build and simulation performance still depend on the laptop GPU/CPU.

QGroundControl profiles containing `-swrast` or `--swrast` launch with Qt Quick's CPU renderer and software OpenGL. This avoids graphics-backend initialization failures on the current laptop. The overrides are local to the QGC child process; inherited RHI backend hints are cleared there. When its working directory is blank, the installed application's directory is used. Explicit working directories are preserved. Remove `-swrast` in the QGC profile to return to its normal graphics selection; CPU rendering can limit GPU-dependent 3D/video features.

## Build and headless verification

Rune source is the parent directory. Install Rust and Microsoft C++ Build Tools / Windows SDK, then:

```powershell
cargo build --manifest-path drone-deck/Cargo.toml
cargo test --manifest-path drone-deck/Cargo.toml
drone-deck/target/debug/drone-deck.exe --snap workspace.png --tree workspace.json
drone-deck/target/debug/drone-deck.exe --scene ai --snap ai.png
```

Deploy the executable alongside a `runtime` directory containing all bundled Python, PowerShell, C# and template files. Headless preview mode disables process launches, downloads and profile writes. Runtime tests click real Rune elements, navigate pages and validate profile editing. A separate process-manager test runs a harmless Windows echo command. See `verification/` for generated screenshots and trees once checks are run.

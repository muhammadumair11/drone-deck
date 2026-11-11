# Changelog

Drone Deck's user-visible changes. Older entries were reconstructed from the per-version guides and verification reports that this file replaces. Open problems are tracked in [docs/known-issues.md](docs/known-issues.md).

## 0.9.2 — release candidate

- Add light and dark themes, file browsing, installation notifications, and page shortcuts.
- Keep workspace launch failures visible and link runtime components to their output.
- Improve narrow-window layout, section separation, and selectable process output.
- Add a vehicle-link status banner and open Guided setup on first use.
- Share PX4 locations between SIH and Gazebo per WSL distribution, with conservative profile migration and preserved custom commands.

## 0.9.1 — release checks

- Standardize the project and repository name as Drone Deck, including the project README and path examples.
- Wait for each Windows headless snapshot process to exit and verify its output before checking success.
- Synchronize the Linux orphan-process test with child startup so it tests a surviving process group reliably.

## 0.9.0

This release replaces the relay, jMAVSim, MAVSDK telemetry and PowerShell monitor with Drone Deck's own MAVLink link, adds a live vehicle view, simulation-only flight actions, physical-vehicle tools, flight logs, pipelines and user-created systems, and moves data to a per-user folder. None of it has run against a real WSL distribution, PX4, Gazebo, ArduPilot, ROS 2 graph, QGroundControl session, Foxglove or flight controller yet, and the installer has never been built: it is tested with fake agents, a loopback fake vehicle and generated files. What is unverified is listed in [known issues](docs/known-issues.md).

### Vehicle link and simulation
- A native MAVLink link (`src/mavlink/`) replaces the display relay (`relay.py`), jMAVSim, MAVSDK telemetry (`telemetry.py`) and the PowerShell monitor (`hardware.ps1`, `mavlink.cs`). Simulation sessions reach PX4 through the Linux agent's MAVLink router (UDP 14550 ⇄ TCP 5760 in WSL) and the link on `tcp:127.0.0.1:5760`, over WSL localhost forwarding. No firewall rule, relay or Java runtime is needed.
- Workspace stages: PX4 SIH → router → link (→ DDS Agent) (→ QGroundControl); PX4 + Gazebo → router → link (→ DDS Agent → ROS–Gazebo camera bridge) (→ QGroundControl); ArduPilot SITL → link (→ QGroundControl), since it serves TCP 5760 itself.
- The link fans out to QGroundControl on `127.0.0.1:14550` (it should connect on its own; not yet observed) and to the system's scripts address (default `127.0.0.1:14540`), and writes a `.tlog` of every session to `logs\tlog\`. Captures are bounded: each stops at 2 GB, and the folder keeps the newest 100 and 2 GB.
- The router keeps the first UDP sender as its vehicle and ignores any other. Before a simulator or the router starts, Drone Deck checks, off the UI thread, that nothing on Windows already holds TCP 5760.
- One simulator at a time: a second, or the same one for another system, is refused with a notice naming the system that runs it. One vehicle link at a time, owned by one system.
- **Live vehicle** on the System page: attitude indicator, heading tape and a position trace since the link came up (home marked once the vehicle reports it; the link asks a newly found vehicle for its home once), with mode, armed state, altitude, speeds, battery, GPS, link rate and heartbeat age. Values grey out after five seconds without a heartbeat.
- **Preflight & health** (read-only): ready to arm only when the vehicle says so (PX4's health and arming check report on PX4 v1.14 and later, else the `SYS_STATUS` pre-arm bit or a pre-arm `STATUSTEXT`), `SYS_STATUS` sensors, GPS, battery, landed state and recent `STATUSTEXT` messages.
- **Flight actions** (Arm, Disarm, Takeoff, Land, Return, Hold and a mode picker; Arm and Takeoff ask first) and a **Vehicle console** (the PX4 MAVLink shell), for simulations Drone Deck started only. The Authority gate enables them only when the link is the app's own simulator link, the system's source is simulated and its PX4 or ArduPilot profile runs under that system; they also stay disabled while another UDP sender or a second autopilot shows up on the link. Otherwise they say why. Each command shows its acknowledgement, rejection or timeout.

### WSL agent and launch
- A WSL agent (`scripts/agent/agent.py`, one per distribution, JSON lines) replaces `supervisor.py` and the per-process helpers: it owns each Linux tool's process group, reports SIH readiness, runs the MAVLink router and serves recordings, projects and pipelines.
- Processes an earlier session left running are listed at the top of every page as **Running (previous session)**, with **Adopt** and **Stop**.
- Gazebo starts the system's world and model, ArduPilot the system's vehicle and ROS tools the system's domain, passed as environment variables, never on the command line. An adopted ArduPilot Copter build refuses to start for another vehicle type.
- New profiles: `rosgz` (the `x500_mono_cam` camera into `/camera/image_raw` and `/camera/camera_info`) and `foxglove` (`foxglove_bridge` on port 8765), with reviewed `ros-jazzy-ros-gz` and `ros-jazzy-foxglove-bridge` recipes.

### Physical vehicles
- Physical vehicles connect on the same link (`serial:COMx:baud`, `udp:host:port` listen, `tcp:host:port`). Drone Deck itself sends no heartbeat, flight command, parameter write or shell line there, only read requests: the home position, and the parameter and log transfers you start.
- With QGroundControl or scripts enabled for a physical system, the link is a two-way bridge: what they send, commands and parameter writes included, reaches the vehicle. The button then reads **Connect (bridged)**, Settings → Systems and the connection panel say so, and the panel counts **Sent by Drone Deck** separately from **Sent by QGroundControl/scripts**. Closing the app with a bridged link open writes a warning to Activity and `app.log`.
- A UDP-listen link stays with the source of the first autopilot heartbeat until it goes down.
- **Parameters**: fetch every parameter, search them, export a QGroundControl `.params` file to `exports\params\`, and compare with a `.params` file (*Changed*, *Only on vehicle*, *Only in file*). Nothing is written to the vehicle.
- **Onboard logs**: list the vehicle's logs and download one at a time to `logs\vehicle\`, with progress and cancel; a partial download resumes. Nothing is erased.

### Systems
- Systems are user-created. `systems.json` schema 2 stores each system's source (PX4 SIH, PX4 + Gazebo, ArduPilot SITL, a physical endpoint or a recording replay) and consumers (QGroundControl, live view, ROS agent and recorder with a domain, inference, Foxglove, a scripts address). A schema 1 file is migrated on read; a migrated ROS system keeps PX4 SIH, while a new one uses Gazebo with `x500_mono_cam`.
- **New system…** in the system list starts Guided setup from a template; **Settings → Systems** makes a system active, edits its name, source and consumers, duplicates and deletes it.
- Switching systems is now allowed while work runs: processes, launches and captures stay with the system that started them, and **Stop session** stops all owned work, including the link, the router and a running pipeline job.

### Data & models
- **Pipelines** replaces **Datasets & evaluation**: frame extraction from finalized recordings (PNG, NPZ or JSON lines, with `index.csv` and a manifest of hashes) and ONNX inference on an isolated replay (recorder, inference node and `ros2 bag play` as owned agent processes, with a `run.json` manifest), and a runs list. A running job counts as owned work.
- **Open in Foxglove** opens a recording or inference MCAP through `\\wsl.localhost` (or a copy in `exports\`); **Live in Foxglove** starts the `foxglove` bridge profile and asks Windows to open Foxglove on `ws://localhost:8765` once that port accepts a connection.
- **Flight logs** tab: PX4 ULog files from SITL, vehicle downloads and `.tlog` captures, summarized locally (duration, altitude, distance, battery, armed time, failsafes, logged messages, estimator flags) with plots, and **Open in PX4 Flight Review ↗** for detailed analysis (nothing is uploaded by Drone Deck). ULog files over 512 MB are left to Flight Review. `.tlog` summaries use the link's own MAVLink decoder: frames are CRC-checked and files are streamed.
- The MAVSDK project starter listens on `udpin://0.0.0.0:14540` inside WSL by default.

### Data folder and reliability
- Data moves out of the program folder to `%LOCALAPPDATA%\DroneDeck\workspace\` (profiles, systems, activity, preferences, setup jobs, vehicle captures, onboard logs, exports). Models stay in `%LOCALAPPDATA%\DroneDeck\models`.
- First start copies an existing `workspace\` beside the executable into the new location once, leaves `MIGRATED-TO.txt` in the old folder and reports it in the status bar and Activity.
- `DRONE_DECK_DATA_DIR` chooses another data folder; a `portable.txt` beside the executable keeps data in `workspace\` there (the old behaviour).
- Profiles, systems, activity, preferences and the setup queue are written atomically (temporary file, flush, rename) and the previous version is kept as `<name>.bak`. A file that fails to parse is set aside as `<stem>.corrupt-<YYYYMMDD-HHMMSS>.json` (for example `profiles.corrupt-20261109-142500.json`), the backup is tried, and the user is told; nothing is reset silently.
- `profiles.json` carries `"schema": 2`. Schema 0 files drop the empty `ai` profile; schema 1 files drop the retired `relay`, `jmavsim`, `telemetry` and `hardware` profiles once, with a notice, upgrade unedited Gazebo and ArduPilot commands and add `rosgz` and `foxglove`. User profiles are kept.
- Helper scripts are found in `runtime\` beside the executable, or in `drone-deck\scripts` under `cargo run`.

### Diagnostics and packaging
- `app.log` (rolling, about 3 MiB at most) and a crash report `crash-<stamp>.txt` for a panic, in `%LOCALAPPDATA%\DroneDeck\logs\` (`<data root>\logs` when the data root is overridden or portable). The next launch offers the newest report once at the top of Activity.
- **Activity → Export diagnostics** writes a redacted `exports\drone-deck-diagnostics-<stamp>.zip` (version, OS and WSL, process and job state, profiles, systems, activity, process output, logs and crash reports). Review it before sharing.
- Windows-native child processes (QGroundControl and other Windows profiles) and the `wsl.exe` agent start inside a Job Object with kill-on-close, so they end when Drone Deck exits, even after a crash. The setup and Data & models PowerShell helpers run in a second such job; the installation and model download workers they start keep running across a restart.
- `Build.ps1 -Installer` builds a per-user Inno Setup installer (`dist\DroneDeck-<version>-setup.exe`, unsigned; the uninstaller keeps user data), and a `v<version>` tag runs a CI release job that attaches the zip and the installer. Neither has been run yet. See [Releasing](docs/release.md).
- `Drone Deck.exe`, `runtime/`, `assets/` and `dist/` (build outputs) and `workspace/` (local data of the repository-root build) are no longer tracked; the files stay on disk. `Build.ps1` still deploys beside the repository root and also produces `dist\DroneDeck-<version>\` and a zip.
- Documentation is organised by task in `docs/` (user guide, troubleshooting, architecture, known issues, releasing) instead of one guide per version. The `verification/` screenshot and report dumps were removed; their open items are in known issues.

### Fixes
- **Ready to arm** on PX4 v1.14 and later reads PX4's health and arming check report (MAVLink events, received only). It says whether the vehicle can arm in the current mode, lists components with errors and warnings, and names the modes it can arm in. New **Arming source** and **Health report** rows. The report is cross-checked with the `SYS_STATUS` pre-arm bit (the stricter result wins) and marked as possibly out of date when events were missed (UI-05).
- While a workspace launch runs, the flight-actions bar and console say "Workspace launch under way: starting <stage> (stage i of n)…" instead of "No live link" (UI-01).
- On ArduPilot SITL the console input, **Send** and **End shell session** are disabled, since ArduPilot has no MAVLink shell; the vehicle link also refuses shell lines and the shell close there, so nothing is sent (UI-06).
- A simulation link that keeps retrying shows **Disconnect link** on the System page when no launch runs. It closes only the link (UI-07).
- Settings → Systems warns, without blocking the save, when a physical system sends scripts traffic to an address off this computer (UI-02).
- The Guided setup preset is renamed **Physical vehicle**; checkpoints saved under the old name still resume (UI-03).
- **Compare later with this file** offers only the active system's last `.params` export (UI-04).
- Settings → Systems refuses a scripts address that is not an IP address and port: a host name (`localhost` included) gets "Host names are not resolved", 0.0.0.0 or `[::]` is named a listen address, and port 0 is refused. The off-this-computer warning covers only real non-loopback IP addresses. Systems saved earlier keep their address until edited (UI-08).
- A loaded `.params` comparison and a running **Load and compare** belong to the system they were started for: switching systems no longer shows another system's comparison, a load that lands after a switch is kept for its own system, **Clear comparison** and **Stop waiting** act on the active system, and deleting a system forgets its comparison, load and export. A headless scene no longer stays on **Loading…** (UI-09).
- A router stage refused because TCP 5760 is taken shows that reason on the router station and in the flight-actions bar. The bar tells a link closed with **Disconnect link** ("the vehicle link was closed while … still runs") from one never opened ("… runs, but no vehicle link was opened for it") (UI-10).
- **Export diagnostics**, **Load and compare** and the TCP 5760 check no longer pause the window. The buttons read **Exporting…** and **Loading…**; **Stop waiting** drops a slow parameter file read. When TCP 5760 is taken, **Start workspace** now stops at its first stage with the reason instead of refusing at once; a direct simulator start waits for the check, then starts on its own, unless its system was deleted or the active system changed meanwhile, which drops it with a notice (PERF-01).
- The Pipelines job counter and the launch's seconds counter tick only on the page that shows them (PERF-02).
- **Live in Foxglove** waits until port 8765 accepts a connection and gives up after 20 s with a clear message instead of opening Foxglove after a fixed 1.5 s. Its message says only what was checked: the port accepts connections and Windows was asked to open Foxglove; any program on that port passes (PIPE-02 stays open). Inference runs record the real exit code of a process that exits at once, and a cancel no longer reports "processes stopped" too early (PIPE-01).
- The setup and Studio helpers, the helper that opens an `.mcap` and the `wsl.exe` agent end with Drone Deck, also after a crash or a kill from Task Manager. Diagnostics list both job objects (PROC-01).
- Closing Drone Deck while a setup start or a model download start or retry is still with its helper writes a warning naming it to Activity and `app.log`, and the next start shows it once as a notice. The helper is still ended with the app (PROC-02 stays open).

### Removed
- The display relay, jMAVSim with its recipe and Java checks, MAVSDK telemetry, the PowerShell MAVLink monitor and `supervisor.py`.
- The **Datasets & evaluation** tab (immutable dataset versions and CSV evaluation), replaced by Pipelines. Dataset folders already written in WSL are not deleted.
- PyTorch CPU training: the project starter, the `torch` recipe and profile, and the "CPU training and inference" preset. ONNX inference stays.
- The empty default `ai` profile. `ros2` stays.
- The legacy package download: the Tools **Download package…** action, its confirmation, the profile form's package URL field and QGroundControl's default download URL. Old profiles that still contain `download_url` load normally; the value is ignored and no longer written. Use **Review installation** for managed, verified installs.
- QGroundControl no longer starts in GPU-safe mode by default. Turn on **GPU-safe mode (software rendering)** in its Tools & config row (it adds `-swrast`); profiles saved with `-swrast` keep it.

## 0.8.0 — system workspace

- The interface follows the black, uppercase DIN design (Bahnschrift stands in for D-DIN), organised around one **active system**: Local simulation, ROS & sensor capture, Offline AI workspace, Physical vehicle. Switching is refused while work runs.
- New rail: 01 System, 02 Tools & config, 03 Data & models, 04 Activity, Guided setup, Settings.
- System shows the planned connection chain (solid only where traffic was observed), staged workspace start/stop with progress, QGroundControl and MAVSDK branches, live read-only telemetry, the native vehicle monitor and a runtime list.
- Tools & config replaces Simulators, Development and Integrations: filters, search, **All registered tools**, and rows that expand to command, guidance, profile form, detection, installation review, docs and folder. The integration inspector, metric cards and panel toggles were removed.
- Data & models gathers Recordings, Models, Datasets & evaluation and Projects.
- Activity: history of requested actions (`activity.json`, exportable), per-tool process output and installation jobs.
- Guided setup: Intention → The system → Preparation → First run. A system is marked prepared only when Preparation finishes with every tool reused or installed; First run asks for observed evidence.
- Settings: environment, display & zoom, Hugging Face token, profile registry. Default zoom is now 100 % (a saved 85 % is kept).

| 0.7 page | 0.8 location |
|---|---|
| Workspace | System |
| Simulators, Development, Integrations | Tools & config |
| Flight operations → Physical vehicle | System with Physical vehicle active |
| AI & data, Recordings | Data & models |
| Setup (workflows, checks, batch install) | Guided setup → Preparation |
| Setup (WSL, distribution, jobs) | Settings → Environment; Activity → Installations |
| Process console | Activity → Process output |

## 0.7.0 — setup queues, AI and physical monitoring

- Whole-workflow setup: review everything a workflow needs, reuse verified installations and install missing ones in a persistent sequential queue that pauses on failure, input or restart.
- New recipes: Gazebo Harmonic, pinned ArduPilot Copter SITL, PX4-matched jMAVSim build, PX4 v1.17 message workspace, ONNX CPU inference, PyTorch CPU training (removed in 0.9).
- Hugging Face token in Windows Credential Manager; model inspection at an immutable revision with size, license and gating; resumable, hash-verified download jobs into `%LOCALAPPDATA%\DroneDeck\models`.
- Immutable dataset versions over finalized recordings; CSV evaluation (accuracy and confusion matrix, MAE/RMSE, latency mean/p95) with provenance.
- Editable project starters (ONNX inference, ROS 2 subscriber, MAVSDK monitor, CPU training) in WSL.
- Native read-only MAVLink monitoring over serial, UDP and TCP with vehicle identity, `.tlog` capture and read-only parameter backups.

## 0.6.0 — repairs and simpler setup

- Fixed the discovery, retry, timeout, cancellation and lock defects from the 0.5.1 review (SET-01 to SET-10), WSL argument expansion (SIM-01, SIM-04) and the PX4 prompt flood (SIM-03).
- Staged SIH workspace launch: PX4 vehicle status, then a relay packet, then jMAVSim (RUN-01).
- Workflow checklists for lightweight simulation, PX4/ROS and ArduPilot development; **Use this installation**; **Configure after success** in the installation review.
- Plain profile form (one Linux command, or one Windows argument per line) and an existing-tool location picker; JSON became the advanced route.
- Installations and running tools are coordinated; long steps report **Needs attention** instead of hanging silently.
- jMAVSim adoption verifies its JAR and Java runtime.

## 0.5.1 — QGroundControl GPU-safe launch

- Profiles containing `-swrast`/`--swrast` start QGroundControl with Qt Quick's software renderer and software OpenGL, set only for that process; a blank working directory uses the installation folder.

## 0.5.0 — Windows/WSL setup foundation

- Setup page: Windows and WSL readiness scan, installed and live-catalog distributions, WSL and distribution installation reviews, Linux user initialization.
- Existing-tool detection and adoption bound to their distribution.
- Reviewed managed recipes (ROS Jazzy + rosbag2/MCAP, MAVSDK 4.1.0, PX4 v1.17.0 SIH, DDS Agent 2.4.3, QGroundControl 5.1.5) as persistent jobs with logs, cancellation at safe steps, retry and recovery; jobs continue after the app closes.
- Linux launches are gated on the configured distribution; native Windows tools stay available without WSL.

## 0.4.0 — recordings

- rosbag2/MCAP recording workspace: dependency check, topic discovery and selection, graceful finalization, session library with metadata, and replay on a separate local-only ROS domain with control topics filtered.

## 0.3.0 — settings and zoom

- Settings page (Ctrl+,) and whole-UI zoom from 50 % to 150 % with Ctrl+−, Ctrl++ and Ctrl+0, saved in `ui-preferences.json`.
- Embedded multi-resolution Windows icon.

## 0.2.0 — responsive native UI

- Ported the design's light theme; the window opens at the monitor's work area and can shrink to 360×320.
- Navigation becomes a drawer on narrow windows; content reflows; confirmations are centred dialogs.
- Tool filtering, command copying and project-folder opening use real state.

## 0.1.0 — first build

- Native Windows shell with a tool registry (launch profiles), managed processes with log capture and no terminal windows, the PX4 SIH → relay → jMAVSim workspace, QGroundControl, and a read-only MAVSDK telemetry adapter.

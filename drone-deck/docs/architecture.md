# Drone Deck architecture

For maintainers. What the app does for users is in the [user guide](user-guide.md).

Drone Deck is one Rust binary (`drone-deck`, edition 2024) built on Rune, the GPU UI framework in this repository (`crates/rune`, path dependency; pinned revision in [RUNE_REVISION.md](../RUNE_REVISION.md)). The UI and process control run on Windows. Linux work runs inside one WSL 2 distribution through helper scripts that ship beside the executable.

## Build, run, test

```powershell
cargo build --manifest-path drone-deck/Cargo.toml
cargo test  --manifest-path drone-deck/Cargo.toml
cargo run   --manifest-path drone-deck/Cargo.toml             # uses drone-deck/scripts as the runtime folder
drone-deck/target/debug/drone-deck.exe --scene <name> --snap shot.png --size 1440x900 --tree tree.json
drone-deck/Build.ps1                                          # release build + deploy, see below
```

- Preview flags (`--snap`, `--tree`, `--script`, `--list-scenes`, `--frames`, `--trace`) put the app in **headless** mode: defaults instead of the saved registry, no backend, no process launches, downloads, installs or profile writes. Scenes are the `SCENES` table in `src/main.rs`; scripts (`drone-deck/scripts/*.json`) click real elements by key.
- Tests are Rune runtime tests that render the real view tree and click elements by key (`src/tests.rs` and per-module `*tests.rs`), plus unit tests. Under `cfg(test)` the data root is `<test exe dir>\workspace`, never the user's folders.
- Helper tests: `scripts/agent/test_*.py`, `scripts/test_recordings.py`, `scripts/setup/test_*.py` and `scripts/studio/test_data.py` (run inside WSL, they use POSIX APIs); `scripts/setup/test-*.ps1` and `scripts/studio/test-*.ps1` (Windows PowerShell 5). None of them installs software.
- `Build.ps1` builds release, copies `Drone Deck.exe`, `runtime\` (from `scripts\`) and `assets\` beside the repository root for local use, and produces `dist\DroneDeck-<version>\` plus a zip. These outputs are not tracked in git. `-Installer` adds the per-user Inno Setup installer `dist\DroneDeck-<version>-setup.exe`; see [Releasing](release.md) for the installer, the CI release job, code signing (a follow-up) and diagnostics.
- `build.rs` embeds `assets/drone-deck.ico` with the Windows SDK resource compiler (`RC` overrides its path). Regenerate icons with `scripts/generate-icon.ps1`; see [ICON.md](../ICON.md).

Keep Rust files under 800 non-test lines and put tests in sibling `tests.rs` / `*_tests.rs` files.

## Module map (`src/`)

| Module | Responsibility |
|---|---|
| `main.rs` | `State` (all app state), `AppSpec`, the frame `poll` loop that drains backend events, startup notice, headless scenes. Methods `start`, `bundle`, `stop_all`, `select_system`, `apply`, `edit`, `send`, `navigate`, `log`. |
| `backend.rs` | `Profile`/`Config`, default profiles, data-root resolution (`root()`, `resolve_root`), legacy-workspace migration, `scripts()` (runtime folder), profile load/save with schema migration, and the process worker thread (`Request` in, `Event` out). |
| `store.rs` | Every durable JSON write and corrupt-file recovery (see [Data files](#data-files)). |
| `systems/` | User systems (`model.rs`: `System { id, name, template, prepared, source, consumers }`, the four templates), components, connection chains and session stages derived from source + consumers (`components.rs`), observed-state labels (`state.rs`), the Settings editor draft (`edit.rs`), `systems.json` schema 2 with schema 1 migration (`file.rs`), and which system owns running work (`mod.rs`). |
| `session.rs` | Staged workspace launch: start a stage, wait for its readiness (SIH `Ready`, other profiles `Running`, the virtual `router` / `link` stages through `vehicle::stage`), then the next; fail with a specific message on exit, refusal or timeout. |
| `vehicle/` | The live vehicle link (`Vehicle`, `State::vehicle`): one `mavlink::Service` at a time owned by one system; `connect_sim` (app loopback `tcp:127.0.0.1:5760`), `connect_physical` (read-only, no GCS heartbeat), fan-out peers for QGroundControl (`127.0.0.1:14550`) and `scripts_udp`, `.tlog` capture; the virtual launch stages and the agent router (`stage.rs`); the safety gate (`gate.rs`: `authority`, `gate_reason`, exclusive simulators); fake snapshots for scenes (`preview.rs`); the System-page live view and physical tools (`view/`). |
| `mavlink/` | MAVLink service thread: link I/O (TCP, UDP, serial), fan-out, `.tlog` capture and its CRC-checked, streamed summary (`summarize_tlog`, used by Flight logs), `VehicleSnapshot`, the one HOME_POSITION request at link-up, commands with ACK tracking behind `Authority`, parameters, onboard logs, the MAVLink shell. |
| `agent/` | The WSL agent client (`agent.py`, JSON lines): owned Linux processes (`Launcher`), probes, the MAVLink router, leftovers from an earlier session. |
| `activity.rs` | Activity history (`activity::record`), capped at 200 entries, export. |
| `profile_editor.rs` | The launch-profile form inside a Tools & config row; validation and the active-workload locks. |
| `qgc_launch.rs` | QGroundControl working directory and opt-in GPU-safe (`-swrast`) environment. |
| `process_output.rs` | `BoundedLines`: caps allocation while reading a line, before its newline arrives. |
| `setup/` | Host scan, recipe catalog, discovery, reviewed installation jobs (`bridge.rs` talks to PowerShell), `workflow.rs` presets and checks, `batch.rs` the persistent install queue, `profiles.rs` post-install profile configuration, `view.rs` installation review, environment and jobs panels. |
| `guided/` | Guided setup: `content.rs` (copy per system), `prep.rs` (Preparation), `run.rs` (First run evidence), `view.rs`. |
| `recordings/` | rosbag2/MCAP state and view; `bridge.rs` sends the agent's `rec.*` methods (the agent loads `recordings.py` through `services.py`). |
| `studio/` | Models, Hugging Face token, projects; `bridge.rs` calls `studio/control.ps1` (Windows) or the agent's `data.*` methods (`studio/data.py`, loaded by `services.py` in WSL). `pipelines.rs` + `view/pipelines.rs`: Data → Pipelines (extract frames, infer on replay, runs) over the agent's `pipe.*`; `foxglove.rs`: Open in Foxglove (UNC path, else a copy in `exports\`) and Live in Foxglove (`foxglove` profile). |
| `ulog/` | Data → Flight logs: the ULog parser (`parse.rs`), summaries (`summary.rs`) and reduced plot lines (`plots.rs`), log discovery in PX4 SITL folders, `logs/vehicle/`, `logs/tlog/` and `hardware/` (`sources.rs`), the background reader with a bounded cache (`worker.rs`), `.tlog` summaries through `mavlink::tlog`, and the PX4 Flight Review hand-off (upload page plus Explorer; nothing is uploaded). `fixture.rs` builds the synthetic logs that tests and headless scenes use. The view is `ui/pages/data/logs.rs`. |
| `hardware/` | The physical-vehicle endpoint and its read-only connect/disconnect (`mod.rs`) on the vehicle link; its System-page panel (`view.rs`). |
| `ui/` | Shell (`mod.rs`: rail, top bar with the active-system selector, status bar, confirm dialog, `Page`, `UiState`), `theme.rs` (tokens and type helpers), `kit.rs` (pills, ghosts, tabs, fields, rows, masts…), `settings.rs` (zoom and shortcuts), `pages/` (system, tools, data, activity, settings). |
| `window.rs` | Initial window size from the monitor work area; native minimum 360×320. |
| `winjob.rs` | Job Object with kill-on-close; every Windows-native tool profile (QGroundControl and other Windows profiles) joins it, so none outlives the app. The PowerShell helpers for setup and Data & models, the installation and model jobs they start detached (which survive a restart by design), and `wsl.exe` are not in the job. |
| `diagnostics/` | Panic hook and crash reports, rolling `app.log`, next-launch crash offer, Activity → Export diagnostics zip (`zip.rs` store-only writer, `redact.rs`; written on the UI thread, each file read up to its last 512 KiB). See [Releasing](release.md). |

UI rules: everything uses `ui::theme` and `ui::kit`; element keys are stable because tests and scripts use them; pages honour `Viewport::compact()` (≤ 720) and `medium()` (≤ 1100).

## Process model

```
UI thread (Rune)  ──Request──▶  backend worker thread  ──run──▶  WSL agent (agent.py, one per distro) ──▶ bash -lc <cmd>
       ▲                              │                    ──spawns──▶ QGroundControl.exe (Windows, Job Object)
       └──────────Event (bounded)─────┘◀── stdout/stderr reader threads (BoundedLines)
```

- `backend::spawn` starts one worker thread. `Request::{Start, StartEnv, Stop, Detect, Open, Adopt, Shutdown}` go in over an unbounded channel; `Event::{Log, Status, Detected, Readiness}` come back over a bounded channel (2048) that `State::poll` drains each frame. `State::start` passes the owning system's environment (`backend::launch_env`: `DRONE_DECK_GZ_WORLD`, `DRONE_DECK_GZ_MODEL`, `DRONE_DECK_AP_VEHICLE`, `ROS_DOMAIN_ID`) through the agent, never on the command line.
- **WSL tools** run through the agent (`run`): it creates and owns a process group per profile, keeps the child's stdin open (PX4's shell spins on EOF), streams output and reports SIH readiness. The app starts the agent with `wsl.exe -d <distro> --cd <runtime>\agent --exec python3 -u agent.py`, so wsl.exe maps the folder through the distro's own drive mount (a custom `[automount] root` should work; not verified on such a distro); the agent sets `DRONE_DECK_SCRIPTS` to the runtime folder as the distro sees it. A process started without a `cwd` runs in the agent's own folder (`runtime\agent` as the distro sees it); the shipped profiles `cd` themselves.
- **Windows tools** run directly with `CREATE_NO_WINDOW` inside the Job Object: they start suspended, join the job and only then resume (`NtResumeProcess`), so their own children are in the job too. Stop uses `taskkill /T /F` on the process tree.
- **Exclusive** profiles (alternative flight stacks) are refused with `Blocked` while another exclusive one runs, and the UI refuses a second simulator (or the same one for another system) with a notice naming the owner (`vehicle::simulator_conflict`).
- **Vehicle link** (no Linux relay, no firewall rule): PX4 in WSL sends MAVLink to UDP 14550 on localhost, where the agent's router (`router.start`: UDP bound to `127.0.0.1:14550`, TCP to `0.0.0.0:5760`) bridges it to TCP 5760. The first UDP sender stays the router's vehicle; datagrams from any other sender are counted as `foreign` and never forwarded, and the safety gate refuses commands while `foreign` is non-zero or the link carries more than one autopilot system id. WSL localhost forwarding lets the native `mavlink::Service` connect to `tcp:127.0.0.1:5760`. ArduPilot SITL serves TCP 5760 itself (no router stage). The service fans out to QGroundControl (`udpout 127.0.0.1:14550`, where it should auto-connect; not yet observed, ENV-02) and the system's `scripts_udp`, writes `<data root>\logs\tlog\<stamp>.tlog`, and feeds the native live view. Session stages: SIH `sih → router → link (→ dds) (→ qgc)`, Gazebo `gazebo → router → link (→ dds → rosgz) (→ qgc)`, ArduPilot `ardupilot → link (→ qgc)`. A physical system opens the same service on its endpoint, read-only and without a GCS heartbeat; a UDP-listen endpoint stays with the source address of the first autopilot heartbeat until the link closes. The 5760 check before a simulator starts (`stage::sim_port_taken`) binds and connects on the UI thread (up to about 150 ms on a free port, two or three times per session start). One link at a time; it belongs to the system that opened it.
- SIH readiness arrives as the agent's `readiness` event: `procs.py` runs `sih.py`'s monitor on a process started with `readiness: "sih"`. Every other output line is a log line, truncated to 3,000 characters for display.
- **On close** the app closes the vehicle link, stops the router it started, stops every owned process, finalizes recordings gracefully, and leaves installation workers running.
- **Installation jobs** are separate detached PowerShell workers (`setup/run-job.ps1`) started by `setup/control.ps1`. Each job folder holds `plan.json`, an atomically replaced `state.json`, `output.log` and markers. Worker identity is PID + start time. One worker at a time per Windows session (named mutex `Local\DroneDeck.SetupInstaller`). Cancellation is a request file checked at safe step boundaries; long package transactions are preserved and flagged with `output.log.waiting`.
- **Model downloads** are detached workers too (`studio/model-download.ps1`, jobs in `studio\jobs\`).

## Helper scripts (`scripts/` → `runtime\` beside the exe)

`scripts()` returns `<exe dir>\runtime`, or `drone-deck/scripts` from `CARGO_MANIFEST_DIR` when that folder is missing (so `cargo run` works).

| Script | Runs on | Purpose |
|---|---|---|
| `agent/` | WSL | The agent (`agent.py`): owned process groups, probes, SIH readiness (`sih.py`), the MAVLink UDP ⇄ TCP router (`router.py`), recorder and data adapters. |
| `recordings.py`, `install-ros-recording.py` | WSL | rosbag2/MCAP recorder, loaded by the agent (`agent/services.py`) and served as `rec.*` methods, and the ROS recording install recipe. |
| `setup/catalog.json` | — | Recipe catalog: source, version, target, privileges, packages, verification. |
| `setup/control.ps1`, `run-job.ps1`, `common.ps1` | Windows | Host scan, WSL catalog, discovery, job start/cancel/status, native command runner with timeouts and logging. |
| `setup/windows-tools.ps1`, `picker.ps1` | Windows | QGroundControl discovery, location picker. |
| `setup/linux-tools.py`, `extra_tools.py` | WSL | Linux discovery and recipes (PX4, MAVSDK, DDS, ROS; Gazebo, ros-gz, Foxglove bridge, ArduPilot, message workspace, ONNX). |
| `studio/control.ps1`, `common.ps1`, `model-download.ps1`, `hub.cs`, `credentials.cs` | Windows | Hugging Face API, model jobs, Windows Credential Manager. |
| `studio/data.py`, `studio/templates/` | WSL | Project library and scaffolds, loaded by the agent and served as `data.*` methods. |
| `agent/pipe.py`, `agent/infer_node.py` | WSL | `pipe.*`: MCAP frame extraction (worker process with the ROS environment), inference on an isolated replay (three owned processes), run listing, `pipe.status` (folders still being written, which a cancel waits for), export copy; the template ONNX inference node. |
| `*.json` (top level) | — | Headless UI scripts used for screenshots and checks. |

## Data files

Data root (`backend::root()`):

1. `DRONE_DECK_DATA_DIR` if set and non-empty;
2. `<exe dir>\workspace` if `portable.txt` sits beside the exe;
3. `%LOCALAPPDATA%\DroneDeck\workspace` otherwise.

On first start without a `profiles.json` in the data root, a legacy `<exe dir>\workspace\profiles.json` (not portable) triggers a one-time copy of that folder into the data root, `MIGRATED-TO.txt` in the legacy folder, a startup notice and an activity entry.

| File (under the data root) | Owner | Content |
|---|---|---|
| `profiles.json` | `backend` | `{ "schema": 2, "distro", "profiles": [...] }`. Missing `schema` is 0 (drops the retired empty `ai` profile); schema 1 drops the retired built-ins `relay`, `jmavsim`, `telemetry`, `hardware` with one notice, upgrades unedited Gazebo/ArduPilot commands and adds `rosgz` and `foxglove`; user profiles are untouched. `download_url` is still accepted on read and never written. |
| `systems.json` | `systems` | `{ "schema": 2, "active": "<id>", "systems": [...] }`. A schema 1 file (four `{ name, prepared }` records and an active index) is migrated on read and written back as schema 2 (the old file stays as `.bak`). |
| `activity.json` | `activity` | Up to 200 history entries; `activity-export.json` on export. |
| `ui-preferences.json` | `ui::settings` | Zoom. Not read or written in headless mode. |
| `session.log` | Activity | Process output, written on **Export log**. |
| `setup/jobs/<id>/` | `setup` | `plan.json`, `state.json`, `output.log`, `output.log.waiting`, `configuration-applied`. |
| `setup/workflow-state.json` | `setup::batch` | Guided/batch install checkpoint (version and environment checked on resume). |
| `studio/jobs/<id>/` | `studio` | Model download `state.json`, `progress.json`. |
| `logs/tlog/` | `vehicle` | `.tlog` capture of every vehicle link session (listed in Flight logs). |
| `logs/vehicle/` | `vehicle` | Onboard logs downloaded from the vehicle. |
| `exports/params/` | `vehicle` | Exported parameters (`<system>-<stamp>.params`, QGroundControl format). |
| `hardware/` | — | Captures of earlier versions (still listed in Flight logs). |

Outside the data root: `%LOCALAPPDATA%\DroneDeck\models` (models), `%LOCALAPPDATA%\DroneDeck\logs` (`app.log`, `crash-*.txt`; `<data root>\logs` when the data root is overridden or portable), Windows Credential Manager (Hugging Face token), and `~/.local/share/drone-deck/{tools,recordings,projects}` in WSL (pipeline outputs live in each recording's `extract/` and `inference/` folders).

**Durability** (`store.rs`): every JSON write goes to a temporary file in the same folder, is flushed and renamed over the target, and the previous file is kept as `<name>.bak`. A file that fails to parse at load (`profiles.json`, `systems.json`, `activity.json`, `ui-preferences.json`, `setup/workflow-state.json`) is renamed `<stem>.corrupt-<YYYYMMDD-HHMMSS>.<ext>`, the `.bak` is tried, and the user is told in the notice. Nothing is reset silently. PowerShell helpers write their own JSON with temp-file-and-move.

## Invariants worth keeping

- Running ≠ installed ≠ configured ≠ connected ≠ healthy. Labels come from observed state only; never invent metrics.
- Flight commands and the MAVLink console reach only an app-owned simulation: `vehicle::authority` grants `Authority::OwnedSimulation` only when the link is the app's own loopback simulator link (router started by this app, or ArduPilot SITL on 5760), the owning system's source is simulated, and its PX4/ArduPilot profile runs and is owned by that system. On a physical link Drone Deck sends no flight command, PARAM_SET, SERIAL_CONTROL or GCS heartbeat, whatever a caller passes; its own writes there are read requests: one `MAV_CMD_REQUEST_MESSAGE` for HOME_POSITION when a vehicle is first found (`mavlink/home.rs`; one `MAV_CMD_GET_HOME_POSITION` if refused, never SET_MESSAGE_INTERVAL) and the parameter and log transfers the user starts. Fan-out peers (QGroundControl, `scripts_udp`) are bridged both ways, so they can command a physical vehicle; the engine counts their frames in `LinkStatus::peer_to_vehicle_frames` and the connection panel shows them. Stop session stops processes and the link, not vehicles.
- Nothing installs without an explicit reviewed action; compatible existing installations are reused, never upgraded in place; package managers are never killed mid-transaction; cancellation is not rollback.
- Changing the distribution, editing or removing a running profile, and starting installations are blocked while dependent work runs (`State::busy`).
- Secrets never reach profiles, manifests, command lines, logs or WSL.

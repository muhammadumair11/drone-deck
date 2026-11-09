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
- Helper tests: `scripts/test_supervisor.py`, `scripts/test_recordings.py`, `scripts/setup/test_*.py` and `scripts/studio/test_data.py` (run inside WSL, they use POSIX APIs); `scripts/setup/test-*.ps1` and `scripts/studio/test-*.ps1` (Windows PowerShell 5). None of them installs software.
- `Build.ps1` builds release, copies `Drone Deck.exe`, `runtime\` (from `scripts\`) and `assets\` beside the repository root for local use, and produces `dist\DroneDeck-<version>\` plus a zip. These outputs are not tracked in git.
- `build.rs` embeds `assets/drone-deck.ico` with the Windows SDK resource compiler (`RC` overrides its path). Regenerate icons with `scripts/generate-icon.ps1`; see [ICON.md](../ICON.md).

Keep Rust files under 800 non-test lines and put tests in sibling `tests.rs` / `*_tests.rs` files.

## Module map (`src/`)

| Module | Responsibility |
|---|---|
| `main.rs` | `State` (all app state), `AppSpec`, the frame `poll` loop that drains backend events, startup notice, headless scenes. Methods `start`, `bundle`, `stop_all`, `select_system`, `apply`, `edit`, `send`, `navigate`, `log`. |
| `backend.rs` | `Profile`/`Config`, default profiles, data-root resolution (`root()`, `resolve_root`), legacy-workspace migration, `scripts()` (runtime folder), profile load/save with schema migration, and the process worker thread (`Request` in, `Event` out). |
| `store.rs` | Every durable JSON write and corrupt-file recovery (see [Data files](#data-files)). |
| `systems.rs` | The four systems, their components (each backed by a profile or a subsystem), connection chains, session stages, observed-state labels (`state`, `profile_state`, `Tone`) and `systems.json`. |
| `session.rs` | Staged workspace launch: start a profile, wait for its readiness (SIH `Ready`, relay `Forwarding`, others `Running`), then the next; fail with a specific message on exit or timeout. |
| `activity.rs` | Activity history (`activity::record`), capped at 200 entries, export. |
| `profile_editor.rs` | The launch-profile form inside a Tools & config row; validation and the active-workload locks. |
| `qgc_launch.rs` | QGroundControl working directory and opt-in GPU-safe (`-swrast`) environment. |
| `process_output.rs` | `BoundedLines`: caps allocation while reading a line, before its newline arrives. |
| `setup/` | Host scan, recipe catalog, discovery, reviewed installation jobs (`bridge.rs` talks to PowerShell), `workflow.rs` presets and checks, `batch.rs` the persistent install queue, `profiles.rs` post-install profile configuration, `view.rs` installation review, environment and jobs panels. |
| `guided/` | Guided setup: `content.rs` (copy per system), `prep.rs` (Preparation), `run.rs` (First run evidence), `view.rs`. |
| `recordings/` | rosbag2/MCAP state and view; `bridge.rs` keeps a JSON-lines worker (`recordings.py`) alive in WSL. |
| `studio/` | Models, Hugging Face token, datasets, evaluation, projects; `bridge.rs` calls `studio/control.ps1` (Windows) or `studio/data.py` (WSL). |
| `hardware/` | Native MAVLink monitor state (`mod.rs`) and its System-page view (`view.rs`). |
| `ui/` | Shell (`mod.rs`: rail, top bar with the active-system selector, status bar, confirm dialog, `Page`, `UiState`), `theme.rs` (tokens and type helpers), `kit.rs` (pills, ghosts, tabs, fields, rows, masts…), `settings.rs` (zoom and shortcuts), `pages/` (system, tools, data, activity, settings). |
| `window.rs` | Initial window size from the monitor work area; native minimum 360×320. |

UI rules: everything uses `ui::theme` and `ui::kit`; element keys are stable because tests and scripts use them; pages honour `Viewport::compact()` (≤ 720) and `medium()` (≤ 1100).

## Process model

```
UI thread (Rune)  ──Request──▶  backend worker thread  ──spawns──▶  wsl.exe -d <distro> --exec python3 runtime/supervisor.py start <id> <cmd>
       ▲                              │                              java.exe / QGroundControl.exe / hardware.ps1 (Windows)
       └──────────Event (bounded)─────┘◀── stdout/stderr reader threads (BoundedLines)
```

- `backend::spawn` starts one worker thread. `Request::{Start, Stop, Input, Detect, Open, Shutdown}` go in over an unbounded channel; `Event::{Log, Status, Detected, Metric, Readiness, Hardware}` come back over a bounded channel (2048) that `State::poll` drains each frame.
- **WSL tools** run as `wsl.exe -d <distro> --exec python3 <runtime>/supervisor.py …`. Explicit execution keeps `$VAR`, quotes and `$(…)` intact for the child Bash (an earlier `wsl --` launch expanded them too early). `DRONE_DECK_SCRIPTS` is the Linux path of the runtime folder (`/mnt/<drive>/…`). The supervisor creates and owns a process group, refuses a duplicate owner, keeps the child's stdin open (PX4's shell spins on EOF) and stops only groups it created.
- **Windows tools** run directly with `CREATE_NO_WINDOW`; stop uses `taskkill /T /F` on the process tree. The hardware monitor is asked to stop over stdin first.
- **Exclusive** profiles (alternative flight stacks) are refused with `Blocked` while another exclusive one runs.
- Helpers report structured state on stdout: `@readiness {json}` (SIH status, relay Forwarding), `@telemetry {json}` (MAVSDK values), JSON lines from the monitor. Everything else is a log line, truncated to 3,000 characters for display.
- **On close** the app stops every owned process, finalizes recordings gracefully, and leaves installation workers running.
- **Installation jobs** are separate detached PowerShell workers (`setup/run-job.ps1`) started by `setup/control.ps1`. Each job folder holds `plan.json`, an atomically replaced `state.json`, `output.log` and markers. Worker identity is PID + start time. One worker at a time per Windows session (named mutex `Local\DroneDeck.SetupInstaller`). Cancellation is a request file checked at safe step boundaries; long package transactions are preserved and flagged with `output.log.waiting`.
- **Model downloads** are detached workers too (`studio/model-download.ps1`, jobs in `studio\jobs\`).

## Helper scripts (`scripts/` → `runtime\` beside the exe)

`scripts()` returns `<exe dir>\runtime`, or `drone-deck/scripts` from `CARGO_MANIFEST_DIR` when that folder is missing (so `cargo run` works).

| Script | Runs on | Purpose |
|---|---|---|
| `supervisor.py` | WSL | Process-group ownership for every WSL profile. |
| `relay.py` | WSL | SIH state from `127.0.0.1:19410` to the Windows gateway on UDP 19410. |
| `readiness.py` | WSL | Read-only SIH initialization check, run by the supervisor for PX4 (never arms or changes parameters). |
| `telemetry.py`, `check_mavsdk.py` | WSL | Read-only MAVSDK adapter on UDP 14540 and its API check. |
| `recordings.py`, `install-ros-recording.py` | WSL | rosbag2/MCAP worker (JSON lines over stdin/stdout) and the ROS recording install recipe. |
| `setup/catalog.json` | — | Recipe catalog: source, version, target, privileges, packages, verification. |
| `setup/control.ps1`, `run-job.ps1`, `common.ps1` | Windows | Host scan, WSL catalog, discovery, job start/cancel/status, native command runner with timeouts and logging. |
| `setup/windows-tools.ps1`, `native-visualizer.ps1`, `picker.ps1` | Windows | QGroundControl and jMAVSim discovery/installation, location picker. |
| `setup/linux-tools.py`, `extra_tools.py` | WSL | Linux discovery and recipes (PX4, MAVSDK, DDS, ROS; Gazebo, ArduPilot, jMAVSim build, message workspace, ONNX). |
| `studio/control.ps1`, `common.ps1`, `model-download.ps1`, `hub.cs`, `credentials.cs` | Windows | Hugging Face API, model jobs, Windows Credential Manager. |
| `studio/data.py`, `studio/templates/` | WSL | Datasets, evaluation, project scaffolds. |
| `studio/hardware.ps1`, `mavlink.cs` | Windows | Native serial/UDP/TCP MAVLink monitor, CRC checks, tlogs, read-only parameter list. |
| `*.json` (top level) | — | Headless UI scripts used for screenshots and checks. |

## Data files

Data root (`backend::root()`):

1. `DRONE_DECK_DATA_DIR` if set and non-empty;
2. `<exe dir>\workspace` if `portable.txt` sits beside the exe;
3. `%LOCALAPPDATA%\DroneDeck\workspace` otherwise.

On first start without a `profiles.json` in the data root, a legacy `<exe dir>\workspace\profiles.json` (not portable) triggers a one-time copy of that folder into the data root, `MIGRATED-TO.txt` in the legacy folder, a startup notice and an activity entry.

| File (under the data root) | Owner | Content |
|---|---|---|
| `profiles.json` | `backend` | `{ "schema": 1, "distro", "profiles": [...] }`. Missing `schema` is 0 and is migrated (drops the retired empty `ai` profile). `download_url` is still accepted on read and never written. |
| `systems.json` | `systems` | Active system and per-system `{ name, prepared }`. |
| `activity.json` | `activity` | Up to 200 history entries; `activity-export.json` on export. |
| `ui-preferences.json` | `ui::settings` | Zoom. Not read or written in headless mode. |
| `session.log` | Activity | Process output, written on **Export log**. |
| `setup/jobs/<id>/` | `setup` | `plan.json`, `state.json`, `output.log`, `output.log.waiting`, `configuration-applied`. |
| `setup/workflow-state.json` | `setup::batch` | Guided/batch install checkpoint (version and environment checked on resume). |
| `studio/jobs/<id>/` | `studio` | Model download `state.json`, `progress.json`. |
| `hardware/` | `hardware` | `.tlog` captures and parameter backups. |

Outside the data root: `%LOCALAPPDATA%\DroneDeck\models` (models), `%LOCALAPPDATA%\DroneDeck\tools\jmavsim` (managed native jMAVSim), `%LOCALAPPDATA%\DroneDeck\logs` (reserved for app logs), Windows Credential Manager (Hugging Face token), and `~/.local/share/drone-deck/{tools,recordings,projects,datasets,evaluations}` in WSL.

**Durability** (`store.rs`): every JSON write goes to a temporary file in the same folder, is flushed and renamed over the target, and the previous file is kept as `<name>.bak`. A file that fails to parse at load (`profiles.json`, `systems.json`, `activity.json`, `ui-preferences.json`, `setup/workflow-state.json`) is renamed `<stem>.corrupt-<YYYYMMDD-HHMMSS>.<ext>`, the `.bak` is tried, and the user is told in the notice. Nothing is reset silently. PowerShell helpers write their own JSON with temp-file-and-move.

## Invariants worth keeping

- Running ≠ installed ≠ configured ≠ connected ≠ healthy. Labels come from observed state only; never invent metrics.
- Drone Deck sends no flight commands. Stop session stops processes, not vehicles. The hardware monitor is passive by default and only sends the read-only parameter list request on demand.
- Nothing installs without an explicit reviewed action; compatible existing installations are reused, never upgraded in place; package managers are never killed mid-transaction; cancellation is not rollback.
- Changing the distribution, editing or removing a running profile, and starting installations are blocked while dependent work runs (`State::busy`).
- Secrets never reach profiles, manifests, command lines, logs or WSL.

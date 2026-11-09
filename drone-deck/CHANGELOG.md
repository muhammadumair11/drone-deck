# Changelog

Drone Deck's user-visible changes. Older entries were reconstructed from the per-version guides and verification reports that this file replaces. Open problems are tracked in [docs/known-issues.md](docs/known-issues.md).

## Unreleased (phase 1)

### Data and reliability
- Data moves out of the program folder to `%LOCALAPPDATA%\DroneDeck\workspace\` (profiles, systems, activity, preferences, setup jobs, hardware captures, exported logs). Models stay in `%LOCALAPPDATA%\DroneDeck\models`; `%LOCALAPPDATA%\DroneDeck\logs\` is reserved for app logs.
- First start copies an existing `workspace\` beside the executable into the new location once, leaves `MIGRATED-TO.txt` in the old folder and reports it in the status bar and Activity.
- `DRONE_DECK_DATA_DIR` chooses another data folder; a `portable.txt` beside the executable keeps data in `workspace\` there (the old behaviour).
- Profiles, systems, activity, preferences and the setup queue are written atomically (temporary file, flush, rename) and the previous version is kept as `<name>.bak`. A file that fails to parse is set aside as `<stem>.corrupt-<YYYYMMDD-HHMMSS>.json` (for example `profiles.corrupt-20261109-142500.json`), the backup is tried, and the user is told; nothing is reset silently.
- `profiles.json` carries `"schema": 1`; older files load as schema 0 and are migrated.
- Helper scripts are found in `runtime\` beside the executable, or in `drone-deck\scripts` under `cargo run`.

### Removed
- PyTorch CPU training: the project starter, the `torch` recipe and profile, and the "CPU training and inference" preset. ONNX inference stays.
- The empty default `ai` profile (removed from existing registries by the schema migration). `ros2` stays.
- The legacy package download: the Tools **Download package…** action, its confirmation, the profile form's package URL field and QGroundControl's default download URL. Old profiles that still contain `download_url` load normally; the value is ignored and no longer written. Use **Review installation** for managed, verified installs.
- QGroundControl no longer starts in GPU-safe mode by default. Turn on **GPU-safe mode (software rendering)** in its Tools & config row (it adds `-swrast`); profiles saved with `-swrast` keep it.

### Build and repository
- `Drone Deck.exe`, `runtime/`, `assets/` and `dist/` (build outputs) and `workspace/` (local data of the repository-root build) are no longer tracked; the files stay on disk.
- `Build.ps1` still deploys beside the repository root and also produces `dist\DroneDeck-<version>\` and a zip.
- Documentation is organised by task in `docs/` (user guide, troubleshooting, architecture, known issues) instead of one guide per version. The `verification/` screenshot and report dumps were removed; their open items are in known issues.

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
- New recipes: Gazebo Harmonic, pinned ArduPilot Copter SITL, PX4-matched jMAVSim build, PX4 v1.17 message workspace, ONNX CPU inference, PyTorch CPU training (removed in phase 1).
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

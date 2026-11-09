# Drone Deck user guide

Drone Deck is a Windows desktop workspace for drone autonomy work. The interface runs on Windows; Linux flight tools (PX4, ROS 2, MAVSDK, the DDS Agent, AI runtimes) run inside a WSL 2 distribution, Ubuntu 24.04 by default. Drone Deck starts and stops those tools, keeps their output, prepares them for you and records what you asked it to do.

Drone Deck is not a ground station. Vehicle setup, missions, arming and flight control stay in QGroundControl (or Mission Planner). **Stop session** stops processes Drone Deck owns; it never lands, disarms or commands a vehicle.

Contents: [Systems](#systems) · [01 System](#01-system) · [02 Tools & config](#02-tools--config) · [03 Data & models](#03-data--models) · [04 Activity](#04-activity) · [Guided setup](#guided-setup) · [Settings](#settings) · [Where your data lives](#where-your-data-lives) · [Honest states](#honest-states)

Problems: see [troubleshooting](troubleshooting.md) and [known issues](known-issues.md).

## Systems

Everything is organised around one **active system**, chosen with the **Active system** selector at the top of each management page:

| System | What it brings together |
|---|---|
| Local simulation | PX4 SIH (autopilot + internal physics) → display relay → jMAVSim visualizer, with QGroundControl and read-only MAVSDK telemetry. No hardware needed. |
| ROS & sensor capture | PX4 SIH → Micro XRCE-DDS Agent → ROS 2 Jazzy → rosbag2/MCAP recordings, plus QGroundControl. |
| Offline AI workspace | ONNX CPU runtime → your inference project → run artifacts and evaluation reports. |
| Physical vehicle | Vehicle endpoint (serial/UDP/TCP) → native read-only MAVLink monitor → local `.tlog` captures, plus QGroundControl. |

Each system keeps its own name and whether Guided setup finished preparing it. Switching systems is refused while owned processes, recordings or installations are running. All systems share one tool registry (launch profiles) and one WSL distribution.

The rail on the left holds **01 System**, **02 Tools & config**, **03 Data & models**, **04 Activity** and **Guided setup ↗**. Its foot shows the environment, the number of owned processes, **Stop session** while anything runs, **Settings** and the version. Below 720 logical pixels the rail becomes a drawer opened with **Menu**; Escape or a click outside closes it.

## 01 System

The System page is the active system in one place.

- **Start workspace** starts the system's session in stages, each waiting for the previous one:
  - Local simulation: PX4 SIH, then the relay once SIH reports vehicle status (Ready), then jMAVSim once the relay reports Forwarding.
  - ROS & sensor capture: PX4 SIH, then the DDS Agent.
  - Offline AI workspace: the button reads **Open projects** and takes you to Data & models → Projects.
  - Physical vehicle: the button reads **Start monitoring** and starts the native monitor on the configured endpoint.

  While a session starts, the page shows which stage is waiting and for how long. A missing readiness signal, a process exit or a timeout stops the sequence with a specific message. While anything runs, the button becomes **Stop session** (confirmed).
- **First time here?** If the system is not prepared yet, **Prepare a system →** opens Guided setup. Once prepared, **Review setup ↗** reopens it. If WSL is not ready for a WSL-based system, this strip says so.
- **System connections** draws the planned chain for the system. A wire is dashed and labelled **Unverified** until there is live evidence for it, then solid and **Observed** (for example relay Forwarding between the relay and jMAVSim, a recorder that is recording between ROS 2 and MCAP, a fresh heartbeat between the endpoint and the monitor).
- **Branches** (simulation and ROS): start or stop QGroundControl; start or stop MAVSDK telemetry on UDP 14540 (simulation); **Open recordings ↗** (ROS).
- **Vehicle telemetry / read-only MAVSDK** appears while telemetry runs: link (Connected, Stale after five seconds without updates, Offline), altitude, flight mode, latitude, longitude. Values are shown only when MAVSDK reported them.
- **Runtime** lists every component of the system with its execution target, observed state and an action: Start/Stop for profiles and the monitor, or a link to Recordings, Projects, artifacts, logs or the endpoint. **Configuration & paths ↗** goes to Tools & config.

### Physical vehicle monitoring

With **Physical vehicle** active, the System page holds the native MAVLink monitor:

1. Enter an endpoint: `serial:COM4:115200`, `udp:127.0.0.1:14555` or `tcp:localhost:5760`, or click **Find serial devices** and pick a COM port. Use a dedicated endpoint; another app (often the ground station) may already own a serial port. Endpoint changes apply the next time monitoring starts.
2. **Start monitoring**. The monitor only listens: it sends no heartbeat, arm, mode, parameter-write or flight command.
3. A vehicle is identified only after an autopilot heartbeat arrives; ground stations and companion computers are ignored. Select a vehicle with a fresh heartbeat.
4. **Read parameters** sends the read-only parameter-list request once and saves a raw backup (wire values and types, with completeness). Nothing is written to the vehicle. Parameter requests on MAVLink-signed links are refused; signatures are not authenticated by this monitor.
5. Timestamped `.tlog` captures and parameter backups are kept under the data folder's `hardware\` subfolder. **Open capture folder ↗** opens it.

Parameter editing, onboard log download and flight commands stay in QGroundControl or Mission Planner.

## 02 Tools & config

Tools & config lists the tools behind the active system, or every registered tool with **All registered tools** (including Gazebo, ArduPilot and your own). Filter by category (Simulation, Transport, Telemetry, Ground station, Data, AI) or search by name, description or category.

- **Detect tools** checks each profile's installation probe (inside WSL for Linux tools, a file path for Windows tools). A probe proves presence, not compatibility or a working connection.
- Each row shows the tool, its execution target (WSL or Windows), its state, **Start/Stop** and **Configure ▾**.
- **Configure** expands the row in place: guidance, the exact command with **Copy command**, the working directory and the launch-profile form. The form is locked with "Stop this process before editing." while the tool runs. From the expanded row you can also **Detect** the installation, **Review installation** (the managed recipe, see below), open the **Official docs ↗** and **Open folder ↗**.
- **Add a tool** creates a new profile for a ROS node, inference server, recorder, alternative simulator or any other command.

### Launch profiles

A profile is one launchable tool. Fields:

| Field | Meaning |
|---|---|
| Display name | Shown throughout the app. |
| Program | `wsl` for a Linux command, `java` for native jMAVSim, or a full Windows executable path. |
| Command / arguments | For `wsl`: one Bash command, run with explicit execution inside the configured distribution (variables such as `$HOME` and `$DRONE_DECK_SCRIPTS` reach Bash intact). For Windows/Java: one argument per line, no JSON or surrounding quotes. |
| Windows working directory | Optional, Windows programs only. WSL commands `cd` themselves. |
| Installation probe | A Bash test for WSL tools, or a file path for Windows tools. |
| Ubuntu distribution | Shared by every WSL profile. |
| Category | Simulation, Development, Infrastructure, Flight operations, or AI & data. |

Profile IDs are unique letters, digits, hyphens and underscores. Profiles marked `exclusive` (PX4 SIH, Gazebo, ArduPilot) are alternative flight stacks: only one can run at a time, and an externally started PX4 also blocks a second start. Every managed process gets lifecycle control and log capture without opening a terminal window; jMAVSim, QGroundControl and Gazebo open their own windows.

QGroundControl's row also has **GPU-safe mode (software rendering)**, off by default. Turn it on if QGroundControl closes at start or opens a blank window; see [troubleshooting](troubleshooting.md#qgroundcontrol).

The whole registry can also be edited as JSON in **Settings → Profile registry** (advanced).

### Managed installation

**Review installation** opens the installation review for a tool's recipe: source and version, destination, packages/components, required privileges and how the result is verified. Nothing installs until you click **Install reviewed tool**.

- Compatible existing installations are offered for reuse instead of being upgraded.
- App-owned Linux tools go under `~/.local/share/drone-deck/tools` in the selected distribution; shared apt prerequisites may be installed through the distribution's root account. Source builds have no reliable size or duration estimate.
- **Apply the verified launch configuration after success** (on by default) points the tool's profile at the verified result. Completed configuration is recorded so reopening the app does not overwrite your later edits.
- Installation is blocked while tools, recordings or other installations run; tools that depend on an installation wait for it.

Maintained recipes: WSL, Linux distribution, ROS 2 Jazzy with rosbag2/MCAP, MAVSDK 4.1.0, PX4 v1.17.0 SIH (built with `DONT_RUN=1`, no Gazebo or firmware cross-compilers), Micro XRCE-DDS Agent 2.4.3 (for a DDS-v2 PX4 client), the PX4 v1.17 message workspace (`px4_msgs`, requires ROS Jazzy), Gazebo Harmonic (runtime only, not every PX4 world/bridge), pinned ArduPilot Copter SITL, the PX4-matched jMAVSim build (needs a native Java 21 runtime, declared in its review), ONNX CPU inference, and QGroundControl v5.1.5 AMD64. The QGroundControl recipe downloads the official installer, checks its published SHA-256 or Authenticode signature, and opens the visible Windows installer; if neither check is available the file is left unexecuted and the job reports **Needs input**.

Linux recipes target Ubuntu 24.04; other distributions are labelled Advanced. Clean-machine validation of the recipes is still outstanding (see [known issues](known-issues.md)).

Installation jobs are listed under **Activity → Installations**.

## 03 Data & models

Four tabs. Each has a status block, a library and honest empty states.

### Recordings (rosbag2 + MCAP)

Recordings run inside the selected WSL distribution with ROS 2 Jazzy (`/opt/ros/jazzy`).

1. **Check dependencies** verifies ROS 2 Jazzy, rosbag2 and the MCAP storage plugin.
2. Set the capture **ROS_DOMAIN_ID** to match your publishers (normally 0). Start your sensor nodes or the PX4 → ROS 2 bridge separately. PX4 SIH alone does not publish ROS sensor topics: PX4 sensor capture needs the DDS Agent, a matching firmware DDS client and matching `px4_msgs`.
3. **Discover topics** and select streams, or type absolute topic names separated by spaces or commas (for example `/camera/image_raw /imu`). Publishers that appear after capture starts are still recorded.
4. Name the session and add a source/vehicle note. Keep **system time** unless your simulation publishes `/clock`; with ROS clock, rosbag2 waits for the first clock message.
5. **Start MCAP recording**. Watch the process output for subscriptions and errors: a running recorder does not prove messages are arriving.
6. **Stop & finalize** sends SIGINT to the recorder, waits for buffers and metadata, and refreshes the library. Files split at 1 GiB with a 32 MiB cache.
7. Select a session for message counts, duration, size, topic types, clock mode and source note. **Open folder ↗** opens the WSL location in File Explorer.

Sessions are stored at `~/.local/share/drone-deck/recordings` inside the distribution (`session.json` plus a `bag` folder with `.mcap` files and `metadata.yaml`), so high-rate data never crosses the Windows mount. Nothing is deleted automatically. An interrupted recording is labelled for inspection; a finalized recording can contain zero messages if nothing was published, and cannot be replayed.

**Replay**: pick a finalized, non-empty session, a replay domain different from both the capture domain and the session's original domain (default **77**), and a speed from 0.1× to 4×. Replay uses local-only discovery and filters known PX4 control-input and common command topics; custom topic names cannot be classified automatically, so keep live-vehicle bridges out of the replay graph. Run your analysis nodes in the same distribution:

```bash
source /opt/ros/jazzy/setup.bash
export ROS_DOMAIN_ID=77
export ROS_LOCALHOST_ONLY=1
# start your analysis node; use use_sim_time:=true when it consumes playback /clock
```

**Stop replay** stops only the player. **Stop session** and closing the app also stop and finalize owned recordings; give the recorder time to flush. Changing the distribution during a recording or replay is blocked.

### Models (Hugging Face)

1. Enter `organization/model`, a revision and comma-separated file patterns. The repository format is checked before anything is sent.
2. **Inspect revision** resolves an immutable commit and reviews the selected files, total bytes, license and gating.
3. **Download reviewed files** starts a persistent job with progress, cancel and retry. Files are verified by SHA-256 (LFS) or Git blob hash; retry reuses verified files and resumes supported partial downloads. Repository code is never executed.
4. The local library is `%LOCALAPPDATA%\DroneDeck\models`. **Refresh** and **Verify** work offline. A project receives the model's WSL-mounted path; large files are not copied into WSL.

**Hugging Face access** (also in Settings): the token field is masked. Save, validate or remove a token for the current Windows user. It is stored in Windows Credential Manager and never in profiles, manifests, command lines, logs or WSL login files. Removing it does not revoke it at Hugging Face. Public repositories work anonymously; gated access and license acceptance happen on huggingface.co.

A downloaded model is not a ready runtime: the runtime and a task-specific adapter are separate components.

### Datasets & evaluation

- **Reload library**, pick a finalized recording session and **Create dataset version**. A version hashes the source metadata and artifacts and references the original MCAP files without copying them. Changing or deleting those files, or the manifest, makes **Verify dataset offline** fail.
- **Evaluation** takes a CSV with `sample_id,target,prediction` and optional `latency_ms` (unique IDs). Classification gives accuracy and a confusion matrix; regression gives MAE and RMSE; latency gives mean and p95. Reports keep the dataset version and the CSV checksum; a matching inference `run.json` also binds model/input hashes and runtime versions.
- Scores measure the labels and predictions you supply. Drone Deck does not invent labels or align frames to the recording.

Datasets and reports live under `~/.local/share/drone-deck/{datasets,evaluations}` in the distribution.

### Projects

Create an editable **ONNX inference**, **ROS 2 subscriber** or **MAVSDK monitor** project. Creation writes source, instructions and a manifest only, under `~/.local/share/drone-deck/projects` in the selected distribution (recorded in the manifest).

- Prepare the runtime first (**Prepare AI runtimes ↗** opens Guided setup for the Offline AI workspace).
- Choose a model file and inputs, then **Run**. Output goes to Activity → Process output; **Stop** and **Stop session** stop owned project processes. The project's saved distribution must match the selected one.
- The ONNX starter takes a model and preprocessed NPZ tensors named for its inputs, records CPU batch latency, model/input hashes and outputs, and writes `predictions.csv` when classification targets are given (usable in Evaluation). Its latency is the amortized time of one batch, not a per-frame real-time deadline. Camera/LiDAR decoding, calibration, timestamp alignment and preprocessing are the project's job.
- The MAVSDK starter listens on a different endpoint from the built-in telemetry so they do not compete for UDP 14540; route telemetry to it explicitly.

## 04 Activity

- **History**: every action you requested that changed something (starts and stops, profile saves, detection, recordings, installations, systems prepared), with time, system and kind. Kept in `activity.json` in the data folder. **Export history** writes `activity-export.json` and shows its path.
- **Process output**: the captured stdout/stderr of every managed tool, filterable per tool, errors in red. **Export log** writes `session.log` in the data folder; **Clear** empties the view. Lines are capped in length and in memory.
- **Installations**: setup jobs with tool, status, step/total and message; the selected job's log tail; **Cancel after current step**, **Review retry** and **Apply verified configuration** for a finished job.

Installation jobs keep running if Drone Deck closes and are restored when it reopens; only one installation worker runs at a time, even across app instances. Cancellation is checked at safe step boundaries: package managers and source builds finish their current step and are never killed mid-change. A step that runs past its time budget or goes quiet for five minutes shows **Needs attention**. Cancellation is not an uninstall or rollback. **Review retry** restores the original recipe, distribution and custom location and rechecks completed work. A job with no live worker after startup is shown as **Interrupted**.

Simulator, recorder and project processes are different: they stop when the app closes normally.

## Guided setup

**Guided setup ↗** prepares one system end to end in four chapters.

1. **Intention**: choose Watch a virtual drone (Local simulation), Capture sensor data (ROS), an Offline AI workspace or a Physical vehicle.
2. **The system**: the blueprint of stations and connections. **Inspect or adapt the technical recipe** lets you pick the execution target (an installed WSL distribution, or Windows local for the physical vehicle) and an existing tool path. Give the system a name (up to 60 characters).
3. **Preparation**: **Check this computer** scans Windows/WSL if needed, then detects each tool the system needs. Each row says **Found** (reuse after verification), **Not found** (review setup) or **Found / incompatible** with the reason. If WSL or a distribution is missing, those reviews come first. Read the plan (what is reused, installed and configured, with sources, locations and verification), tick **I reviewed the changes for this system.** and **Apply reviewed plan**. Missing tools install one at a time; a failed, input-required or restart-required job pauses the rest until you retry. The queue survives an app restart; a queue from another version or a changed environment pauses for review. On success the system is marked prepared and becomes active.
4. **First run**: three real tasks for the system, for example start the virtual vehicle, open the ground station and read telemetry. **Try this step** runs the real action; **Confirm observation →** is enabled once evidence exists (or after the action was requested, for things only you can see such as an external window). **Skip for now** is always available. The receipt lists the evidence collected.

Nothing installs from a check alone. **Start over ↗** resets the journey. Headless previews disable every action.

## Settings

Open Settings from the foot of the rail or with **Ctrl+,**. Settings apply to this computer and every system.

- **Environment**: computer readiness (Windows build, WSL, virtualization, memory and graphics; checking changes nothing) and the Linux environment. Pick an installed distribution and **Use this environment**, or review installing WSL or a distribution from Microsoft's live catalog (offline errors are shown, never a made-up list). Installing WSL may show a UAC prompt and need a restart; Drone Deck never restarts Windows. A new distribution's first launch asks for a Linux user: **Initialize Linux user ↗** opens it; tool installs need a regular user, not root. Changing the distribution does not move tools: detect them again for the new distribution. Existing WSL 1 distributions are never converted or removed.
- **Display** (interface zoom): 50–150 %, default 100 %. **Smaller −**/**Ctrl+−** and **Larger +**/**Ctrl++** (or **Ctrl+=**) change it by 5 %; **Reset to 100%**, a click on the percentage in the bottom-right controls, or **Ctrl+0** resets it. The bottom-right controls are on every page. Zoom scales and reflows the UI; it does not change Windows display scaling. The choice is saved in `ui-preferences.json`.
- **Model access**: the Hugging Face token (see [Models](#models-hugging-face)).
- **Profile registry** (advanced): **Edit JSON** for the whole registry, **Validate & save** or **Cancel**. Saving is refused if it would remove a running profile or change the distribution while anything is active. **Open folder ↗** opens the data folder.

## Where your data lives

| What | Where |
|---|---|
| Profiles, systems, activity, UI preferences, setup jobs, hardware captures, exported logs | `%LOCALAPPDATA%\DroneDeck\workspace\` |
| Downloaded models | `%LOCALAPPDATA%\DroneDeck\models\` |
| Hugging Face token | Windows Credential Manager |
| Managed Linux tools, recordings, projects, datasets, evaluations | `~/.local/share/drone-deck/` inside the selected WSL distribution |

To keep the data folder beside the executable instead (a USB stick or a self-contained folder), put an empty `portable.txt` next to `Drone Deck.exe`; the data folder is then `workspace\` beside it. Downloaded models, the managed jMAVSim copy and the token stay in their per-user locations above. To use another folder, set the environment variable `DRONE_DECK_DATA_DIR`. Earlier versions kept data beside the executable; the first start of this version copies it to the new location and leaves `MIGRATED-TO.txt` in the old folder. Details: [troubleshooting → data locations](troubleshooting.md#where-drone-deck-keeps-things).

## Honest states

Drone Deck keeps these separate everywhere: **installed** (a probe or verified recipe), **configured** (a profile points at it), **running** (the process is alive), **connected** (protocol traffic observed), **healthy** (fresh data). A running visualizer is not proof it receives state; a downloaded model is not a ready runtime; a probe is not a compatibility test; Stop session is not a vehicle command. Dashed connections are unverified.

Not provided: embedded simulator or ground-station windows, arming, mission upload, offboard or other flight commands, firmware flashing, a ROS graph browser, remote/onboard execution. See the [roadmap](../ROADMAP.md).

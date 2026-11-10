# Drone Deck user guide

Drone Deck is a Windows desktop workspace for drone autonomy work. The interface runs on Windows; Linux flight tools (PX4, ROS 2, MAVSDK, the DDS Agent, AI runtimes) run inside a WSL 2 distribution, Ubuntu 24.04 by default. Drone Deck starts and stops those tools, keeps their output, prepares them for you and records what you asked it to do.

Drone Deck is not a ground station. QGroundControl (or Mission Planner) remains the flight-control interface for real vehicles: vehicle setup, missions and flight control stay there. Drone Deck sends flight commands (arm, disarm, takeoff, land, return, hold, mode) and opens the PX4 console only for simulations it started itself ([Flight actions and PX4 console](#flight-actions-and-px4-console-simulation-only)); physical vehicles are read-only from Drone Deck. When the QGroundControl or scripts consumer is enabled for a physical system, those programs can command the vehicle through Drone Deck's link ([Physical vehicle](#physical-vehicle)). **Stop session** stops processes Drone Deck owns; it never lands, disarms or commands a vehicle.

Contents: [Systems](#systems) · [01 System](#01-system) · [02 Tools & config](#02-tools--config) · [03 Data & models](#03-data--models) · [04 Activity](#04-activity) · [Guided setup](#guided-setup) · [Settings](#settings) · [Where your data lives](#where-your-data-lives) · [Honest states](#honest-states)

Problems: see [troubleshooting](troubleshooting.md) and [known issues](known-issues.md).

## Systems

Everything is organised around one **active system**, chosen with the **Active system** selector at the top of each management page:

| System | What it brings together |
|---|---|
| Local simulation | PX4 SIH (autopilot + internal physics) → MAVLink router in WSL → Drone Deck's native vehicle link and live view, with QGroundControl and your own scripts on UDP 14540. No hardware needed. |
| ROS & sensor capture | PX4 + Gazebo (`x500_mono_cam`) → MAVLink router → vehicle link, plus Micro XRCE-DDS Agent and the ROS–Gazebo camera bridge → ROS 2 Jazzy → rosbag2/MCAP recordings, with QGroundControl and Foxglove. |
| Offline AI workspace | A recording replay → ONNX CPU runtime and your inference project → inference runs on replayed recordings (Data & models → Pipelines), with Foxglove. |
| Physical vehicle | Vehicle endpoint (serial/UDP/TCP) → native vehicle link and live view → local `.tlog` captures. QGroundControl and scripts are off by default; enabling them makes the link a two-way bridge. |

The four rows above are templates. A system is one vehicle source (PX4 SIH, PX4 + Gazebo world/model, ArduPilot SITL, a physical endpoint, or a recording replay) and the consumers it feeds (QGroundControl, a live view, the ROS 2 bridge and MCAP recorder, inference, Foxglove, your own scripts over UDP (two-way MAVLink)); its components, connections and workspace launch follow from those. A first start has one system per template; the ROS template uses Gazebo with `x500_mono_cam`, while a ROS system migrated from 0.8 keeps PX4 SIH. Create more with **New system…** in the system list (guided setup from a template) and rename, duplicate, delete or change source details in **Settings → Systems**. Switching systems is allowed while work runs: processes, launches and captures stay with the system that started them (a dot marks it in the list) and nothing is stopped; **Stop session** stops all owned work. All systems share one tool registry (launch profiles) and one WSL distribution.

The rail on the left holds **01 System**, **02 Tools & config**, **03 Data & models**, **04 Activity** and **Guided setup ↗**. Its foot shows the environment, the owned work that runs (processes, the MAVLink router, the vehicle link, a physical one too, recording or replay, a pipeline job), **Stop session** while any of it runs, **Settings** and the version. Below 720 logical pixels the rail becomes a drawer opened with **Menu**; Escape or a click outside closes it.

## 01 System

The System page is the active system in one place.

- **Start workspace** starts the system's session in stages, each waiting for the previous one:
  - PX4 SIH (Local simulation): PX4 SIH, then the MAVLink router once SIH reports vehicle status (Ready), then the vehicle link once the router listens, then QGroundControl once the link is up. "Up" means the TCP connection is open; the heartbeat shows separately in the live view.
  - PX4 + Gazebo (ROS & sensor capture): Gazebo, the router and the link, then the DDS Agent and the ROS–Gazebo camera bridge when the system uses the ROS agent, then QGroundControl. A PX4 SIH system with the ROS agent adds the DDS Agent after the link.
  - ArduPilot SITL: ArduPilot, then the link (it serves TCP 5760 itself, so there is no router stage), then QGroundControl.
  - Only one simulator runs at a time; a second is refused with the name of the system that runs the first.
  - Replay (Offline AI workspace): the button reads **Open projects** (or **Open recordings** without an inference consumer) and takes you to Data & models.
  - Physical vehicle: the button reads **Connect read-only**, or **Connect (bridged)** when QGroundControl or scripts are enabled for the system, and opens the vehicle link on the configured endpoint.

  While a session starts, the page shows which stage is waiting and for how long. A missing readiness signal, a process exit or a timeout stops the sequence with a specific message. While anything runs, the button becomes **Stop session** (confirmed).
- **First time here?** If the system is not prepared yet, **Prepare a system →** opens Guided setup. Once prepared, **Review setup ↗** reopens it. If WSL is not ready for a WSL-based system, this strip says so.
- **System connections** draws the planned chain for the system. A wire is dashed and labelled **Unverified** until there is live evidence for it, then solid and **Observed** (for example a router forwarding packets and a fresh heartbeat between the router and the vehicle link, a recorder that is recording between ROS 2 and MCAP, a fresh heartbeat between a physical endpoint and the link).
- **Branches** (simulation and ROS): start or stop QGroundControl; the state of your scripts' UDP address (simulation); **Open recordings ↗** (ROS).
- **Live vehicle** shows what the native vehicle link reports for the system that owns it: mode, armed, altitude, climb, ground speed, heading, battery, link rate and the last heartbeat, an attitude indicator and heading tape, and a position trace since the link came up (home is marked when the vehicle reports it, otherwise the first fix is labelled *Start*). Values grey out once no heartbeat arrived for five seconds; another system's page says whose link it is. When a simulation link is open but not connected (it keeps retrying, for example because its router died) and no launch is running, the live view shows **Disconnect link**. It closes only the link and sends nothing to the vehicle; the simulator and other tools keep running, and **Stop session** still stops everything. There is no Reconnect: stop the session and start the workspace again.
- **Runtime** lists every component of the system with its execution target, observed state and an action: Start/Stop for profiles, Connect/Disconnect for a physical link, or a link to Recordings, Projects, artifacts, logs or the endpoint. **Configuration & paths ↗** goes to Tools & config.
- **Running (previous session)** appears at the top of every page when the WSL agent finds processes an earlier Drone Deck session started and left running. **Adopt** follows their output and stops them with the session; **Stop** ends them now. Neither is a vehicle command.

### Preflight & health

Below the live view, **Preflight & health** shows only what the vehicle itself reports, read-only:

- **Ready to arm**: on PX4 v1.14 and later, from PX4's health and arming check report (the MAVLink events protocol). The line says whether the vehicle can arm in the current flight mode and lists the components with arming errors and warnings. When it cannot arm in the current mode, it also lists the modes it can arm in. Without that report, readiness comes from the pre-arm check bit in `SYS_STATUS`, then from pre-arm `STATUSTEXT` messages (`PreArm`, `Preflight Fail`, `Arming denied`). A vehicle that reports none of these shows **Not reported**; Drone Deck never guesses.
  - When the report and the `SYS_STATUS` bit disagree about the current mode, the stricter result is shown and the source says they disagree.
  - When events were missed since the report (a gap in the event sequence, for example after a link dropout), the report may be out of date. Readiness then comes from the `SYS_STATUS` bit if the vehicle sends it. Otherwise the report is shown with that warning and is never green. The next complete report clears the mark.
  - PX4 sends the report only when a check result changes, and Drone Deck does not ask for one. After connecting to a vehicle that is already running, the report appears only when a result changes or another ground station (such as QGroundControl) asks for checks. It is dropped when the vehicle reboots.
- **Arming source**: where readiness came from: the PX4 health and arming check report, the `SYS_STATUS` pre-arm bit, a `STATUSTEXT` pre-arm message, the heartbeat's armed flag, or nothing yet.
- **Health report** (when PX4 sent one): how many components reported, and which have errors or warnings. The section header counts components with errors. Individual check messages (for example a low GPS fix) are not decoded; read them in QGroundControl.
- GPS (fix, satellites, horizontal accuracy), battery (volts and percent) and landed state.
- The `SYS_STATUS` sensor table: each sensor's present, enabled and health bits.
- **Vehicle messages**, newest first: the `STATUSTEXT` lines the vehicle sent on this link.

### Flight actions and PX4 console (simulation only)

On a simulated system (PX4 SIH, PX4 + Gazebo, ArduPilot SITL) the System page also shows:

- **Flight actions**: **Arm**, **Disarm**, **Takeoff** (to the altitude typed beside it, 1–100 m above home), **Land**, **Return**, **Hold** and **Mode ▾** (the modes the flight stack offers). **Arm** and **Takeoff** open a confirmation first. Each command shows its result as the vehicle answers: *Accepted*, *Rejected* with the reason, *Timed out*, *Unsupported by this flight stack* or *Not sent* (for example no heartbeat yet, or for Takeoff, the vehicle has not reported its home yet).
- **Vehicle console / MAVLink shell**: type a PX4 shell command (`help`, `ver all`, `commander status`) and press Return or **Send**; Up and Down recall earlier commands. **Clear output** empties the view; **End shell session** closes the shell on the vehicle. ArduPilot has no MAVLink shell: on an ArduPilot SITL system the input, **Send** and **End shell session** are disabled with that reason, and nothing is sent.

Both are enabled only by the Authority gate, which needs evidence that the link is a simulation this app started:

1. the vehicle link is the app's own simulator link: through the MAVLink router Drone Deck started and that still reports itself up (PX4), or ArduPilot SITL on TCP 5760;
2. the system's source is a simulator;
3. its PX4 or ArduPilot profile runs and was started for this system.

When any of these is missing, the controls stay disabled and say why, for example "Workspace launch under way: starting *<stage>* (stage *i* of *n*)…" until the launch reaches its link stage, "No live link…" (naming another system whose launch runs, saying that the link was closed while the simulator still runs, or that the simulator runs but no link was opened for it), "Commands are only sent to a simulated vehicle.", "The link is not the app's own simulator link. Start the workspace from Drone Deck.", the router's own error, or "*<simulator>* is not running under this system." On another system's page the bar names the system whose link it is. Physical systems never show these controls, and the vehicle link refuses flight commands and shell lines on a physical link whatever asks for them.

Flight actions and the console have been tested only against a loopback fake vehicle, not a real PX4 or ArduPilot (see [known issues](known-issues.md), VAL-02).

### Physical vehicle

With **Physical vehicle** active, the System page holds the vehicle connection:

1. Enter an endpoint: `serial:COM4:115200`, `udp:0.0.0.0:14550` (listen) or `tcp:192.168.1.10:5760`, or click **Find serial devices** and pick a COM port. Use a dedicated endpoint; another app (often the ground station) may already own a serial port.
2. **Connect read-only** (**Connect (bridged)** when QGroundControl or scripts are enabled). Drone Deck's native vehicle link sends no ground-station heartbeat, arm, mode, parameter-write or flight command. The only messages Drone Deck itself sends are read requests: one home-position request when the vehicle is first seen (`MAV_CMD_REQUEST_MESSAGE` for `HOME_POSITION`, or `MAV_CMD_GET_HOME_POSITION` if the firmware refuses that), and the parameter list, onboard log list and log downloads you start. If QGroundControl or the scripts address is enabled for the system, the link is a two-way bridge: what those programs send (commands and parameter writes included) reaches the vehicle unchanged. The connection panel counts it under **Sent by QGroundControl/scripts**, next to **Sent by Drone Deck**.
3. The live view shows the vehicle once an autopilot heartbeat arrives; ground stations and peripherals are ignored. Values grey out after 5 seconds without a heartbeat. A UDP-listen link stays with the address of the first autopilot heartbeat until it closes: if the vehicle or a radio bridge changes its source port, **Disconnect** and **Connect** again.
4. Every session is recorded to a timestamped `.tlog` under the data folder's `logs\tlog\` (listed in Data & models → Flight logs). **Open capture folder ↗** opens it. Each capture stops at 2 GB; before a link opens, the oldest captures are deleted so the folder keeps at most the newest 100 and 2 GB (copy one elsewhere to keep it). A link that never received a frame leaves no file, and a capture that cannot be created leaves the link open without one ("Capture not started").

**Vehicle tools** holds two read-only tools; they work once the link is up and an autopilot heartbeat has arrived, and otherwise say what is missing:

- **Parameters**: **Fetch parameters** reads every parameter from the vehicle, with progress; nothing is written. Search by name or tick **Changed only**. **Export .params** saves a QGroundControl-format file to `exports\params\<system>-<stamp>.params` in the data folder (**Open exports folder ↗**). **Compare with a .params file**: type or paste the path of an earlier export (there is no file picker; after an export, **Compare later with this file** fills in the active system's last export of this app session) and **Load and compare** to list parameters that are *Changed*, *Only on vehicle* or *Only in file*; **Clear comparison** ends it. The file is read in the background: the button reads **Loading…**, a "Reading *<path>*…" line shows, and **Stop waiting** drops a slow read, for example from an offline network share. A comparison belongs to its system: after you switch systems the page shows that system's own comparison, a load that finishes after the switch is kept for the system it was started for, and **Clear comparison** and **Stop waiting** act on the active system only. Drone Deck neither signs its requests nor checks MAVLink signatures, so a vehicle that requires signing may ignore the parameter and log requests.
- **Onboard logs**: **List logs** asks the vehicle for its log list; **Download** fetches one log at a time into `logs\vehicle\` in the data folder, with progress and **Cancel download**. A partial download resumes from its `.part` file. Nothing on the vehicle is erased. **Open in Flight logs →** shows a downloaded PX4 ULog (`.ulg`) in Data & models; Flight logs does not read ArduPilot DataFlash `.bin` logs, so for those use **Show in folder ↗**.

Parameter editing and flight commands stay in QGroundControl or Mission Planner. These tools were tested with a loopback fake vehicle only, not a real flight controller (see [known issues](known-issues.md), VAL-04).

## 02 Tools & config

Tools & config lists the tools behind the active system, or every registered tool with **All registered tools** (including Gazebo, ArduPilot and your own). Filter by category (Simulation, Transport, Telemetry, Ground station, Data, AI) or search by name, description or category.

- **Detect tools** checks each profile's installation probe (inside WSL for Linux tools, a file path for Windows tools). A probe proves presence, not compatibility or a working connection.
- Each row shows the tool, its execution target (WSL or Windows), its state, **Start/Stop** and **Configure ▾**.
- **Configure** expands the row in place: guidance, the exact command with **Copy command**, the working directory and the launch-profile form. The form is locked with "Stop this process before editing." while the tool runs. From the expanded row you can also **Detect** the installation, **Review installation** (the managed recipe, see below), open the **Official docs ↗** and **Open folder ↗**.
- **Add a tool** creates a new profile for a ROS node, inference server, recorder, alternative simulator or any other command.

**Remove a custom tool:** open its **Configure** form, choose **Remove tool** beside **Save profile**, then confirm. Stop the tool first if it is starting or running. Removal deletes only its saved launch profile; installed software, projects, recordings and logs remain. Built-in tools cannot be removed through this control.

### Launch profiles

A profile is one launchable tool. Fields:

| Field | Meaning |
|---|---|
| Display name | Shown throughout the app. |
| Program | `wsl` for a Linux command, or a full Windows executable path. |
| Command / arguments | For `wsl`: one Bash command, run with explicit execution inside the configured distribution (variables such as `$HOME` and `$DRONE_DECK_SCRIPTS` reach Bash intact). For Windows: one argument per line, no JSON or surrounding quotes. |
| Windows working directory | Optional, Windows programs only. WSL commands `cd` themselves. |
| Installation probe | A Bash test for WSL tools, or a file path for Windows tools. |
| Ubuntu distribution | Shared by every WSL profile. |
| Category | Simulation, Development, Infrastructure, Flight operations, or AI & data. |

Profile IDs are unique letters, digits, hyphens and underscores. Profiles marked `exclusive` (PX4 SIH, Gazebo, ArduPilot) are alternative flight stacks: only one can run at a time, and an externally started PX4 also blocks a second start. Every managed process gets lifecycle control and log capture without opening a terminal window; QGroundControl and Gazebo open their own windows.

QGroundControl's row also has **GPU-safe mode (software rendering)**, off by default. Turn it on if QGroundControl closes at start or opens a blank window; see [troubleshooting](troubleshooting.md#qgroundcontrol).

The whole registry can also be edited as JSON in **Settings → Profile registry** (advanced).

### Managed installation

**Review installation** opens the installation review for a tool's recipe: source and version, destination, packages/components, required privileges and how the result is verified. Nothing installs until you click **Install reviewed tool**.

- Compatible existing installations are offered for reuse instead of being upgraded.
- App-owned Linux tools go under `~/.local/share/drone-deck/tools` in the selected distribution; shared apt prerequisites may be installed through the distribution's root account. Source builds have no reliable size or duration estimate.
- **Apply the verified launch configuration after success** (on by default) points the tool's profile at the verified result. Completed configuration is recorded so reopening the app does not overwrite your later edits.
- Installation is blocked while tools, recordings or other installations run; tools that depend on an installation wait for it.

Maintained recipes: WSL, Linux distribution, ROS 2 Jazzy with rosbag2/MCAP, MAVSDK 4.1.0, PX4 v1.17.0 SIH (built with `DONT_RUN=1`, no Gazebo or firmware cross-compilers), Micro XRCE-DDS Agent 2.4.3 (for a DDS-v2 PX4 client), the PX4 v1.17 message workspace (`px4_msgs`, requires ROS Jazzy), Gazebo Harmonic (runtime only, not every PX4 world/bridge), pinned ArduPilot Copter SITL, the ROS–Gazebo bridge (`ros-jazzy-ros-gz`) and the Foxglove bridge (`ros-jazzy-foxglove-bridge`, both need ROS Jazzy), ONNX CPU inference, and QGroundControl v5.1.5 AMD64. The QGroundControl recipe downloads the official installer, checks its published SHA-256 or Authenticode signature, and opens the visible Windows installer; if neither check is available the file is left unexecuted and the job reports **Needs input**.

Linux recipes target Ubuntu 24.04; other distributions are labelled Advanced. Clean-machine validation of the recipes is still outstanding (see [known issues](known-issues.md)).

Installation jobs are listed under **Activity → Installations**.

## 03 Data & models

Five tabs: Recordings, Models, Pipelines, Projects and Flight logs. Each has a status block, a library and honest empty states.

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

### Pipelines

Pipelines work on finalized recordings in the selected distribution (the WSL agent's `pipe.*` methods, `scripts/agent/pipe.py`). Select a recording; its image topics are preselected. One job runs at a time; the status line shows its progress and **Cancel** stops it. A cancelled job stays on the status line as cancelling until the agent has stopped its processes (up to a minute while an inference recorder finalizes its MCAP). A running job is owned work like a process: the rail lists it, **Stop session** and closing the app cancel it, and installations or a distribution change wait until it ends.

- **Extract frames** writes `<recording>/extract/<name>/`: one file per kept message plus `index.csv` (`timestamp_ns,topic,file`) and `manifest.json` (source MCAP hashes, topics, counts, a SHA-256 per file, tool versions, status). `sensor_msgs/Image` (`rgb8`, `bgr8`, `rgba8`, `bgra8`, `mono8`, `mono16`/`16UC1`) becomes PNG or an NPZ array; `CompressedImage` keeps its JPEG/PNG bytes (or an NPZ of them); other messages become JSON lines per topic (`<topic>.jsonl#<line>` in the index). **Start**/**End** are seconds from the first message; **Max rate** keeps at most that many messages per second per topic. Reading needs the `mcap` and `mcap-ros2-support` Python packages (`python3 -m pip install --user mcap mcap-ros2-support`; `numpy` for NPZ) or ROS 2 `rosbag2_py`; the error names what is missing. A cancelled extraction keeps its partial output, marked `Cancelled`.
- **Infer on replay** starts three owned agent processes on an isolated, localhost-only ROS domain (default 78; it must differ from the capture domain): a rosbag2 recorder for `/drone_deck/detections`, the inference node (`scripts/agent/infer_node.py`: rclpy + onnxruntime on the CPU, using the ONNX environment from Setup when installed) and `ros2 bag play` of the chosen image topic only. The node resizes each image to the model's first input, scales float inputs to 0–1 and publishes one JSON `std_msgs/String` per image (small output tensors in full, otherwise the top-5 scores). When the replay ends, or you cancel, all three stop together. `<recording>/inference/<run>/` holds the detections MCAP and `run.json` (model and node SHA-256, recording, topics, commands, timings, exit codes, detection count). Decoding boxes or classes is model-specific: copy `infer_node.py` into a project folder and enter the project's name, and runs use that copy.
- **Runs** lists extractions and inference runs, newest first, with status (`Completed`, `Cancelled`, `Failed`, `Interrupted` when the app or agent stopped mid-run) and outputs. **Show folder ↗** opens it through `\\wsl.localhost`. The list of recordings and runs is read the first time you open the tab and after a job ends; press **Reload recordings** to see a recording made since.
- **Open in Foxglove** (inference runs, and recordings in the Recordings tab) opens the `.mcap` through `\\wsl.localhost\<distro>\…` (or `\\wsl$\…`) with the app Windows associates with `.mcap`. If Windows cannot reach or open that path, the file is copied to `<data root>\exports\` and the copy is opened. **Live in Foxglove** starts the owned `foxglove` profile (`foxglove_bridge`, port 8765, reached through WSL localhost forwarding) and asks Windows to open `foxglove://open?ds=foxglove-websocket&ds.url=ws://localhost:8765` once the profile reports Running and port 8765 on `127.0.0.1` accepts a TCP connection. While it waits, the status says so. If the port does not accept a connection within 20 seconds of the bridge running, it says so and does not open Foxglove. Any program listening on that port passes the check. It shows the live graph of the capture domain. It needs the `foxglove` profile and a ready Linux environment.

Not verified on this development machine: it has no WSL distribution, ROS or Foxglove. The Python side is covered by unit tests with generated MCAP files and fake processes; the Rust side by tests with a fake agent.

### Projects

Create an editable **ONNX inference**, **ROS 2 subscriber** or **MAVSDK monitor** project. Creation writes source, instructions and a manifest only, under `~/.local/share/drone-deck/projects` in the selected distribution (recorded in the manifest).

- Prepare the runtime first (**Prepare AI runtimes ↗** opens Guided setup for the Offline AI workspace).
- Choose a model file and inputs, then **Run**. Output goes to Activity → Process output; **Stop** and **Stop session** stop owned project processes. The project's saved distribution must match the selected one.
- The ONNX starter takes a model and preprocessed NPZ tensors named for its inputs, records CPU batch latency, model/input hashes and outputs, and writes `predictions.csv` when classification targets are given. Its latency is the amortized time of one batch, not a per-frame real-time deadline. Camera/LiDAR decoding, calibration, timestamp alignment and preprocessing are the project's job.
- The MAVSDK starter runs in WSL with the adopted MAVSDK environment and only reads position telemetry. By default it listens on `udpin://0.0.0.0:14540`, where PX4 SITL sends its API stream inside WSL; only one program can own that port. Pass `--endpoint` for another source. The vehicle link's scripts address (default `127.0.0.1:14540`) is for scripts running on Windows, which a WSL process cannot receive. Not verified against a live PX4 here.

### Flight logs

Flight logs lists logs from three sources, newest first, and **Refresh list** searches again:

- **PX4 SITL (WSL)**: the ULog files PX4 writes under `build/<target>/rootfs/log` each time the simulator arms, in the PX4 folders your profiles name. Searched only when the Linux environment is ready.
- **Vehicle downloads**: ULog files downloaded from a vehicle (System → Vehicle tools → Onboard logs), in `logs\vehicle\` in the data folder. **Downloads folder ↗** opens it.
- **Telemetry captures**: the `.tlog` of every vehicle link session (`logs\tlog\`, and `hardware\` for captures written by 0.8 and earlier).

Select a log to read it on a background thread. Drone Deck summarizes what was logged and never makes up a value (*Not logged* when a field is missing):

- **ULog**: duration, start time (UTC, from GPS), maximum altitude, distance, maximum ground speed, minimum battery, armed time, failsafes, logged warnings and errors, dropouts, estimator faults and the file's contents (ULog version, topics, parameters), with plots of local position, battery, attitude and vibration (armed time shaded, failsafes marked). A truncated file is summarized as far as it goes and damaged regions are skipped; the summary says so. Files over 512 MB are not read into memory.
- **`.tlog`**: read with the vehicle link's own MAVLink decoder (frames that fail their CRC check are not counted, any size is streamed): duration, vehicle, packets, heartbeats, message types, armed time, highest altitude above home, lowest battery, modes, status text and damage (CRC failures, skipped bytes, truncation).

**Open in PX4 Flight Review ↗** opens Flight Review's upload page in your browser and shows the file in Explorer so you can drop it on the page; Drone Deck uploads nothing itself, and detailed analysis stays there. **Show in folder ↗** opens the file's folder. The ULog reader was tested with synthetic files only, not logs from a real PX4 (see [known issues](known-issues.md), VAL-10).

## 04 Activity

- **History**: every action you requested that changed something (starts and stops, profile saves, detection, recordings, installations, systems prepared), with time, system and kind. Kept in `activity.json` in the data folder. **Export history** writes `activity-export.json` and shows its path.
- **Process output**: the captured stdout/stderr of every managed tool, filterable per tool, errors in red. **Export log** writes `session.log` in the data folder; **Clear** empties the view. Lines are capped in length and in memory.
- **Installations**: setup jobs with tool, status, step/total and message; the selected job's log tail; **Cancel after current step**, **Review retry** and **Apply verified configuration** for a finished job.
- **Export diagnostics** writes `exports\drone-deck-diagnostics-<stamp>.zip` in the data folder in the background (the button reads **Exporting…**) and opens that folder when it is done. The zip holds the version and OS, owned process and job state, profiles, systems, activity, process output, the app log, the newest crash reports and installation job logs, each cut to its last 512 KiB. User folders, user, computer and domain names and tokens are replaced by placeholders; process output can still name your projects or files, so review the zip before sharing it.

Drone Deck writes an application log, `app.log` (rolled to `app.1.log` and `app.2.log` at 1 MiB), and a `crash-<stamp>.txt` report when it hits an internal error, in `%LOCALAPPDATA%\DroneDeck\logs\` (`<data root>\logs` when the data folder is moved with `DRONE_DECK_DATA_DIR` or `portable.txt`). The next start offers the newest report once in a strip at the top of Activity, with **Open logs folder ↗** and **Dismiss**.

Installation jobs keep running if Drone Deck closes and are restored when it reopens. A request still being prepared is not: if Drone Deck closes before the job's worker has started, the install does not start or shows **Interrupted**, so wait until the job is listed before closing. Only one installation worker runs at a time, even across app instances. Cancellation is checked at safe step boundaries: package managers and source builds finish their current step and are never killed mid-change. A step that runs past its time budget or goes quiet for five minutes shows **Needs attention**. Cancellation is not an uninstall or rollback. **Review retry** restores the original recipe, distribution and custom location and rechecks completed work. A job with no live worker after startup is shown as **Interrupted**.

Simulator, recorder and project processes are different: they stop when the app closes normally.

## Guided setup

**Guided setup ↗** prepares one system end to end in four chapters.

1. **Intention**: choose Watch a virtual drone (Local simulation), Capture sensor data (ROS), an Offline AI workspace or a Physical vehicle.
2. **The system**: the blueprint of stations and connections. **Inspect or adapt the technical recipe** lets you pick the execution target (an installed WSL distribution, or Windows local for the physical vehicle) and an existing tool path. Give the system a name (up to 60 characters).
3. **Preparation**: **Check this computer** scans Windows/WSL if needed, then detects each tool the system needs. Each row says **Found** (reuse after verification), **Not found** (review setup) or **Found / incompatible** with the reason. If WSL or a distribution is missing, those reviews come first. Read the plan (what is reused, installed and configured, with sources, locations and verification), tick **I reviewed the changes for this system.** and **Apply reviewed plan**. Missing tools install one at a time; a failed, input-required or restart-required job pauses the rest until you retry. The queue survives an app restart; a queue from another version or a changed environment pauses for review. On success the system is marked prepared and becomes active.
4. **First run**: three real tasks for the system, for example start the virtual vehicle, open the ground station and read telemetry. **Try this step** runs the real action; **Confirm observation →** is enabled once evidence exists (or after the action was requested, for things only you can see such as an external window). **Skip for now** is always available. The receipt lists the evidence collected.

Nothing installs from a check alone. **Start over ↗** resets the journey. Headless previews disable every action.

## Settings

Open Settings from the foot of the rail or with **Ctrl+,**. **Systems** comes first; the other sections apply to this computer and every system.

- **Systems**: every saved system with its template and whether it is prepared, active or running. **Make active**, **Edit ▾** (name, source details such as the Gazebo world and model, the ArduPilot vehicle, the physical endpoint or the replayed recording, and the consumers with their ROS domain, project and scripts address), **Duplicate** and **Delete system** (confirmed; refused for the only system or one whose session runs; only the saved setup is removed). **Save system** stores the change in `systems.json`; nothing is installed or started by saving. **New system ↗** opens Guided setup for a new system. On a physical system, enabling **QGroundControl** or **User scripts** makes the link a two-way bridge: those programs can then command the vehicle, and the editor says so under the consumers. Keep the scripts address on `127.0.0.1`. With scripts on, the address must be an IP address and port: a host name (`localhost` included), `0.0.0.0` or `[::]`, or port 0 shows the reason in red under the field and is refused on save, since host names are not resolved and nothing would be forwarded. On a physical system, an IP address that is not loopback shows an amber warning under the field and in the save message: the computer at that address receives the vehicle's telemetry and can command the vehicle. Saving is not blocked by that warning.

- **Environment**: computer readiness (Windows build, WSL, virtualization, memory and graphics; checking changes nothing) and the Linux environment. Pick an installed distribution and **Use this environment**, or review installing WSL or a distribution from Microsoft's live catalog (offline errors are shown, never a made-up list). Installing WSL may show a UAC prompt and need a restart; Drone Deck never restarts Windows. A new distribution's first launch asks for a Linux user: **Initialize Linux user ↗** opens it; tool installs need a regular user, not root. Changing the distribution does not move tools: detect them again for the new distribution. Existing WSL 1 distributions are never converted or removed.
- **Display** (interface zoom): 50–150 %, default 100 %. **Smaller −**/**Ctrl+−** and **Larger +**/**Ctrl++** (or **Ctrl+=**) change it by 5 %; **Reset to 100%**, a click on the percentage in the bottom-right controls, or **Ctrl+0** resets it. The bottom-right controls are on every page. Zoom scales and reflows the UI; it does not change Windows display scaling. The choice is saved in `ui-preferences.json`.
- **Model access**: the Hugging Face token (see [Models](#models-hugging-face)).
- **Profile registry** (advanced): **Edit JSON** for the whole registry, **Validate & save** or **Cancel**. Saving is refused if it would remove a running profile or change the distribution while anything is active. **Open folder ↗** opens the data folder.

## Where your data lives

| What | Where |
|---|---|
| Profiles, systems, activity, UI preferences, setup jobs, vehicle `.tlog` captures (`logs\tlog\`), downloaded onboard logs (`logs\vehicle\`), parameter exports (`exports\params\`), diagnostics zips and other exports (`exports\`) | `%LOCALAPPDATA%\DroneDeck\workspace\` |
| Application log and crash reports | `%LOCALAPPDATA%\DroneDeck\logs\` |
| Downloaded models | `%LOCALAPPDATA%\DroneDeck\models\` |
| Hugging Face token | Windows Credential Manager |
| Managed Linux tools, recordings (with their pipeline outputs), projects | `~/.local/share/drone-deck/` inside the selected WSL distribution |

To keep the data folder beside the executable instead (a USB stick or a self-contained folder), put an empty `portable.txt` next to `Drone Deck.exe`; the data folder is then `workspace\` beside it. Downloaded models and the token stay in their per-user locations above. To use another folder, set the environment variable `DRONE_DECK_DATA_DIR`. In both cases the application log and crash reports go to `logs\` inside that data folder. Earlier versions kept data beside the executable; the first start of this version copies it to the new location and leaves `MIGRATED-TO.txt` in the old folder. Details: [troubleshooting → data locations](troubleshooting.md#where-drone-deck-keeps-things).

## Honest states

Drone Deck keeps these separate everywhere: **installed** (a probe or verified recipe), **configured** (a profile points at it), **running** (the process is alive), **connected** (protocol traffic observed), **healthy** (fresh data). A running visualizer is not proof it receives state; a downloaded model is not a ready runtime; a probe is not a compatibility test; Stop session is not a vehicle command. Dashed connections are unverified.

Not provided: embedded simulator or ground-station windows, flight commands to physical vehicles (arming, mode changes and the rest), parameter writes, mission upload, offboard control, firmware flashing, a ROS graph browser, remote/onboard execution. See the [roadmap](../ROADMAP.md).

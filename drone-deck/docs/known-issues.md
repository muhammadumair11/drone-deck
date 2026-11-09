# Known issues

Last reviewed: 9 November 2026, against the phase 1 source tree (after 0.8). This list replaces the 0.5.1 issue review and the per-version verification reports that used to live in `verification/`. Each item was re-checked against the current code; "fixed" items name where the fix lives so a regression is easy to spot.

Workarounds are in [troubleshooting](troubleshooting.md).

## Open

| ID | Area | Issue | Notes / workaround |
|---|---|---|---|
| VAL-01 | Setup | Fresh-machine installation is unverified end to end: WSL feature install with UAC and restart, new distribution and Linux user, and clean source builds of PX4 SIH, the DDS Agent, the PX4 message workspace, Gazebo, ArduPilot and the jMAVSim build. The QGroundControl recipe's real installer has not been run by Drone Deck. | Recipes, their reviews and non-installing fixtures are tested; existing installations were discovered and reused. Treat a first install on a new PC as a trial and read the job log. |
| VAL-02 | Simulation | A complete takeoff → hover → landing cycle through a Drone Deck–started session has not been recorded. Takeoff to about 2.8 m (QGroundControl) and a live SIH → relay → jMAVSim chain with MAVSDK telemetry were observed separately. | Flight control is QGroundControl's; Drone Deck sends no flight commands. |
| VAL-03 | ROS | The live PX4 → DDS Agent → ROS 2 topic path has not been verified. Installing the Agent and `px4_msgs` does not establish routing; the firmware DDS client, Agent and message versions must match. Recording itself was verified with generated IMU data. | Discover topics before recording; PX4 SIH alone publishes no ROS topics. |
| VAL-04 | Physical vehicle | The native monitor was tested with synthetic loopback MAVLink only, not a real flight controller or serial device. MAVLink signatures are not authenticated, and parameter reads on signed links are refused. | Use a dedicated endpoint; keep QGroundControl/Mission Planner for configuration. |
| VAL-05 | Models | Authenticated, gated, large-weight and interrupted Hugging Face downloads have not had acceptance testing. A small public pinned file was downloaded and hash-verified. | Errors are reported per job; retry reuses verified files. |
| VAL-06 | AI | ONNX project runs have not been exercised with the optional runtime installed on a test machine. | Prepare the runtime through Guided setup → Offline AI workspace first. |
| ENV-01 | Simulation | The jMAVSim relay depends on a Windows firewall rule for inbound UDP 19410 that Drone Deck does not create or diagnose. The WSL address range can change after a restart and break a rule scoped to one address. Relay "Forwarding" does not prove delivery through the firewall. | See [troubleshooting → relay](troubleshooting.md#simulation-relay-firewall-and-visualizer). |
| ENV-02 | Simulation | QGroundControl on Windows may not see PX4 running inside WSL 2 without a manually added UDP comm link. Drone Deck does not configure QGroundControl. | See [troubleshooting → ports](troubleshooting.md#ports). |
| ENV-03 | Setup | Changing the WSL distribution keeps every profile's old paths; there is no consolidated per-tool migration checklist. | Run Guided setup → Preparation (or Detect tools) for the new distribution. |
| CFG-01 | Defaults | The default jMAVSim profile's working directory and probe, and one discovery candidate in `scripts/setup/windows-tools.ps1`, still point at `%USERPROFILE%\Desktop\Simulator Launchers\jMAVSim-Windows`, a location from the original development machine. On other PCs the default profile shows as not installed. | Detect jMAVSim with its real JAR path, or let Guided setup install the managed copy in `%LOCALAPPDATA%\DroneDeck\tools\jmavsim`. Source: `backend::defaults`. |
| CFG-02 | Readiness | **Detect tools** still runs each profile's installation probe (presence only), while Guided setup and Review installation run recipe discovery with compatibility checks. The two can disagree. | Prefer Preparation's result; a probe never claims compatibility. |
| DOC-01 | Data & models | The **Recording guide ↗** button (`src/recordings/view.rs`) opens `<data root parent>\drone-deck\RECORDINGS.md`. That guide was merged into [user-guide.md](user-guide.md#recordings-rosbag2--mcap), and with the data root in `%LOCALAPPDATA%` the path never resolves. | Point the button at `docs/user-guide.md` beside the source or at a hosted copy. |
| LOG-01 | Diagnostics | No application log or crash file is written yet; `%LOCALAPPDATA%\DroneDeck\logs\` is reserved for it. | Use Activity → Process output → **Export log** and the setup job logs. |

## By design (not defects)

- Cancelling an installation waits for the current package-manager or build step; it is never killed mid-change, and cancellation is not rollback. A step past its time budget or silent for five minutes shows **Needs attention**.
- A running process is never reported as connected or healthy; dashed connections stay unverified until traffic is observed.
- Only one exclusive flight stack runs at a time; a PX4 started outside Drone Deck blocks a second PX4.

## Fixed

From the 0.5.1 review (November 2026), re-checked in the current code. SET, SIM and RUN keep the review's IDs; UX and GAP number its unnumbered usability points and capability gaps (its readiness-label and distribution-change points are CFG-02 and ENV-03 above).

| ID | Was | Fixed in | Where |
|---|---|---|---|
| SET-01 | QGroundControl discovery accepted any file (e.g. a `.txt`) as a compatible installation. | 0.6 | `windows-tools.ps1` checks the `MZ` header, product name and version; an invalid explicit location is not replaced silently. |
| SET-02 | An incomplete `~/PX4-Autopilot` hid a working app-managed PX4. | 0.6 | `linux-tools.py` keeps searching past incompatible candidates. |
| SET-03 | An empty `build/px4_sitl_sih` folder made a usable SITL build look incompatible. | 0.6 | Build target chosen by an executable binary, not folder presence. |
| SET-04 | An incompatible MAVSDK 3.x environment hid a compatible 4.x one. | 0.6 | Candidate fallback in `linux-tools.py`. |
| SET-05 | Gazebo discovery ignored the supplied location. | 0.6 | `extra_tools.py` uses the given executable or folder. |
| SET-06 | Retry lost the original custom location. | 0.6 | `setup::retry` restores distribution and location (test `retry_retains_original_target_location_and_distribution`). |
| SET-07 | Logged installer commands ignored their timeout. | 0.6 | `common.ps1` enforces the deadline; preserved package transactions publish **Needs attention** instead. |
| SET-08 | A cancel during the visible QGroundControl installer was ignored and the job could report Ready. | 0.6 | Cancellation is checked after the installer closes. |
| SET-09 | Advanced JSON editing could switch the WSL distribution while tools ran. | 0.6 | `State::apply` refuses a distribution change while busy. |
| SET-10 | PX4 retry did not recover an interrupted recursive submodule checkout. | 0.6 | `git submodule update --init --recursive` before building on retry. |
| SIM-01 | WSL launches expanded `$DRONE_DECK_SCRIPTS` too early, so the relay started `/relay.py` and exited. | 0.6 | `wsl.exe -d <distro> --exec` in `backend.rs`. |
| SIM-02 | jMAVSim opened but received no state. | 0.6 | Consequence of SIM-01; the staged launch now waits for relay Forwarding. (The firewall dependency remains: ENV-01.) |
| SIM-03 | PX4 flooded `pxh>` prompts (≈1.6 cores) and an unterminated line could grow without bound. | 0.6 | `supervisor.py` keeps stdin open; `process_output::BoundedLines` caps allocation before the newline. |
| SIM-04 | The duplicate-PX4 guard became `test -z ""`. | 0.6 | Same fix as SIM-01. |
| RUN-01 | Workspace start launched SIH, relay and jMAVSim at once and called each Running on spawn. | 0.6 | `session.rs` staged launch with readiness, exit and timeout handling. |
| UX-01 | No workflow-level setup; users assembled each tool themselves. | 0.6 checklists, 0.8 | Guided setup: Intention → The system → Preparation → First run. |
| UX-02 | Two download systems: managed installation jobs and a raw "download package" action. | Phase 1 | The legacy package download was removed; only reviewed installation jobs remain. |
| UX-03 | A verified install still needed a separate Apply step. | 0.6 | **Apply the verified launch configuration after success** (recorded so later edits are not overwritten). |
| UX-04 | Custom tools required JSON and Windows/Linux path knowledge. | 0.6, 0.8 | Plain profile form (one Linux command, or one Windows argument per line), location picker; JSON is the advanced route. |
| UX-05 | The installation review hid packages, destinations and privileges. | 0.6 | Review lists destination, privileges, packages/components and verification. |
| UX-06 | Installation was not coordinated with running tools. | 0.6 | Installs are refused while tools/recordings run; dependent launches wait for installs. |
| UX-07 | A long or stalled step gave no feedback. | 0.6 | **Needs attention** marker with the job log. |
| GAP-01 | No automatic recipes for jMAVSim, Gazebo or ArduPilot. | 0.7 | Maintained recipes (clean-machine validation still open: VAL-01). |
| GAP-02 | No Hugging Face, datasets, evaluation or physical-vehicle support. | 0.7 | Data & models, and the Physical vehicle system (acceptance still open: VAL-04, VAL-05). |

# Known issues

Last reviewed: 10 November 2026, against the 0.9 source tree (WSL agent, MAVLink router and native vehicle link, simulation-only flight actions, Flight logs, Pipelines, diagnostics, installer). This list replaces the 0.5.1 issue review and the per-version verification reports that used to live in `verification/`. Each item was re-checked against the current code; "fixed" items name where the fix lives so a regression is easy to spot.

Workarounds are in [troubleshooting](troubleshooting.md).

## Not verified

Nothing below has been run in a real environment. The development machine is Windows 10 with no WSL distribution, PX4, Gazebo, ArduPilot, ROS 2, QGroundControl session, Foxglove, Inno Setup, serial device or vehicle. What was tested instead is named in each row.

| ID | Area | Not verified | Tested instead / workaround |
|---|---|---|---|
| VAL-01 | Setup | Fresh-machine installation end to end: WSL feature install with UAC and restart, new distribution and Linux user, and clean source builds of PX4 SIH, the DDS Agent, the PX4 message workspace, Gazebo, ros-gz, the Foxglove bridge and ArduPilot. The QGroundControl recipe's real installer has not been run by Drone Deck. | Recipes, their reviews and non-installing fixtures are tested; existing installations were discovered and reused in earlier versions. Treat a first install on a new PC as a trial and read the job log. |
| VAL-02 | Simulation | A complete takeoff → hover → landing cycle through a Drone Deck–started session. The router → native link path, the Authority gate, the flight commands and the console ran only against a fake agent and a fake vehicle on loopback, never a real PX4, Gazebo or ArduPilot. The router keeps the first UDP sender as its vehicle; whether PX4 SITL always sends from the same source port (normally 18570), and what happens when PX4 restarts on a new port while the router runs (the gate then asks for a workspace restart), was not observed. | Treat the first real session as a trial: watch the launch stages and the live view's heartbeat. |
| VAL-03 | ROS | The live PX4 → DDS Agent → ROS 2 topic path. Installing the Agent and `px4_msgs` does not establish routing; the firmware DDS client, Agent and message versions must match. Recording itself was verified with generated IMU data in an earlier version. | Discover topics before recording; PX4 SIH alone publishes no ROS topics. |
| VAL-04 | Physical vehicle | The vehicle link on a real flight controller. It ran only with synthetic MAVLink on loopback UDP and TCP; no `serial:COMx:baud` endpoint has been opened on a real port. MAVLink signatures are not checked and Drone Deck's own requests are unsigned, so a vehicle that requires signing may ignore its parameter and log requests. The HOME_POSITION request at link-up (`MAV_CMD_REQUEST_MESSAGE`, falling back to `MAV_CMD_GET_HOME_POSITION`) follows the MAVLink, PX4 and ArduPilot documentation and was answered only by the fake vehicle; a vehicle without a home yet may not answer, and the trace then says "home not reported". Parameter fetch, export and compare, and onboard log download, were tested against the fake vehicle only. | Use a dedicated endpoint; keep QGroundControl or Mission Planner for configuration. |
| VAL-05 | Models | Authenticated, gated, large-weight and interrupted Hugging Face downloads. A small public pinned file was downloaded and hash-verified in an earlier version. | Errors are reported per job; retry reuses verified files. |
| VAL-06 | AI | ONNX project runs with the optional runtime installed on a test machine. | Prepare the runtime through Guided setup → Offline AI workspace first. |
| VAL-07 | Simulation | The WSL agent (`scripts/agent/agent.py`), its process groups, leftover adoption and the MAVLink router (`router.py`) on a real distribution. In particular: `wsl.exe --cd <Windows path>` combined with `--exec`, and its path translation under a custom `[automount] root`; the router's UDP socket on `127.0.0.1:14550` receiving the ground-station stream of PX4 SIH and Gazebo; WSL localhost forwarding reaching the router's TCP 5760 (bound to `0.0.0.0`) from Windows. | Python unit tests on generated processes and sockets, Rust tests against a fake agent. Watch Activity → Process output on the first start; **Running (previous session)** lists anything an earlier session left running. |
| VAL-08 | Simulation | Gazebo camera topic names and the `ros_gz` remaps (`/world/<world>/model/<model>_0/link/camera_link/sensor/camera/{image,camera_info}` → `/camera/image_raw`, `/camera/camera_info`). They come from the PX4-gazebo-models description of `x500_mono_cam`; other models or worlds need their own topic names. | Run `ros2 topic list` in the distribution and adjust the `rosgz` profile if the names differ. |
| VAL-09 | Data & models | Pipelines (frame extraction with `mcap` or `rosbag2_py`, inference on an isolated replay with rclpy and onnxruntime, `ros2 bag record` and `play` under the agent, the recorder finalizing its MCAP on stop), **Open in Foxglove** (UNC path through `\\wsl.localhost`, else a copy in `exports\`) and **Live in Foxglove** (`foxglove_bridge` and the `foxglove://` handler). | A fake agent, fake processes and generated MCAP files. Read the job's status line and Activity → Process output; the error names a missing reader package. |
| VAL-10 | Data & models | The ULog parser and summaries on logs written by a real PX4, and `.tlog` summaries of real captures. | The synthetic files in `src/ulog/fixture.rs` and generated `.tlog` data. Cross-check a summary with **Open in PX4 Flight Review ↗**. |
| VAL-11 | Release | `Build.ps1 -Installer` (Inno Setup) and the CI `release` job for a `v<version>` tag have never run; the installer script has never been compiled. The setup executable and `Drone Deck.exe` are unsigned (SmartScreen warns). | Nothing: the installer script and the release job were only read, and the last package folder and zip were built before 0.9. Build the installer once on a machine with Inno Setup 6.3+ and install, upgrade and uninstall by hand before publishing. See [Releasing](release.md). |
| VAL-12 | Diagnostics | The Job Object (`src/winjob.rs`) when Drone Deck is ended from Task Manager with QGroundControl running, and on Windows versions other than the Windows 10 development machine: children start suspended and are resumed with `NtResumeProcess`, an undocumented `ntdll` export. Diagnostics redaction (`src/diagnostics/redact.rs`) on a bundle from a real user profile. | Unit tests. Review an exported diagnostics zip before sharing it. |
| VAL-13 | Simulation | The check that nothing on Windows holds TCP 5760 before a simulator starts, with WSL localhost forwarding or a real MAVProxy. | Loopback listeners only. If the check misses a listener, the gate still refuses commands when the router reports no client. |
| ENV-02 | Simulation | QGroundControl connecting on its own to the vehicle link's fan-out to `127.0.0.1:14550` on Windows, without a hand-made comm link into WSL, and QGroundControl commanding a simulated vehicle back through the link. Drone Deck does not configure QGroundControl. | See [troubleshooting → ports](troubleshooting.md#ports). |

## Open

| ID | Area | Issue | Notes / workaround |
|---|---|---|---|
| ENV-03 | Setup | Changing the WSL distribution keeps every profile's old paths; there is no consolidated per-tool migration checklist. | Run Guided setup → Preparation (or Detect tools) for the new distribution. |
| CFG-02 | Readiness | **Detect tools** still runs each profile's installation probe (presence only), while Guided setup and Review installation run recipe discovery with compatibility checks. The two can disagree. | Prefer Preparation's result; a probe never claims compatibility. |
| UI-01 | Simulation | While a workspace launch is in progress, the flight-actions bar shows the general reason "No live link. Start the workspace of a simulated system first." instead of saying the launch is under way. | Wait for the launch stages to finish. |
| UI-02 | Physical vehicle | A physical system's scripts address can be a network address, and Settings → Systems does not warn about it. With the scripts consumer on, the computer at that address receives the vehicle's telemetry and can command the vehicle. | Keep the scripts address on `127.0.0.1`, or leave the consumer off. |
| UI-03 | Setup | The Guided setup preset for a physical vehicle is still named "Physical vehicle (read-only link)" although it installs QGroundControl, which bridges two-way when it is enabled for the system. | The template itself leaves QGroundControl and scripts off. |
| UI-04 | Physical vehicle | The last `.params` export is remembered for the whole app session, so after switching to another physical system **Compare later with this file** still offers the previous system's export. | Check the path before **Load and compare**. |
| UI-05 | Physical vehicle | **Ready to arm** reads the `SYS_STATUS` pre-arm bit and pre-arm `STATUSTEXT` messages only. PX4's event-based health report is not decoded, so a vehicle that sends neither shows **Not reported**. | Check readiness in QGroundControl. |
| UI-06 | Simulation | The console stays enabled for ArduPilot SITL, which has no MAVLink shell: lines are sent but never answered. The console says so. | Use a PX4 system for the console. |
| UI-07 | Simulation | A simulation link whose router has died keeps retrying, and the System page has no Disconnect for it. | **Stop session** closes it. |
| PERF-01 | App | Some work runs on the UI thread and can pause the window: the Windows TCP 5760 check (up to about 150 ms on a free port, two or three times per session start), **Export diagnostics** (each file read up to its last 512 KiB, about 20 files) and **Load and compare** (up to 8 MiB; a path on an offline network share can block until Windows gives up). | Keep compared `.params` files on a local disk. |
| PERF-02 | App | Two once-a-second redraws do not check which page is shown: the Pipelines job's seconds counter while a job runs, and the heartbeat age while a vehicle link is open. | None needed; without a job or a link the idle window draws nothing. |
| PIPE-01 | Data & models | **Live in Foxglove** opens the `foxglove://` link about 1.5 seconds after the profile reports Running, without checking that port 8765 answers. A replay, node or recorder that exits within milliseconds can be recorded in `run.json` with a null exit code (the run's status is still classified). Between a cancel and the agent creating the run folder, the status can briefly report nothing to wait for; the agent still honours the cancel. | Reload the runs list after a cancel. |

## By design (not defects)

- Cancelling an installation waits for the current package-manager or build step; it is never killed mid-change, and cancellation is not rollback. A step past its time budget or silent for five minutes shows **Needs attention**.
- A running process is never reported as connected or healthy; dashed connections stay unverified until traffic is observed. The link launch stage is ready when its TCP connection opens; the heartbeat is shown separately.
- Only one exclusive flight stack runs at a time; a PX4 started outside Drone Deck blocks a second PX4.
- Telemetry captures are bounded: each `.tlog` stops at 2 GB, and before a link opens the oldest captures in `logs\tlog\` are deleted so at most the newest 100 and 2 GB remain.
- A UDP-listen physical link stays with the source address of the first autopilot heartbeat until it goes down; a vehicle or radio bridge that changes its source port needs a **Disconnect** and **Connect**. The router in WSL likewise keeps its first UDP sender until it is restarted.
- A PX4 Takeoff waits until the vehicle reports its home altitude.
- Flight actions and the PX4 console reach only a simulation Drone Deck started itself; on any other link they stay disabled and show the gate's reason. Drone Deck sends no flight command, parameter write or shell line to a physical vehicle.
- QGroundControl or scripts enabled on a physical system are bridged two-way: what they send, commands and parameter writes included, reaches the vehicle. The connection panel counts those frames under **Sent by QGroundControl/scripts**. Leave both consumers off in Settings → Systems for a strictly read-only session.
- The window cannot refuse to close. Closing Drone Deck while a bridged physical link is open cuts QGroundControl and scripts off from the vehicle; a warning is written to Activity and `app.log`.
- A WSL process started without a working directory runs in the agent's own folder (`runtime\agent`); the shipped profiles change directory themselves.

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
| SIM-02 | jMAVSim opened but received no state. | 0.6 | Consequence of SIM-01; the staged launch now waits for relay Forwarding. (The relay and its firewall rule are gone: the router and vehicle link replace them.) |
| SIM-03 | PX4 flooded `pxh>` prompts (≈1.6 cores) and an unterminated line could grow without bound. | 0.6 | `scripts/agent/procs.py` keeps each child's stdin on a FIFO that never reaches EOF; `process_output::BoundedLines` caps allocation before the newline. |
| SIM-04 | The duplicate-PX4 guard became `test -z ""`. | 0.6 | Same fix as SIM-01. |
| RUN-01 | Workspace start launched SIH, relay and jMAVSim at once and called each Running on spawn. | 0.6 | `session.rs` staged launch with readiness, exit and timeout handling. |
| UX-01 | No workflow-level setup; users assembled each tool themselves. | 0.6 checklists, 0.8 | Guided setup: Intention → The system → Preparation → First run. |
| UX-02 | Two download systems: managed installation jobs and a raw "download package" action. | 0.9 | The legacy package download was removed; only reviewed installation jobs remain. |
| UX-03 | A verified install still needed a separate Apply step. | 0.6 | **Apply the verified launch configuration after success** (recorded so later edits are not overwritten). |
| UX-04 | Custom tools required JSON and Windows/Linux path knowledge. | 0.6, 0.8 | Plain profile form (one Linux command, or one Windows argument per line), location picker; JSON is the advanced route. |
| UX-05 | The installation review hid packages, destinations and privileges. | 0.6 | Review lists destination, privileges, packages/components and verification. |
| UX-06 | Installation was not coordinated with running tools. | 0.6 | Installs are refused while tools/recordings run; dependent launches wait for installs. |
| UX-07 | A long or stalled step gave no feedback. | 0.6 | **Needs attention** marker with the job log. |
| GAP-01 | No automatic recipes for jMAVSim, Gazebo or ArduPilot. | 0.7 | Maintained recipes (clean-machine validation still open: VAL-01). |
| GAP-02 | No Hugging Face, datasets, evaluation or physical-vehicle support. | 0.7 | Data & models, and the Physical vehicle system (acceptance still open: VAL-04, VAL-05). Datasets and CSV evaluation were removed again in 0.9; Data & models → Pipelines replaces them. |
| DOC-01 | The **Recording guide ↗** button opened a `RECORDINGS.md` path that no longer resolved. | 0.9 | The button was removed from `src/recordings/view.rs`; recordings are documented in the [user guide](user-guide.md#recordings-rosbag2--mcap). |
| LOG-01 | No application log or crash file was written. | 0.9 | `src/diagnostics/`: rolling `app.log`, `crash-<stamp>.txt`, the next-launch offer and Activity → **Export diagnostics**. |

# Troubleshooting

Start with **Activity → Process output** (the tool's own messages) and **Activity → Installations** (setup job logs). Known defects and unverified areas are in [known issues](known-issues.md). Everything else is in the [user guide](user-guide.md).

## WSL and the Linux environment

| Symptom | What to do |
|---|---|
| "WSL is not detected" | **Settings → Environment → Review WSL installation →**. Enabling the Windows feature can show a UAC prompt and usually needs a restart. Drone Deck never restarts Windows: restart yourself, reopen the app and click **Check again**. Virtualization disabled in firmware, organisation policy or a Windows edition without WSL 2 need manual action. |
| WSL installed but no WSL 2 distribution | Review a distribution installation from the live catalog (Ubuntu 24.04 is the tested baseline; others are Advanced). If the catalog cannot be fetched, the error is shown; connect and retry. |
| Linux tools fail with permission errors, or installs refuse to run | The distribution needs a regular Linux user. Use **Initialize Linux user ↗**, finish the prompts in the terminal it opens, then **Check again**. App-owned tools are not installed as root. |
| A WSL 1 distribution is listed | Drone Deck leaves it alone. Convert it yourself (`wsl --set-version <name> 2`) or install a WSL 2 distribution. |
| Launch buttons for Linux tools are disabled or say the environment is missing | The configured distribution (default `Ubuntu-24.04`) is not installed. Select an installed one with **Use this environment**. Native Windows tools (QGroundControl, the vehicle monitor) keep working without WSL. |
| Tools stopped being found after changing distribution | Each distribution has its own tools. Run **Detect tools**, or Guided setup → Preparation, for the new distribution and reuse or install what is missing. |
| "Stop active tools, installations, recording and replay before changing the WSL distribution." | Stop the session (rail foot), stop recordings/replay, and wait for or cancel installations first. |
| Automatic Linux recipe says it supports Ubuntu 24.04 only | Use detection of an existing installation or a manual profile on other distributions. |

## Simulation: router and vehicle link

The Local simulation chain is PX4 SIH (WSL) → MAVLink router (the Linux agent, UDP 14550 ⇄ TCP 5760 inside WSL) → Drone Deck's vehicle link on `tcp:127.0.0.1:5760` (WSL forwards localhost TCP to Windows) → QGroundControl on UDP 14550 and your scripts on UDP 14540. No firewall rule and no Java runtime are needed. ArduPilot SITL serves TCP 5760 itself, without the router.

- **The launch stops at router**: the Linux agent could not bind UDP 14550 or TCP 5760 inside WSL (another router, MAVProxy or ground station holds it), or the agent is not running (Activity → Process output, `[agent]` lines). The notice carries the agent's reason.
- **"Something on Windows already listens on TCP 5760"**: before a simulator or the router starts, Drone Deck checks that nothing on Windows (MAVProxy, mavlink-router, Mission Planner, a bridge to a real flight controller) holds TCP 5760, because its link would reach that program instead of the simulator. The check runs in the background. **Start workspace** begins the launch and stops at the first simulator or router stage with this reason. A simulator started from Tools & config or the runtime list first says "Checking that nothing on Windows listens on TCP 5760…" and starts on its own once the port is free; if you switch the active system or delete the system meanwhile, the start is dropped with a notice instead. Stop that program and start the session again. When the router stage is the one refused, the System connections router station and the flight-actions bar show this reason until **Stop session** or the next router start. If the link still reaches something other than the app's router (the router reports no client), flight commands stay disabled with that reason.
- **The launch waits at link, then times out**: Windows cannot reach `127.0.0.1:5760`. WSL localhost forwarding must be on (the default; `localhostForwarding=false` in `.wslconfig` turns it off). The timeout notice shows the link's last error.
- **The link is up but shows "no heartbeat"**: the router listens but PX4 sends nothing to UDP 14550 inside WSL; check PX4's output for its MAVLink instances. Live values grey out after 5 seconds without a heartbeat; they are never shown as live when stale.
- **"Workspace stopped at …: the … profile is missing"**: a profile the session uses (`sih`, `gazebo`, `ardupilot`, `dds`, `rosgz`, `qgc`) was deleted or renamed. Add it back or restore defaults by editing the registry. Upgrading from 0.8 removes the retired `relay`, `jmavsim`, `telemetry` and `hardware` profiles once, with a notice.
- **"… is running for <system>"**: one simulator runs at a time (they share UDP 14550 and TCP 5760), and a running simulator belongs to the system that started it. Stop that session first.
- **"Blocked" / "Another managed flight stack is running."**: only one exclusive flight stack (PX4 SIH, Gazebo, ArduPilot) runs at a time. Stop the other one first.
- **PX4 exits at once**: the PX4 profiles refuse to start while any `px4` process already runs in the distribution, including one started outside Drone Deck (a terminal, another launcher, an earlier crashed session). Stop it (`pkill -x px4` in that distribution) and start again.
- **Startup stops at SIH with a timeout**: the staged launch waits for SIH vehicle status. Read Process output for PX4 build or startup errors (a missing build is the usual cause on a new machine).
- **Flight actions or the console are disabled**: they work only on a simulation Drone Deck started for this system, and the bar says which condition is missing. Start the system's workspace from Drone Deck (a PX4 or router started elsewhere does not count), wait for the router and the link, and use the page of the system that owns the link. Physical vehicles never get these controls. See [Flight actions and PX4 console](user-guide.md#flight-actions-and-px4-console-simulation-only).
- **The flight-actions bar says "Workspace launch under way: starting … (stage … of …)"**: the launch has not reached its link stage yet. The controls unlock once the link is up and the simulator runs under this system.
- **The bar says "the vehicle link was closed while … still runs"**: the simulator runs for this system but you closed its link with **Disconnect link**. **Stop session**, then start the workspace again.
- **The bar says "… runs, but no vehicle link was opened for it"**: the simulator was started directly from Tools & config or the runtime list, which opens no link. Start the workspace to open the link.
- **"Another program sends MAVLink to UDP 14550 inside WSL … datagrams ignored"**: the router keeps the first UDP sender as its vehicle for as long as it runs and ignores every other sender; flight commands stay disabled while that happens. Stop the other flight stack or forwarder, then **Stop session** and start the workspace again. The same applies when PX4 restarts and sends from a new source port while the router keeps running (PX4 SITL normally sends from a fixed port; not observed here).
- **"Autopilots with system ids … share this link"**: more than one autopilot sends heartbeats on the link. Stop the other vehicle and restart the workspace.
- **The router stopped but the simulation link stays open and retries**: when no launch is running, the live view shows **Disconnect link**. It closes only the link; the simulator and other tools keep running. **Stop session** closes everything.
- **The console is disabled on an ArduPilot system**: ArduPilot SITL has no MAVLink shell, so Drone Deck sends nothing there. Use a PX4 system for the console.
- **A command shows "Timed out" or "Not sent / no vehicle heartbeat yet"**: the vehicle did not acknowledge, or no heartbeat has arrived on the link yet. Check the live view's heartbeat and PX4's output; QGroundControl may also be commanding the same simulated vehicle.

## Physical vehicle link

- **Connect fails on a serial port**: another program (often the ground station) already holds the COM port. Close it or give Drone Deck its own endpoint, for example a second telemetry radio or a UDP output of the ground station's forwarder. **Find serial devices** lists the ports Windows sees.
- **Connected, but the live view stays empty**: no autopilot heartbeat arrived. Check the endpoint, the baud rate and that the vehicle (not only a radio or a companion computer) is sending MAVLink.
- **A UDP-listen link went quiet after the vehicle or radio bridge restarted**: the link stays with the source address of the first autopilot heartbeat until it closes and ignores other senders. **Disconnect** and **Connect** again.
- **Fetch parameters or List logs never completes on a vehicle that uses MAVLink signing**: Drone Deck does not sign its requests (and does not check signatures on what it receives), so a vehicle that requires signing may ignore them. Read parameters and logs with QGroundControl or Mission Planner there.
- **The panel counts frames under "Sent by QGroundControl/scripts"**: QGroundControl or scripts are enabled for this system, so the link is a two-way bridge and what they send reaches the vehicle. For a strictly read-only session, turn both consumers off in **Settings → Systems** and connect again.
- **Ready to arm says "Not reported" or comes from SYS_STATUS on PX4**: PX4 v1.14 and later sends its health and arming check report only when a check result changes, and Drone Deck does not ask for it. Change the flight mode, or let QGroundControl ask for checks, and the report appears. PX4 before v1.14 and ArduPilot do not send this report.
- **"Events were missed since this report"**: an event from the vehicle was lost, so the report may be out of date. Drone Deck uses the `SYS_STATUS` pre-arm bit instead when the vehicle sends it. The next report from PX4 clears the warning.
- **An amber warning under the scripts address in Settings → Systems**: the physical system's scripts address is not on this computer, so the computer at that address receives the vehicle's telemetry and can command it. Use `127.0.0.1` unless you mean it.
- **A red reason under the scripts address, and Save refused**: the address is not an IP address and port. Host names (`localhost` included) are not resolved, so the link would forward nothing to them; 0.0.0.0 or `[::]` is a listen address, not a destination. Use an IP address such as `127.0.0.1:14540`. A system saved earlier with such an address keeps it until you edit and save it, and nothing is forwarded to it meanwhile.
- **Closing Drone Deck while a bridged link is open**: the window cannot ask first. QGroundControl and scripts lose the vehicle, and a warning is written to Activity and `app.log`.

## Ports

| Port | Used by | Notes |
|---|---|---|
| UDP 14540 | Your scripts (MAVSDK `udpin://0.0.0.0:14540`, pymavlink) on Windows | The vehicle link sends every packet to the system's scripts address (default `127.0.0.1:14540`) and forwards replies to the vehicle, on a physical system too, so a script can command a real vehicle through it. Only one listener can own the port. |
| UDP 14550 | QGroundControl (Windows); the router inside WSL | The vehicle link sends to `127.0.0.1:14550`, where QGroundControl should auto-connect without a configured comm link (not yet observed with a live PX4; see ENV-02 in [known issues](known-issues.md)). Inside WSL the router listens on 14550 for PX4. Close other ground stations that hold the Windows port. |
| TCP 5760 | The router inside WSL (or ArduPilot SITL) → the vehicle link | Reached from Windows as `127.0.0.1:5760` through WSL localhost forwarding. |
| UDP 8888 | Micro XRCE-DDS Agent (`MicroXRCEAgent udp4 -p 8888`) | The PX4 uXRCE-DDS client connects here. Agent 2.4.3 matches DDS-v2 firmware; the firmware client, Agent and `px4_msgs` versions must match. Installation alone does not create ROS topics. |
| UDP 14555 (example) | Physical vehicle link | Any free endpoint. Use a dedicated one so the link and QGroundControl do not compete; the link never forwards a physical vehicle to QGroundControl on the same UDP port it listens on. |

Find what holds a port: on Windows `Get-NetUDPEndpoint -LocalPort 14550 | Select-Object OwningProcess` then `Get-Process -Id <pid>`; inside WSL `ss -ulpn | grep 14540`.

## QGroundControl

- **QGroundControl closes right after starting, shows a graphics/OpenGL backend error, or opens a blank or black window**: use GPU-safe mode. In **Tools & config**, expand QGroundControl with **Configure** and tick **GPU-safe mode (software rendering)** under Graphics (stop QGroundControl first; a running process keeps its renderer). This adds `-swrast` to the profile's arguments, which you can also type on its own line in **Command / arguments**. Drone Deck then starts QGroundControl with Qt Quick's software renderer (`QT_QUICK_BACKEND=software`, `QT_OPENGL=software`) and clears inherited `QSG_RHI_*` hints, for that process only. 3D, shader and video features can be limited. Remove the line to return to normal GPU rendering. GPU-safe mode is no longer on by default; profiles saved by earlier versions keep `-swrast` until you remove it.
- **Starts in the wrong folder / missing resources**: leave **Windows working directory** blank; Drone Deck then uses the installation folder (the parent of `bin\`) like the installed shortcut does.
- **QGroundControl not found**: Detect it, or point the profile's **Program** at the installed `QGroundControl.exe`. A custom install folder may need manual detection after the managed installer finishes.

## Installations

| Status | Meaning |
|---|---|
| Needs input | Something only you can do: finish the visible QGroundControl installer, choose an installed executable, or answer a prompt. |
| Needs restart | Windows must restart (WSL feature). Restart, reopen Drone Deck, check again. |
| Needs attention | The current step ran past its time budget or produced no output for five minutes. The process is kept alive to protect package state; read the job log. Cancellation applies after the step. |
| Interrupted | No live worker matches the job after startup (the PC restarted or the worker was killed). **Review retry** rechecks completed work. |
| Failed | Read the log tail in Activity → Installations, fix the cause, **Review retry**. Retry keeps app-owned source folders and your edits; it does not promise byte-level resume. |

Only one installation runs at a time, across app instances. Installation is refused while tools, recordings or replay run; tools that depend on a running installation wait. Cancelling never uninstalls or rolls back packages.

## Recordings, models and projects

- **Recording finalized with zero messages**: nothing published on the selected topics in the capture domain. Check `ROS_DOMAIN_ID`, that publishers run in the same distribution, and that topic names are absolute. PX4 SIH alone publishes no ROS topics.
- **Replay is refused**: choose a finalized, non-empty session and a replay domain different from the capture and original domains.
- **Model download fails with 401/403**: the repository is gated or private. Accept access on huggingface.co, save a read token in Settings → Model access and **Validate access**. A token cannot grant access you were not given.
- **Live in Foxglove says it is waiting for port 8765**: `foxglove_bridge` runs but nothing on Windows accepts a connection on `127.0.0.1:8765` yet. After 20 seconds Drone Deck stops and does not open Foxglove. Read the `foxglove` profile's Process output, and check that WSL localhost forwarding is on. Another program already on port 8765 also passes the check, and Foxglove then shows that program.
- **Project will not run**: prepare its runtime first (Guided setup → Offline AI workspace), and check that the project's saved distribution matches the selected one.

## Where Drone Deck keeps things

| What | Location |
|---|---|
| Data root | `%LOCALAPPDATA%\DroneDeck\workspace\` |
| Launch profiles (`"schema": 2`) | `profiles.json` |
| Systems (`"schema": 2`: each system's name, template, source, consumers and prepared state, and the active one) | `systems.json` |
| Activity history / export | `activity.json` / `activity-export.json` |
| Zoom and UI preferences | `ui-preferences.json` |
| Exported process output | `session.log` (written by Activity → Process output → **Export log**) |
| Installation jobs | `setup\jobs\<job id>\` with `plan.json`, `state.json`, `output.log` (and `output.log.waiting` while a step needs attention) |
| Guided setup queue | `setup\workflow-state.json` |
| Model download jobs | `studio\jobs\<job id>\` |
| Vehicle link `.tlog` captures | `logs\tlog\` (`hardware\` holds captures and parameter backups from 0.8 and earlier) |
| Onboard logs downloaded from a vehicle | `logs\vehicle\` |
| Parameter exports | `exports\params\<system>-<stamp>.params` |
| Diagnostics (Activity → **Export diagnostics**) | `exports\drone-deck-diagnostics-<stamp>.zip` |
| Downloaded models | `%LOCALAPPDATA%\DroneDeck\models\` |
| Application logs (`app.log`, `app.1.log`, `app.2.log`) and crash reports (`crash-<stamp>.txt`) | `%LOCALAPPDATA%\DroneDeck\logs\`, or `logs\` in the data root when `DRONE_DECK_DATA_DIR` or portable mode moves it |
| Hugging Face token | Windows Credential Manager (generic credential) |
| Helper scripts | `runtime\` beside `Drone Deck.exe` (`drone-deck\scripts\` when run with `cargo run`) |
| Linux-side tools, recordings (with pipeline outputs in `extract/` and `inference/`), projects | `~/.local/share/drone-deck/{tools,recordings,projects}` in the selected distribution |

Overrides, checked in this order:

1. `DRONE_DECK_DATA_DIR`: set it to any folder and Drone Deck uses it as the data root (the folder that holds `profiles.json`).
2. **Portable mode**: an empty `portable.txt` beside `Drone Deck.exe` keeps the data root in `workspace\` beside the executable, the behaviour of versions up to 0.8.

**Migration from 0.8 and earlier.** On first start, if the data root has no `profiles.json` and `workspace\profiles.json` exists beside the executable (and portable mode is off), Drone Deck copies that folder into the data root, writes `MIGRATED-TO.txt` in the old folder naming the new location, and reports it in the status bar and Activity. The old folder is not deleted.

**Damaged files.** `profiles.json`, `systems.json`, `activity.json`, `ui-preferences.json` and `setup\workflow-state.json` are written to a temporary file and renamed into place; the previous version is kept as `<name>.bak` (for example `profiles.json.bak`). If one cannot be parsed at start, it is renamed to `<stem>.corrupt-<YYYYMMDD-HHMMSS>.json` (for example `profiles.corrupt-20261109-142500.json`), the `.bak` is tried, and the status bar and Activity say what happened. Nothing is reset silently. To recover by hand, close Drone Deck and copy a `.bak` or an older copy over the file.

## Other

- **Every button is disabled and the status bar says "Headless preview"**: the app was started with `--snap`, `--tree`, `--script` or a similar preview flag. Start it without arguments.
- **The UI is too large or too small**: **Ctrl+0** resets zoom to 100 %.
- **Drone Deck closed unexpectedly or hit an internal error**: the next start offers the crash report at the top of Activity. **Export diagnostics** there bundles it with the app log, profiles and recent job logs (redacted); review the zip before attaching it to a bug report.
- **Closing the app**: owned simulators, the vehicle link and MAVLink router, recordings (finalized gracefully) and projects stop; installation and model download jobs whose worker has started continue. Windows tools, the PowerShell helpers and the `wsl.exe` agent end with the app, also after a crash or a kill from Task Manager. A setup or model request still being prepared when the app closes may never start its job: wait until the job is listed.

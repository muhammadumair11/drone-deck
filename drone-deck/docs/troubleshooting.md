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

## Simulation: relay, firewall and visualizer

The Local simulation chain is PX4 SIH (WSL) → display relay (WSL) → jMAVSim (Windows). PX4 publishes vehicle state to `127.0.0.1:19410` inside WSL; the relay forwards each packet to the Windows host (the WSL default gateway, found at start with `ip route`) on UDP 19410, where jMAVSim listens in display-only mode.

- **jMAVSim shows "waiting for first message from: /0.0.0.0:19410"**: no state is reaching Windows. Check, in order: PX4 reached **Ready** (SIH status), the relay shows **Forwarding** (it only does after its first packet), and Windows Defender Firewall allows inbound UDP 19410 from the WSL virtual network. Drone Deck never creates or edits firewall rules. Add an inbound rule yourself, for example from an elevated PowerShell (narrow it further with `-RemoteAddress` or `-Program` if your policy requires):

  ```powershell
  New-NetFirewallRule -DisplayName "Drone Deck jMAVSim relay" -Direction Inbound -Protocol UDP -LocalPort 19410 -Action Allow
  ```

  The WSL address range can change after a restart; a rule bound to one remote address then stops matching. Relay **Forwarding** confirms UDP delivery was attempted, not that the firewall let it through or that jMAVSim renders.
- **The relay exits immediately**: it needs `python3` in the distribution and a free UDP 19410 inside WSL (another relay or simulator may be bound to it).
- **jMAVSim does not start**: it needs a native Java 21 runtime on Windows and its JAR. Expand jMAVSim in **Tools & config** and **Detect** it, or prepare it in Guided setup.
- **"Workspace needs the … profile. Configure it in Tools & config."**: a profile the session uses (`sih`, `relay`, `jmavsim`, `dds`) was deleted or renamed. Add it back or restore defaults by editing the registry.
- **"Blocked" / "Another managed flight stack is running."**: only one exclusive flight stack (PX4 SIH, Gazebo, ArduPilot) runs at a time. Stop the other one first.
- **PX4 exits at once**: the PX4 profiles refuse to start while any `px4` process already runs in the distribution, including one started outside Drone Deck (a terminal, another launcher, an earlier crashed session). Stop it (`pkill -x px4` in that distribution) and start again.
- **Startup stops at SIH or relay with a timeout**: the staged launch waits for SIH vehicle status, then a relay packet. Read Process output for PX4 build or startup errors (a missing build is the usual cause on a new machine).

## Ports

| Port | Used by | Notes |
|---|---|---|
| UDP 14540 | MAVSDK telemetry (`udpin://0.0.0.0:14540` inside WSL) | PX4's API/offboard link. Only one listener can own it: stop another MAVSDK script, MAVROS or a project before starting telemetry. The MAVSDK project starter uses a different endpoint for this reason. "No PX4 heartbeat within 30 seconds" means the simulator is not running or another listener has the port. |
| UDP 14550 | QGroundControl ↔ PX4 ground-station link | QGroundControl listens on 14550 by default. With PX4 inside WSL 2 it may not see the vehicle automatically. PX4's [WSL 2 guide](https://docs.px4.io/main/en/dev_setup/dev_env_windows_wsl.html) adds a UDP comm link in QGroundControl (Application Settings → Comm Links) to the WSL address (`ip addr | grep eth0` inside WSL) on port 18570; update it when WSL restarts with a new address. Close other ground stations that hold the port. |
| UDP 19410 | PX4 SIH → relay → jMAVSim | Needs the Windows firewall rule above. Bound on `127.0.0.1` inside WSL by the relay. |
| UDP 8888 | Micro XRCE-DDS Agent (`MicroXRCEAgent udp4 -p 8888`) | The PX4 uXRCE-DDS client connects here. Agent 2.4.3 matches DDS-v2 firmware; the firmware client, Agent and `px4_msgs` versions must match. Installation alone does not create ROS topics. |
| UDP 14555 (example) | Physical vehicle monitor | Any free endpoint. Use a dedicated one so the monitor and QGroundControl do not compete. |

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
- **Project will not run**: prepare its runtime first (Guided setup → Offline AI workspace), and check that the project's saved distribution matches the selected one.

## Where Drone Deck keeps things

| What | Location |
|---|---|
| Data root | `%LOCALAPPDATA%\DroneDeck\workspace\` |
| Launch profiles (`"schema": 1`) | `profiles.json` |
| Systems (names, prepared) | `systems.json` |
| Activity history / export | `activity.json` / `activity-export.json` |
| Zoom and UI preferences | `ui-preferences.json` |
| Exported process output | `session.log` (written by Activity → Process output → **Export log**) |
| Installation jobs | `setup\jobs\<job id>\` with `plan.json`, `state.json`, `output.log` (and `output.log.waiting` while a step needs attention) |
| Guided setup queue | `setup\workflow-state.json` |
| Model download jobs | `studio\jobs\<job id>\` |
| Vehicle `.tlog` captures and parameter backups | `hardware\` |
| Downloaded models | `%LOCALAPPDATA%\DroneDeck\models\` |
| Application logs (reserved for a later release) | `%LOCALAPPDATA%\DroneDeck\logs\` |
| Hugging Face token | Windows Credential Manager (generic credential) |
| Helper scripts | `runtime\` beside `Drone Deck.exe` (`drone-deck\scripts\` when run with `cargo run`) |
| Linux-side tools, recordings, projects, datasets, evaluations | `~/.local/share/drone-deck/{tools,recordings,projects,datasets,evaluations}` in the selected distribution |

Overrides, checked in this order:

1. `DRONE_DECK_DATA_DIR`: set it to any folder and Drone Deck uses it as the data root (the folder that holds `profiles.json`).
2. **Portable mode**: an empty `portable.txt` beside `Drone Deck.exe` keeps the data root in `workspace\` beside the executable, the behaviour of versions up to 0.8.

**Migration from 0.8 and earlier.** On first start, if the data root has no `profiles.json` and `workspace\profiles.json` exists beside the executable (and portable mode is off), Drone Deck copies that folder into the data root, writes `MIGRATED-TO.txt` in the old folder naming the new location, and reports it in the status bar and Activity. The old folder is not deleted.

**Damaged files.** `profiles.json`, `systems.json`, `activity.json`, `ui-preferences.json` and `setup\workflow-state.json` are written to a temporary file and renamed into place; the previous version is kept as `<name>.bak` (for example `profiles.json.bak`). If one cannot be parsed at start, it is renamed to `<stem>.corrupt-<YYYYMMDD-HHMMSS>.json` (for example `profiles.corrupt-20261109-142500.json`), the `.bak` is tried, and the status bar and Activity say what happened. Nothing is reset silently. To recover by hand, close Drone Deck and copy a `.bak` or an older copy over the file.

## Other

- **Every button is disabled and the status bar says "Headless preview"**: the app was started with `--snap`, `--tree`, `--script` or a similar preview flag. Start it without arguments.
- **The UI is too large or too small**: **Ctrl+0** resets zoom to 100 %.
- **Closing the app**: owned simulators, telemetry, recordings (finalized gracefully) and projects stop; installation jobs continue.

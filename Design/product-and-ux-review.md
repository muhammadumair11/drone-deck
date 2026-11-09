# Drone Deck — product understanding and UX redesign

## Scope and evidence
This is a source-informed redesign of Drone Deck 0.7, not a replacement of its Rust implementation. Read-only evidence: drone-deck/README.md, STUDIO-0.7.md, SETUP.md, RECORDINGS.md, src/backend.rs, src/session.rs, src/setup/workflow.rs, src/setup/profiles.rs, and the existing Design/index.html and app.js. New work stays in this workspace's Design directory; the linked source folder remains untouched. Runtime behavior was not exercised against a drone or installer. Older guides conflict with 0.7 on available recipes; use 0.7 where they differ.

## What the product actually does
Drone Deck is a Windows-native Rune/Rust application that coordinates external tools. Linux tools run inside a selected WSL distribution; Windows hosts jMAVSim and QGroundControl. The app is a workflow orchestrator, installer, process supervisor, log viewer, recording/data workspace and read-only vehicle monitor. It is not an embedded simulator or universal autonomous-flight system.

### Core layers
1. Environment: Windows prerequisites, WSL 2, Ubuntu 24.04, regular Linux user, native Java when needed. Changing an environment during active managed work is blocked.
2. Installation: discovery checks actual locations and compatibility. Compatible tools can be reused; missing tools get reviewed recipes. Installation never starts from a scan. Jobs persist through app closure, stop at safe boundaries, and pause for input/restart/failure.
3. Configuration: profiles bind executable, command, working directory, probe and WSL environment. Adoption is separate from installation. A saved profile does not prove connectivity.
4. Session: SIH starts first, then relay after readiness, then jMAVSim after forwarding. Launch failure blocks later stages; timeout is 60 seconds. QGroundControl and MAVSDK are separate tool launches. Process running is not vehicle readiness.
5. Data: read-only MAVSDK telemetry, ROS recording and isolated replay, models, datasets, editable projects and CSV evaluation.

### Connection model
- PX4 SIH contains flight physics. Its state reaches a WSL relay and then native jMAVSim via UDP 19410. jMAVSim is display-only in this workflow.
- MAVSDK consumes vehicle telemetry on UDP 14540; competing listeners must be avoided. Stale telemetry after five seconds must be surfaced.
- QGroundControl opens a separate native window for vehicle setup, mission operations and flight control. Do not put arm/land controls in this redesign.
- PX4 to ROS requires a compatible firmware DDS client, Micro XRCE-DDS Agent (configured UDP 8888), matching px4_msgs and live publishers. Installed packages are insufficient.
- ROS publishers to rosbag2/MCAP to finalized recordings to dataset versions to supplied predictions/evaluation. Datasets reference original artifacts, so removal breaks verification.
- Replay defaults to separate ROS domain 77 with local-only discovery. Replay is distinct from live capture and does not imply a live vehicle connection.
- Model download verifies assets; inference still requires a compatible runtime, inputs/preprocessing and task adapter. Model availability is not autonomy readiness.
- Physical monitoring uses Windows serial/UDP/TCP with separate state, tlogs and read-only parameter requests. It is not direct physical flight control.

## UX diagnosis
The existing design organizes around implementation categories (simulators, development, integrations), making new users assemble a workflow mentally. It presents multiple competing start points. Inspector configuration is disconnected from the intended outcome. Its demo copy includes unsupported mission monitoring and invented preflight counts. A single ready/running label collapses installation, configuration, process liveness and real data into a misleading signal.

## Current direction: persistent UAV system management

The user accepted guided preparation as a strong first-use experience but rejected making it the whole product or calling it an experiment. The current design combines a durable system workspace with an optional guided preparation route. The organizing object is a system/project: its selected execution context, tool configuration, connections, running processes, data and history.

### Information architecture
- `drone-workspace.html`: daily system overview, planned topology, configuration/readiness distinctions and direct session/process actions.
- `system-tools.html`: system-scoped catalog filtering, process actions and expandable executable/endpoint/command editing. Configuration is stored per system in preview storage.
- `system-data.html`: recording prerequisites/sample library, model repository format validation, and honest empty run outputs. Different data types remain separate tabs.
- `system-activity.html`: locally persisted, timestamped sample action history and JSON export.
- `system-setup.html`: the retained guided first-use route: choose purpose, understand system composition, name the project, check/review preparation, then hand off to the management workspace.

Each screen is self-contained with stable links. The primary entry remains `drone-workspace.html`. Previous versions are snapshots, not entry points.

### First use versus the hundredth use
First use: see an unconfigured system and an obvious guided preparation entry. No unsupported ready states, telemetry or installed-tool claims are invented. Preparation explains roles in context and asks for explicit review before sample changes. The prepared configuration becomes a locally saved system context and returns the user to management.

Repeated use: land in the system view with the saved name/context. Start/stop individual sample processes or the workspace directly, inspect tool configuration, return to local data or read activity. The user does not repeat onboarding. Guided setup remains in navigation for adding/revising a system. Technical controls are available in place, rather than hidden behind an expertise mode.

### Independent system contexts
Local simulation: PX4 SIH -> display relay -> native jMAVSim. QGroundControl and read-only MAVSDK are parallel consumers of PX4, not downstream of the visualizer.

ROS and sensor capture: PX4 -> compatible DDS Agent -> ROS publishers/message workspace -> MCAP recording. Installation is not live topic discovery. Versions and messages must match.

Offline AI: runtime -> editable inference project -> retained artifacts. No model is bundled. Model download, runtime, task adapter and prepared inputs are distinct requirements.

Physical vehicle: dedicated native endpoint -> Windows monitor -> local tlog. QGroundControl is an independent external ground station. No simulator or WSL is required by native monitoring. Vehicle identity and heartbeat freshness still need actual observation.

### Technical depth and boundaries
Profiles expose execution host, command/endpoint and an optional absolute path. Save validates basic format; adoption still needs compatibility checks in production. Editing a running sample process is blocked. System switching is blocked while sample processes are active. Session state survives internal page navigation in session storage and is always explicitly labeled as sample; it is never evidence of a real running process.

The main topology remains a schematic, not a live ROS/network graph. Dashed connections are unverified. Running, installed, configured, connected and healthy must stay distinct. QGroundControl remains the flight-control interface. Stop session stops owned processes and is not a vehicle command.

### Prototype behavior
Four management screens and one setup screen support navigation, saved system selection/preparation, sample session and individual process controls, system-scoped profile edits, tool-category filters, data tabs, repository-ID validation, sample recording entries, timestamped activity and export. All operations are local UI simulations. No command executes, no package/model downloads, no vehicle connects, and no MCAP artifact is created. Recording examples explicitly say no real file exists. No result metrics are invented.

The preparation flow's optional first-run chapter remains navigable after preparation, but its principal completion action returns to the management workspace. Existing compatible installation adoption, environment bootstrap, persistent installer job recovery, real model/download/recording adapters and live telemetry are production integrations, not implemented by these HTML files.

### Production integration
Use the native backend's environment-scoped discovery, reviewed batch setup and profile adapters. Bind process controls to owned process IDs, readiness to observed protocol events, activity to real output with provenance, and data pages to actual model/recording/project stores. Map run failure and stale data to the affected component. Configuration compatibility needs actual checks beyond format validation. Installing optional runtimes requires explicit action.

A durable multi-project manifest and fully persisted system launch graph are proposed UX abstractions. The existing 0.7 backend has project manifests and managed profiles but does not automatically provide every proposed cross-module relationship. Production must reconcile process state on launch; restoring a browser sample state is not a production readiness model.

### Visual contract
The active SpaceX palette and type stacks stay bound. A compact management rail and an open operational topology carry the main hierarchy. Tool details expand in their own rows; the rejected side explanation inspector does not return. Guided setup retains the focused page structure. No metric card wall, decoration or photographic backdrop competes with technical work. D-DIN uses the specified fallback because no local font was available.

### Validation and next steps
Verify both directions of the setup handoff, persist preferences across navigation, block context changes and profile edits during active processes, and keep empty/data/error states honest. Test keyboard focus, narrow layouts, source/endpoint validation, and safe text rendering. The next native acceptance pass must cover actual WSL prerequisites, installer input/restart/failure, stale telemetry, occupied ports and real vehicle identity. User testing should compare time-to-first-result and repeated-use access to process/configuration tasks; no usability improvement is claimed without that evidence.

## Revision history
- Initial workflow-map design: rejected as too close to the existing tool-led UI.
- Guided notebook: helpful for first use, rejected as the sole application experience and terminology.
- Current: guided system preparation plus persistent management, configuration, data and activity screens. Previous current HTML and rationale saved as -v3 snapshots; older -v2 snapshots are preserved.

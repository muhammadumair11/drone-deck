# Drone Deck platform plan

Agreed scope: 9 November 2026, revised to Windows-only desktop support and Hugging Face integration. This document records the agreed scope. Version 0.7 implemented the first local accounts/model, dataset/evaluation, project and physical-monitoring capabilities, 0.8 reorganised them around an active system, and 0.9 (unreleased) replaced the relay-based simulation with a native MAVLink link and datasets/evaluation with pipelines; current behaviour and limits are in the [user guide](docs/user-guide.md) and [known issues](docs/known-issues.md). Broader perception, remote deployment and direct physical flight remain future work.

## Confirmed requirements

- Build and distribute Drone Deck only as a Windows desktop application. Linux tools run in WSL 2; a standalone Linux desktop application and native Ubuntu packaging are out of scope. Use Ubuntu 24.04 LTS as the initial tested WSL tool environment; additional distributions need their own compatibility status. Future remote Linux runtimes remain execution targets, not desktop-client platforms.
- After an explicit Install action, download, install, configure, and verify supported tools within a visible workflow. Show required administrator, Linux authentication, and reboot steps.
- Detect existing installations, offer to reuse them, verify compatibility, and ask before changing or updating them.
- Begin physical-drone support with monitoring, logs, parameter setup, and opening QGroundControl or Mission Planner. Direct flight commands are a later, separately tested capability.
- Implement complete multirotor workflows first. Keep vehicle descriptions extensible to fixed-wing, VTOL, and multiple vehicles.
- Model local and remote execution from the start; implement local setup first. Remote workstations and onboard Linux computers are subsequent execution backends.
- Preserve existing user installations, project files and profiles. The PX4 SIH arrangement is SIH → MAVLink router (the WSL agent) → Drone Deck's native vehicle link; the earlier relay and jMAVSim were retired in 0.9.
- Add Hugging Face access-token management in Settings and managed model downloads that can be attached to compatible AI workflows. Treat downloaded models, their required runtimes, and workflow adapters as distinct components.
- Include dataset management, offline operation, and evaluation against recorded sensor data as accepted product requirements. Recording tools and formats must support the reproducible AI workflow; the final adapter/version choices remain subject to implementation validation.

## Current delivery and remaining gaps

The current app manages processes (Linux ones through a WSL agent), profiles, logs, documentation links, user-created systems, a native MAVLink vehicle link with a live view, rosbag2/MCAP recording/replay, frame-extraction and inference pipelines and flight-log summaries. Flight commands and the PX4 console are sent only to simulations Drone Deck started; physical vehicles are read-only from Drone Deck, although QGroundControl or scripts enabled for a physical system are bridged two-way. Version 0.5 adds Windows/WSL discovery, live distribution selection, maintained setup recipe implementations, persisted installation jobs/logs/recovery and environment-bound existing-tool adoption. Fresh defaults use user-relative managed Linux locations; saved profiles are preserved.

An existence probe is not a compatibility or connection test. A command profile is not a complete integration adapter. A downloaded installer is not an installed and configured tool. Setup recipes have persistent lifecycle management; the legacy configured-package download was removed in 0.9, so reviewed installation jobs are the only download route for tools. Existing-tool discovery and non-installing lifecycle fixtures were validated, while fresh WSL/UAC/reboot and clean source-build validation remain outstanding. See [managed installation](docs/user-guide.md#managed-installation). Version 0.7 adds Windows Credential Manager-backed Hugging Face settings, pinned/hash-verified model jobs, immutable dataset versions and reports, optional CPU runtime recipes/project starters and native read-only physical monitoring. Version 0.9 (unreleased) adds parameter export/compare and onboard log download for physical vehicles, crash reports and diagnostics export, and a per-user installer; none of it has run against real WSL, PX4, Gazebo, ROS 2 or vehicle hardware yet, and the installer has never been built. Full task-specific compatibility, hardware acceptance and clean-machine installer validation remain outstanding.

## Environment setup

The application must open with no Linux environment installed. On Windows, inspect Windows version, architecture, WSL availability/version, installed distributions, virtualization readiness where observable, free disk space, and graphics capabilities. Show uncertainty when a check cannot distinguish an unavailable capability from a permission problem.

List available distributions using Microsoft's WSL catalog, retaining their actual identifiers. Mark distributions Recommended, Supported, or Advanced/untested for the selected workflow. An offline catalog must show its age and cannot imply a live availability check.

Offer Use existing and Install new. WSL installation should use supported Microsoft commands, request elevation only for the relevant step, preserve progress across a required reboot, and return to distribution initialization. Linux username/password initialization may require interaction; do not promise a universally unattended installation. BIOS/firmware changes need user instructions.

Require a configured WSL Linux environment for dependent workflows; keep native Windows ground-station and hardware features accessible independently. Avoid requiring local PX4 simulation just to monitor a physical flight controller. No separate native Ubuntu application is planned.

References: [WSL installation](https://learn.microsoft.com/en-us/windows/wsl/install), [WSL commands and distribution discovery](https://learn.microsoft.com/en-us/windows/wsl/basic-commands).

## Tool catalog and installation manager

Every supported tool should offer these paths:

1. Find and reuse an existing installation, with a manual path selector when discovery fails.
2. Install automatically through a maintained recipe.
3. Open the official download/setup source, then detect and configure the result.

Each recipe must declare its source, pinned or deliberately selected version, operating system/architecture support, dependencies, destination, required privileges, verification method, configuration steps, and readiness checks. Show the intended changes and known size estimates before starting. Distinguish missing estimates from measured download sizes.

Use official package managers, signed vendor installers, or official source repositories as appropriate. A PX4 source checkout/build and a ROS package installation are different recipes from downloading a Windows EXE. Validate publisher signatures or published hashes where supplied; show when publisher verification is unavailable.

Persist jobs with states such as Planned, Downloading, Verifying, Installing, Configuring, Checking, Ready, Needs input, Needs restart, Failed, and Cancelled. Include progress, logs, retry, and cancellation where the underlying operation supports it. Report indeterminate progress honestly. Interrupted installation recovery must recheck completed steps; it cannot assume every installer or build supports byte-level resume.

Own and track installation subprocesses. Closing the app must not silently lose running installation jobs. Avoid terminating package managers midway through changes; report when an operation must finish its current step. Never claim that cancellation automatically rolls back system packages.

Record which assets Drone Deck owns. Repair/uninstall must respect shared dependencies and adopted installations. Do not automatically update flight stacks or alter adopted tools. A failed setup should present the failing step, logs, and a supported recovery action.

## Compatibility and execution model

Represent execution targets as Windows local, WSL distribution, and future remote Linux. Drone Deck's desktop UI always runs on Windows. Bind each component to a target, version, path, environment, and endpoint. Separate the flight stack, simulator, visualizer, communications bridge, ground station, development environment, and AI workload.

Replace personal default paths with discovered or chosen locations. Use appropriate per-user writable Windows application data and explicit WSL workspace/cache locations; migrate the existing portable workspace without losing profiles (done in 0.9: `%LOCALAPPDATA%\DroneDeck\workspace`, with `portable.txt` and `DRONE_DECK_DATA_DIR` overrides). Keep WSL and Windows Python environments distinct and label which interpreter runs each project.

Offer tested workflow presets rather than installing every catalog entry. Initial presets should cover the existing lightweight PX4 SIH/visualizer workflow, PX4 development with MAVSDK, PX4/ROS 2, and an independently validated ArduPilot workflow. Add Gazebo as an optional richer simulation workflow.

Maintain a compatibility matrix for autopilot, OS, ROS distribution, middleware, simulator, message definitions, and architecture. PX4's current main-branch guide recommends Ubuntu 24.04 with ROS 2 Jazzy; each supported PX4 release still needs its own checked recipe. ArduPilot needs its own tested ROS recipe rather than inheriting PX4's assumptions.

Readiness must distinguish Installed, Configured, Compatible, Process running, Connected, and Data healthy. Validate real protocol responses and data freshness. Diagnose endpoint ownership and port collisions; support telemetry routing so multiple tools can receive appropriate streams.

References: [PX4 ROS 2 guide](https://docs.px4.io/main/en/ros2/user_guide), [ArduPilot ROS 2](https://ardupilot.org/dev/docs/ros2.html), [MAVSDK compatibility limitations](https://mavsdk.mavlink.io/main/en/faq.html).

## Simulation, physical vehicles, and recorded data

The active system (Local simulation, ROS & sensor capture, Offline AI workspace, Physical vehicle) selects the simulated, physical or recorded-data context explicitly. Each connection has an identity and execution target; a simulator process must not stand in for identification of a physical vehicle.

Physical workflows need serial/USB and UDP/TCP connection discovery/configuration, autopilot and vehicle identification, reconnect status, telemetry freshness, firmware information, parameter backup/diff/apply results, and flight-log retrieval. Parameter writes must identify the target vehicle and report acknowledgements. Initial integration retains QGroundControl and, for ArduPilot on Windows, Mission Planner for their established workflows.

Windows hardware access and WSL hardware access are different paths. Prefer native Windows access where appropriate. Offer USB forwarding to WSL only when a Linux tool needs the device; Microsoft's supported approach uses usbipd-win and requires its own setup. Diagnose WSL networking, discovery, and firewall reachability without silently changing system rules.

Keep process stop distinct from vehicle land/disarm. Stopping a UI or a telemetry process must not silently issue flight commands. Firmware flashing and direct flight control are future capabilities with separate explicit user flows.

References: [USB devices in WSL](https://learn.microsoft.com/en-us/windows/wsl/connect-usb), [WSL networking](https://learn.microsoft.com/en-us/windows/wsl/networking), [QGroundControl](https://docs.qgroundcontrol.com/master/en/qgc-user-guide/index.html), [Mission Planner](https://ardupilot.org/planner/docs/mission-planner-overview.html).

## Development, AI, and reproducibility

Provide project templates and managed launch graphs for MAVSDK, ROS 2, and custom code. Show startup dependencies and readiness conditions, interpreter/environment selection, per-component logs, and collected artifacts. Preserve a version/configuration manifest for each run.

Treat AI as a workflow covering sensor acquisition, synchronized recordings, datasets, model training, evaluation, model versions, inference, and eventual deployment. Track inference latency and dropped/stale data alongside task results. Support recorded-data evaluation and simulation before physical deployment. Keep training and inference engines replaceable.

An onboard companion computer is distinct from the Windows PC and from the flight controller. Later remote execution should deploy and manage Linux services on such computers without requiring the Windows UI to remain the runtime for onboard autonomy.

Useful integration candidates include MAVSDK, pymavlink, MAVROS or autopilot-specific ROS bridges; rosbag2 recording/replay; Foxglove visualization; PyTorch training (a starter shipped in 0.7 and was removed in 0.9); and ONNX Runtime inference. These have different responsibilities and are not interchangeable substitutes.

References: [MAVROS](https://github.com/mavlink/mavros/tree/ros2), [pymavlink](https://mavlink.io/en/mavgen_python/), [rosbag2](https://github.com/ros2/rosbag2), [Foxglove](https://docs.foxglove.dev/docs), [PyTorch](https://docs.pytorch.org/tutorials/beginner/basics/intro.html), [ONNX Runtime](https://onnxruntime.ai/docs/).

## Recording, datasets, offline operation, and evaluation

Accepted scope includes a recording/session library, dataset management, local offline workflows, and AI evaluation against recorded sensor data. The v0.4 implementation adds rosbag2/MCAP dependency checks, explicit topic discovery/selection, managed recording/finalization, session metadata/library browsing, and separate-domain local replay. Version 0.7 added dataset versions and CSV evaluation; 0.9 (unreleased) replaced them with pipelines (frame extraction and inference on an isolated replay) and opens recordings in Foxglove. Offline project readiness and further visualization adapters remain planned. See [Recordings](docs/user-guide.md#recordings-rosbag2--mcap) for prerequisites and limits.

Recommended recording foundation:

- **rosbag2:** the ROS 2 recorder/player, running in the project's selected WSL environment. Allow topic selection, record/stop, supported pause/resume, file splitting, recording profiles, and playback options matched to the installed ROS release. Do not assume every feature in rolling ROS documentation exists in the selected stable distribution.
- **MCAP:** preferred interoperable container for supported sensor/message recordings, including schema metadata, timestamps, optional indexing, and compression. rosbag2 can write MCAP using its storage plugin. Native MCAP libraries also allow custom recordings without requiring every producer to use ROS 2.
- **Rerun:** a candidate SDK/viewer for rich visual debugging of camera imagery, point clouds, transforms, trajectories, and model outputs across time. Its RRD recordings and task-specific visualization adapters complement the raw recording layer. This is a separate project from the Rune desktop UI framework. Validate supported MCAP ingestion/schema mappings rather than assuming arbitrary formats render automatically.
- **Foxglove:** a complementary viewer for supported ROS/MCAP data, with local-file workflows. Product edition, licensing, installation, and offline availability need verification for the chosen integration.
- **Flight-controller logs:** retain original PX4 ULog and ArduPilot onboard/telemetry logs as separate source artifacts. Use format-specific parsers/import adapters; do not equate them with full camera/LiDAR recordings or silently replace the originals with lossy exports.

Each recording session should carry vehicle identity, simulation/hardware source, software versions, captured topics/streams, message schemas, sensor calibration and coordinate-frame metadata, timestamp units, clock domains, and known clock offsets. Preserve source acquisition times and capture/receipt times when available. Recording timestamped messages does not itself synchronize independent sensor clocks. Monitor rates, message/frame loss, write throughput, recording errors, and free disk space. Show missing streams and uncertainty explicitly.

Dataset management should retain immutable raw recording references and produce versioned derived datasets with selected time ranges, labels/annotations, transformations, and train/validation/test splits. Track provenance back to recording/vehicle/session and avoid leakage from adjacent frames of the same flight into evaluation sets. Local model outputs and Hugging Face datasets should be importable through explicit adapters without making every dataset format a built-in promise.

Offline readiness means that the selected project's packages, model weights, tokenizers/processors, message schemas, calibration, and required assets are available locally. Provide a readiness check and list missing dependencies. Local recording, browsing, playback, inference, and evaluation should work without an account/network once their dependencies are present. Download/catalog refresh and cloud services remain network-dependent. Never infer that an absent network prevents reading already available local data.

Evaluation should select a recording/dataset revision, model revision, adapter/configuration, and execution target; run the same input through one or more model versions; and store task-appropriate quality metrics, inference latency, resource usage, sample coverage, and failures. Accuracy metrics require suitable reference labels/ground truth; absent labels should yield diagnostic/consistency results rather than invented accuracy. Preserve outputs and manifests so a result can be reproduced. Offline replay should be isolated from live vehicle command endpoints; it is a recorded-data evaluation workflow, not a guarantee that a physical flight would be reproduced.

The initial recommendation is **rosbag2 + MCAP for ROS sensor recording**, original autopilot logs for flight-controller diagnostics, and an optional Rerun or Foxglove visualization adapter. rosbag2/MCAP recording/replay and the first dataset/report layer are implemented; optional Rerun/Foxglove adapters remain planned. This does not identify the previously remembered library.

References: [rosbag2 for Jazzy](https://github.com/ros2/rosbag2/tree/jazzy), [MCAP format and libraries](https://mcap.dev/), [Rerun SDK and recordings](https://rerun.io/docs/concepts/how-does-rerun-work), [Foxglove ROS 2/local recordings](https://docs.foxglove.dev/docs/getting-started/frameworks/ros2), [PX4 ULog parser](https://github.com/PX4/pyulog), [ArduPilot log types](https://ardupilot.org/dev/docs/common-logs.html).

## Hugging Face accounts and model library

Settings → Model access (Hugging Face), also reachable from Data & models → Models, should offer a masked token input, Validate connection, account identity/status, Replace token, and Remove saved token. Removing the saved token must explain that it does not revoke it on Hugging Face. Link to the official token-management page for creation/revocation. Use a dedicated fine-grained token with read access to the selected repositories where possible; downloading models does not require repository write permission.

Store the credential through Windows Credential Manager. Keep only a credential reference in ordinary configuration. Exclude tokens from profiles, command arguments, logs, crash reports, manifests, project exports, and screenshots of settings. Retrieve credentials only when needed, scope them to the intended authenticated download process/request, and avoid persistent plaintext Hugging Face login files or copying secrets into WSL homes. Redact authenticated request details from diagnostics. A user must explicitly choose any future credential provisioning to a remote runtime.

The Data & models → Models tab should grow into a model library that accepts a Hub repository ID or URL and supports search, selected revision, file/variant selection, model cards, license information, gated-access status, download-size estimates, and destination selection. Use the official Hub client/API with a pinned, tested version. Download only the selected model files and required metadata rather than every weight format in a repository.

Integrate downloads with the persisted job system: progress, retry/cancellation, supported interrupted-download recovery, cached-file reuse, revision/hash metadata, and actionable authentication or disk-space failures. Allow import of an existing local model. Show Windows and WSL cache locations clearly, avoiding accidental duplicate multi-gigabyte copies and respecting each environment's performance needs. Cached models should be usable offline when their runtime and other dependencies are also available; explain missing assets explicitly. Cache cleanup must show which workflows reference the files.

A token authenticates access already granted to the user; it does not grant access to every gated repository. When required, open the model's official access/license page for the user to complete the browser flow, then recheck access. Do not automatically accept model terms on the user's behalf. Support anonymous downloads when the repository allows them.

Each library entry must distinguish Downloaded, Runtime missing, Compatible, and Validated in workflow. Record repository ID, resolved commit, selected files/format, tokenizer or processor where applicable, runtime/version, license metadata, and evaluation results. Pin workflow manifests to a revision for reproducibility; updates should be explicit. Check RAM/VRAM and runtime/device support before offering to run a model, and validate inference with a small appropriate input before declaring it ready.

The catalog should cover drone-relevant tasks such as object detection, segmentation, depth estimation, and vision-language work rather than assuming every model is a chatbot. A downloaded model requires a task-specific adapter to consume sensors and publish usable outputs. Disable custom repository-code execution by default; explain and require an explicit choice for runtimes that need it. Prefer supported non-executable weight formats such as safetensors when available, without treating file format as a guarantee of model quality or correctness.

Extend the same library design to datasets and locally trained model artifacts later. Keep model access/downloads separate from optional hosted inference: cloud inference may incur charges and transmit data, so it must be explicitly enabled with visible provider/cost/data handling. Model downloads alone must not enable it. Local recorded-data evaluation remains the default path before integration with a live drone workflow.

References: [Hugging Face token permissions](https://huggingface.co/docs/hub/security-tokens), [Hub downloads and revisions](https://huggingface.co/docs/huggingface_hub/guides/download), [Gated-model access](https://huggingface.co/docs/hub/models-gated), [Cache management](https://huggingface.co/docs/huggingface_hub/guides/manage-cache), [Windows credential management](https://learn.microsoft.com/en-us/windows/win32/secauthn/credentials-management).

## Delivery order and acceptance

1. Windows/WSL environment discovery, first-run setup, migration of current profiles, and installation job infrastructure. Validate a Windows machine with no WSL, an existing WSL installation, multiple distributions, unavailable network, and a reboot/input-required setup state. Introduce the Windows credential-store abstraction for authenticated downloads.
2. Complete maintained recipes for the core PX4/visualizer, MAVSDK, ROS bridge, and ground-station workflows, followed by the independently tested ArduPilot recipe. Confirm that a newly created supported environment can install and connect without hand-editing personal paths.
3. Physical-vehicle monitoring, parameter setup and logs; explicit vehicle identity and protocol-level checks. Validate with actual supported hardware before claiming physical integration complete.
4. Hugging Face account settings/model library, rosbag2/MCAP recording and replay, dataset management, offline readiness, and the first end-to-end recorded-data AI evaluation workflow; then remote execution/deployment and additional vehicle types. Validate invalid/revoked credentials, gated-access denial, token redaction, cache reuse, interrupted downloads, pinned revisions, missing runtimes, offline operation, missing sensor streams, timestamp/clock metadata, and reproducible evaluation results.

Check each advertised core workflow on Windows with its declared Windows-local or WSL execution target. Native Ubuntu desktop builds are outside the support matrix. Headless UI and simulated installer tests are useful, but do not establish that a clean OS installation or real hardware integration works. Maintain a tested support matrix and clearly label untested/manual-only catalog entries.

The goal is dependable end-to-end support for declared workflows, with an adapter system for extensions. A universal promise that every simulator, autopilot, or AI package works automatically would be inaccurate.

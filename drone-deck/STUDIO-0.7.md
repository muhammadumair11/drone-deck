# Drone Deck 0.7 — setup, AI and physical monitoring

This release adds working local capabilities to the Windows app. It does not install any new tools merely by opening the app, scanning, creating a project or loading a library. Installation recipes are available for explicit review and execution; their fresh-machine validation remains outstanding.

## Setup a whole workflow

Open **Setup**, choose a workflow and **Check this workflow**. Review its detected components, then **Review complete workflow setup**. The final review shows what will be reused, installed and configured. Starting it applies verified existing paths and queues missing components sequentially. Existing installations are not upgraded in place. A failed/input-required/restart-required job pauses later components. Configuration must succeed before the next component starts.

The queue and jobs persist across app restarts. A queue from another app version, a changed environment or an ambiguous interrupted job handoff pauses for review. Closing the app does not cancel installation jobs. Use their cancellation controls; package transactions finish their safe step.

New maintained recipes cover Gazebo Harmonic, pinned ArduPilot Copter SITL, the PX4-matched jMAVSim build/native package, the PX4 v1.17 message workspace, ONNX CPU inference and PyTorch CPU training. ROS Jazzy is required before its message workspace. Linux automatic recipes target Ubuntu 24.04 in WSL. Shared apt prerequisites require WSL root; source builds/private Python environments use the selected regular user. jMAVSim may require a separate native Java installation, declared in its review. Existing native jMAVSim is reused when verified.

Gazebo installation supplies the runtime, not every PX4 world/bridge build. The existing configured Gazebo launch command is preserved. ROS messages/Agent packages likewise do not establish live DDS routing automatically. Verify firmware/message versions and live topics after installation.

## Hugging Face and model assets

Open **Settings → Hugging Face**. The input is masked. Save, validate or remove a token for the current Windows user. Storage uses Windows Credential Manager; tokens are not saved in ordinary profiles, manifests, command lines or plaintext WSL login files. Removing the saved copy does not revoke the token at Hugging Face. Public repositories can be used anonymously. Gated access/license acceptance happens on Hugging Face, not automatically in Drone Deck.

In **AI & data → Models**, enter `organization/model`, a revision and comma-separated file patterns. **Inspect revision and size** resolves an immutable commit and reviews selected files, bytes, license and gating metadata. **Download reviewed files** starts a persistent job with progress, cancellation and retry. SHA-256 for LFS assets or Git blob hashes for ordinary files verify downloads; retry can reuse verified files or resume supported partial downloads. Repository code is never executed by downloading assets. A downloaded asset does not imply that its runtime or sensor adapter is ready.

The managed Windows cache is `%LOCALAPPDATA%\DroneDeck\models`. **Refresh local models** and **Verify selected model offline** do not contact Hugging Face. Model selections attach their WSL-mounted path to project manifests; large files are not copied into WSL automatically. Existing local ONNX files can be supplied by their Linux path when running a project. Search, managed import/cleanup/reference tracking and automatic task-specific model compatibility are still future work.

## Recording datasets and offline evaluation

In **AI & data → Datasets & evaluation**, load finalized recording sessions, select a session and create a named dataset version. The version hashes source metadata and artifact hashes. It references the original MCAP files without copying or modifying them. Keep those files; deleting or changing them makes verification fail. A changed dataset manifest also fails its version check.

Evaluate a CSV with `sample_id,target,prediction` and optional `latency_ms`. IDs must be unique. Classification produces accuracy and a confusion matrix; regression produces MAE/RMSE. Optional latency produces mean/p95. Reports retain the dataset version and CSV checksum. A matching inference `run.json` also binds model/input hashes and runtime versions. These scores measure supplied labels/predictions; they do not invent labels or automatically establish frame/time alignment with the recording.

Dataset verification and reports work locally with their inputs present. Downloading packages/models remains an online operation. ROS sensor recording/replay uses the existing [Recordings workspace](RECORDINGS.md), with replay on a separate domain. Real-vehicle monitoring uses its own connection state.

## Editable projects and AI runs

Open **AI & data → Projects** or the project workspace in **Development**. Create an editable ONNX inference, ROS 2 subscriber, MAVSDK monitor or PyTorch CPU-training starter. Creation writes source, instructions and a manifest only. Projects live in the selected WSL distribution under `~/.local/share/drone-deck/projects`; it is recorded in each project manifest.

Install/apply the matching prerequisites explicitly in Setup, then **Run project**. Output goes to the app console; **Stop project** and **Stop session** include owned project processes. The saved distribution must match the selected environment. Generated ROS/MAVSDK starters are read-only. The MAVSDK starter defaults to a distinct endpoint and requires explicit telemetry routing to avoid competing with the built-in listener.

The CPU-training baseline takes NPZ `features` (finite float32 NxD) and integer class `targets` (N, at least four samples), uses a deterministic 80/20 split and produces weights, training metadata, `model.onnx` and `validation.npz`. This small classifier is an editable baseline; it is not a bundled perception or flight-control system.

The ONNX starter accepts a model and correctly preprocessed NPZ tensors named for its inputs. It records CPU batch latency, model/input hashes and output tensors. Optional classification targets produce `predictions.csv`, usable by the evaluation page. CSV latency is the amortized time of one batch, not a measured per-frame real-time deadline. Camera/LiDAR decoding, calibration, timestamp alignment, labeling and task-specific preprocessing remain project adapter responsibilities. Training/inference requires the optional runtimes; they were not installed as part of this update.

## Physical vehicle monitoring

Open **Flight operations → Physical vehicle**. Find serial devices or enter a native endpoint such as `serial:COM4:115200`, `udp:127.0.0.1:14555` or `tcp:localhost:5760`. Start monitoring explicitly. The default monitor sends no heartbeat, arm, mode, parameter-write or flight command. A dedicated endpoint avoids conflicts with a ground station owning the same port.

Select a detected vehicle with a fresh autopilot heartbeat. **Read selected vehicle parameters** sends the read-only parameter-list request and stores raw wire values/types with completeness information. The app retains timestamped MAVLink `.tlog` captures under `workspace/hardware`. The monitor validates CRCs for its supported decoded messages; other frames remain raw log data. MAVLink signatures are not authenticated, and parameter requests on signed links are refused. Use a ground station configured for that link.

Parameter editing, onboard flight-log retrieval and flight commands remain in QGroundControl/Mission Planner. This release's monitor was tested with synthetic loopback MAVLink, not a connected flight controller. Direct physical flight control is not implemented.

## Remaining larger capabilities

Remote/onboard execution and deployment, full perception adapters and sensor visualization, ROS graph/QoS tools, multi-vehicle orchestration, fixed-wing/VTOL workflows, direct flight commands, managed model import/cleanup/search, and comprehensive clean-machine/hardware acceptance remain follow-up work. Command profiles provide extension points, not universal automatic integration.

Upstream references: [HF Hub API](https://huggingface.co/docs/hub/api), [HF tokens](https://huggingface.co/docs/hub/security-tokens), [PX4 ROS 2](https://docs.px4.io/v1.17/en/ros2/user_guide), [Gazebo Harmonic installation](https://gazebosim.org/docs/harmonic/install_ubuntu), [PyTorch CPU releases](https://pytorch.org/get-started/previous-versions/), [MAVLink parameter requests](https://mavlink.io/en/messages/common.html#PARAM_REQUEST_LIST).

# Capability roadmap

The agreed platform scope and delivery order are in the [platform plan](PLATFORM-PLAN.md): a Windows-only desktop application with WSL 2 setup, automatic installation/configuration, adoption of existing tools, physical-drone monitoring/setup, and Hugging Face credentials/model downloads. Native Linux desktop packaging is out of scope; Linux tools run in WSL or future remote runtimes. The entries below describe capability areas; the platform plan takes precedence for implementation order.

A command profile integrates an executable; it does not implement that tool's entire protocol.

## Delivered so far

What each version added is in the [changelog](CHANGELOG.md); how to use it is in the [user guide](docs/user-guide.md).

- Native shell, tool registry, managed processes and logs, staged PX4 SIH → relay → jMAVSim workspace, read-only MAVSDK telemetry (0.1, 0.6). The relay, jMAVSim and MAVSDK telemetry were removed in 0.9.
- rosbag2/MCAP recording, library and isolated replay ([Data & models → Recordings](docs/user-guide.md#recordings-rosbag2--mcap); 0.4). Verified with generated IMU data.
- Windows/WSL setup, reviewed recipes, persistent installation jobs and existing-tool adoption (0.5–0.7), and Guided setup per system (0.8). Clean-machine installation/UAC/reboot validation remains outstanding.
- Hugging Face token and verified model downloads, ONNX/ROS/MAVSDK project starters, native read-only MAVLink monitoring (0.7; [Data & models](docs/user-guide.md#03-data--models), [Physical vehicle](docs/user-guide.md#physical-vehicle)). The 0.7 immutable datasets and CSV evaluation were removed in 0.9.
- Per-user data folder, migration, atomic writes and recovery of damaged files (0.9).
- A WSL agent with the SIH → MAVLink router → native vehicle link arrangement and user-created systems; flight actions and the PX4 console for simulations Drone Deck started (physical vehicles stay read-only from Drone Deck); parameter fetch/export/compare and onboard log download; ULog and `.tlog` Flight logs; extraction and inference Pipelines with Foxglove; crash reports, app log and diagnostics export; a per-user installer (0.9). None of it has run against a real PX4, Gazebo, ArduPilot, ROS 2 graph or flight controller yet.

Not yet verified items are tracked in [known issues](docs/known-issues.md).

## Next capability areas

Develop these as independent adapters with versioned capabilities:

1. **Vehicle operations:** connection identities, selected vehicle and validation of the simulation-only flight actions against a real PX4 and ArduPilot SITL. Keep QGroundControl available.
2. **ROS 2:** discovery/topic browser, QoS and frame conventions, message type/version detection, DDS agent compatibility and project launch files.
3. **Experiment projects:** project-local environments, launch graphs, startup readiness checks and dependency ordering, persistent run manifests and replayable configurations.
4. **AI platform:** timestamp/clock and calibration metadata, offline readiness, versioned model registry, model search/import/cleanup, inference latency metrics and reproducible recorded-data evaluation. Training is not part of the app today (the PyTorch starter was removed in 0.9); bring it back only as a separately validated adapter. Evaluate Rerun/Foxglove adapters for visual analysis while preserving original autopilot logs. Validate model/runtime/workflow compatibility separately from download completion. Choose frameworks per workload; do not couple the shell to a specific model library.
5. **Simulation adapters:** additional Gazebo vehicle/world profiles, an independently validated ArduPilot workflow, Isaac or AirSim variants where supported, version checks and separate simulator/flight-controller adapters.
6. **Hardware:** live vehicle identity on real flight controllers, hardware-specific setup, read-only-first workflows, and independently tested physical-flight command handling.

Remote deployment, full perception adapters, direct flight and hardware/clean-machine acceptance remain the larger gaps. The profile schema already supports custom command entry points for each area.

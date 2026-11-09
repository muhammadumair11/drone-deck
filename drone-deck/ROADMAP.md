# Capability roadmap

The agreed platform scope and delivery order are now captured in [Platform plan](PLATFORM-PLAN.md): a Windows-only desktop application with WSL 2 setup, automatic installation/configuration, adoption of existing tools, physical-drone monitoring/setup, and Hugging Face credentials/model downloads. Native Linux desktop packaging is out of scope; Linux tools run in WSL or future remote runtimes. The entries below describe capability areas; the platform plan takes precedence for implementation order. Delivered subsets and remaining work are distinguished below.

This first build provides the native shell, tool registry, managed processes, logs and a read-only MAVSDK telemetry adapter. A command profile integrates an executable; it does not implement that tool's entire protocol.

Version 0.4 adds a [rosbag2/MCAP recording workspace](RECORDINGS.md): dependency checks, topic discovery/selection, recording with graceful finalization, saved-session browsing, and local replay on a separate ROS domain. The recording stack was verified against generated IMU data. PX4-to-ROS middleware setup, dataset labeling and AI evaluation remain separate work.

Version 0.5 adds the [Windows/WSL setup foundation](SETUP.md): prerequisites, installed/live distribution discovery, reviewed setup recipes, persisted job status/logs/recovery and existing-tool adoption. It preserves saved simulator profiles and gates Linux launches on the configured environment. Recipe implementations and non-installing tests are present; clean-machine installation/UAC/reboot validation remains outstanding. Version 0.7 delivers initial local Hugging Face/model jobs, immutable datasets/CSV evaluation, project scaffolds/CPU runtime recipes and native read-only MAVLink monitoring. See [0.7 capabilities and limits](STUDIO-0.7.md). Broader adapters and hardware acceptance remain outstanding.

Next additions should be developed as independent adapters with versioned capabilities:

1. **Vehicle operations:** connection identities, selected vehicle, MAVSDK action acknowledgements, simulation-only flight-command gating, command timeouts and a flight-state display. Keep QGroundControl available.
2. **ROS 2:** discovery/topic browser, QoS and frame conventions, message type/version detection, DDS agent compatibility and project launch files.
3. **Experiment projects:** project-local environments, launch graphs, startup readiness checks and dependency ordering, persistent run manifests and replayable configurations.
4. **AI platform:** Hugging Face token settings backed by Windows Credential Manager, managed model downloads/cache, rosbag2/MCAP recording and replay, a versioned dataset registry, timestamp/clock and calibration metadata, offline readiness, training job artifacts, versioned model registry, inference latency metrics and reproducible recorded-data evaluation reports. Dataset management, offline operation and recorded-data evaluation are accepted scope. Evaluate Rerun/Foxglove adapters for visual analysis while preserving original autopilot logs. Validate model/runtime/workflow compatibility separately from download completion. Choose frameworks per workload; do not couple the shell to a specific model library.
5. **Simulation adapters:** ArduPilot SITL, additional Gazebo vehicle/world profiles, Isaac or AirSim variants where supported, version checks and separate simulator/flight-controller adapters.
6. **Hardware:** serial/network device detection, live vehicle identity, hardware-specific setup, read-only-first workflows, and independently tested physical-flight command handling.

These are capability areas; the linked version guides describe the delivered subsets. Remote deployment, full perception adapters, model search/import/cleanup, direct flight and hardware/clean-machine acceptance remain larger gaps. The app's profile schema already supports custom command entry points for each area.

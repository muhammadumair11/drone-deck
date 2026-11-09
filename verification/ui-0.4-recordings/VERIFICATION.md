# Drone Deck v0.4 recording verification

- Fourteen Rust tests passed, including recording navigation/state, domain/rate validation, active-session distribution protection, and all existing responsive/zoom/process tests.
- Seven Python worker tests passed: input/path validation, missing ROS diagnostics, replay domain separation/control-topic filtering, incomplete/corrupt-session handling, and explicit local discovery configuration.
- Actual ROS Jazzy test recorded **135 generated IMU messages**, verified MCAP magic bytes and metadata, then replayed on another domain; a subscriber received five matching messages.
- Actual JSON worker stdin/EOF test finalized an additional **199 messages**, validating normal application-close finalization.
- Test capture used ROS domain 89; replay used 90. No vehicle or simulator was started. Generated recordings were temporary and removed by the test harness.
- Installed packages: official `ros2-apt-source` 1.3.0, `ros-jazzy-ros-base`, and `ros-jazzy-rosbag2-storage-mcap` plus their dependencies in Ubuntu-24.04. rosbag2/MCAP package version: 0.26.11.
- Existing tool profiles, runtime commands, and working simulator setup are preserved. Native icon embedding and zoom remain in the executable.

The feature records ROS topics supplied by independently started publishers/bridges. This test does not establish a PX4 DDS bridge or physical-hardware integration. Dataset management, AI evaluation, pause/resume and an embedded timeline are not claimed by this update.

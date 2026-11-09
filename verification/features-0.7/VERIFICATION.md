# Drone Deck 0.7 verification

9 November 2026. Windows desktop client; existing Ubuntu 24.04 WSL installation. No additional application/runtime packages installed. No real-drone connection or flight commands used.

- Rust: 39 tests passed, including navigation, zoom/layout across small and wide windows, new Studio/physical panels, setup failure/handoff/configuration sequencing, argument handling, process lifecycle, recording state and isolated vehicle identity. Existing Rune dead-code/hard-link-cache warnings do not fail the build.
- Release: built offline against the existing Rust toolchain and dependency cache. Headless PNGs checked for Models, Datasets, Projects, Physical vehicle and Settings at desktop/narrow sizes.
- Python: 8 Studio tests passed for immutable dataset hashes, changed-artifact/manifest rejection, classification/regression validation, inference provenance and non-installing project scaffolding; 5 Linux setup tests; 4 mocked add-on recipe tests; 5 discovery fixtures.
- Windows PowerShell 5: script parse/compilation checks; hash/path/MAVLink-fragment/CRC fixtures; temporary Credential Manager save/read/remove under an isolated test target. No real token used. QGroundControl file identity, timeout, safe-transaction and cancellation fixtures passed.
- HF HTTPS: a public pinned 548-byte config fixture downloaded and hash-verified, then removed. No model weights downloaded. Authenticated/gated/large-weight/interruption acceptance remains outstanding.
- Native monitor: synthetic UDP heartbeat, passive default, explicit read-only PARAM_REQUEST_LIST, parameter reply backup, nonempty tlog and graceful shutdown passed. No real serial device tested.
- WSL regressions: 2 supervisor tests and 7 recording-worker tests passed in Linux. The supervisor tests initially failed when run under Windows Python (POSIX-only APIs); rerun in the intended WSL environment passed.

Fresh WSL/UAC/reboot flows, new Gazebo/ArduPilot/jMAVSim/message-workspace installs, optional Torch/ONNX execution, live DDS integration and physical hardware acceptance were not exercised. Recipes display this limitation in their review. Generic sensor-to-feature/label alignment remains user/project-defined.

Deployment and live UI smoke results are appended after publishing the tested executable.

## Deployed application smoke check

- Replaced the root executable with the tested 0.7.0 release; SHA-256 `7BFF9CBCD0DC84E80C720D8F5F06BCAEC42A11313F86F61A8BECC67528547878`.
- All 51 bundled runtime files match the tested source copies. No backup executable was created; the root contains only `Drone Deck.exe`.
- Saved profile SHA-256 remains `AE6B0040EECEC3647862FC36F5B8DC6B02A7045E9A13A9566702CBCA930D4F30`; saved zoom remains 65%.
- Opened the deployed app, maximized it to the screen, navigated AI & data and loaded the real local dataset/project library successfully through its WSL helper. The current library is empty; no synthetic entries were added to user data.
- Setup's non-installing workflow check reports 4/4 existing components verified: PX4 SIH, native jMAVSim/Java, QGroundControl and MAVSDK. This is installation discovery, not a new simulated flight or a vehicle-ready assertion.
- The app is left open at Setup. No simulation, hardware connection, download or installation was started by this UI smoke check.
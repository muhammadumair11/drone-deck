# Drone Deck v0.5 setup verification

- **22 Rust tests passed**, including no-WSL routing, per-environment adoption, preservation of other profiles, active-session guards and all prior recording/zoom/responsive/process tests.
- PowerShell native argument tests passed with spaces, embedded quotes, trailing backslashes, apostrophes and shell-like text. Installed/live catalog parsers and unavailable-catalog handling passed.
- A dedicated non-installing Windows fixture verified atomic job state, visible progress/logs, worker identity and interrupted-worker recovery. Its script never calls the installation runner or package commands.
- **5 Linux helper tests** cover ownership protection, missing-tool handling, literal custom paths and exact managed post-install verification targets.
- Real read-only checks detected Windows/WSL readiness, fetched Microsoft's online catalog and found existing ROS Jazzy/MCAP, MAVSDK 4.1.0, PX4 v1.17.0 and QGroundControl 5.1.5.
- Installation entry points were not live-tested. Automatic approval review rejected a reuse-only installer invocation because failed detection could fall through to package installation. Non-installing tests were used instead. Fresh-OS installs, UAC/reboot and clean source-build validation remain outstanding.
- No additional software installed, no drone/simulator started, and saved flight/tool profiles preserved during this update.

The presence of a managed recipe is distinct from clean-machine verification or protocol-level integration. Guided-only catalog entries and future plan items are labeled accordingly.

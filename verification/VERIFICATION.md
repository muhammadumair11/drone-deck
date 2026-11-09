# Drone Deck 0.1 verification

Date: 2026-11-08 (Asia/Karachi)

## Release

- Native Windows x64 release build: passed.
- Executable size: 21,081,600 bytes.
- SHA-256: `E5240B4A74BEBC8A7B36C3ED15A7B97F34212E61D04E634EEBEFA08D38C834B6`.
- Rune source revision: `a7f276c3630f85a06a0230295da4d2bd172d8d3b`.
- Packaged source matches the tested source. An immediate repeat release build was up to date.

## Checks passed

Four native Rust tests passed: page navigation and JSON validation; integration settings form validation/save; prevention of external actions in preview state; actual process-manager stdout and successful-exit capture using a harmless Windows echo command.

The Ubuntu supervisor test passed: it starts an owned process group, rejects a duplicate owner, stops the owned process and removes its ownership record. The installed MAVSDK Python environment exposes every API used by the read-only adapter, and all Python scripts parse successfully.

The packaged release executable produced nine PNGs and element trees: all six pages, the 1100×800 minimum viewport, scripted entry into the settings form, and scripted navigation into the JSON profile editor. Trees contain the expected settings controls and have no reported tooling warnings. Default, small, settings, flight, simulator, development and integration renders were visually inspected. The previews created no live workspace configuration. No simulator was launched and no flight command was sent during verification.

One upstream release-build warning remains: two unused internal Rune performance-warning methods. No framework implementation was changed to suppress it.

## Setup changes

Installed Microsoft Visual Studio 2022 C++ Build Tools and Windows SDK 10.0.26100.0 to compile the native app. Added the Rust Windows GNU standard-library target while investigating a fallback build path; the shipped executable uses the MSVC target. Downloaded Rune source and Rust dependencies into local build caches. ROS 2, ArduPilot, AI frameworks and additional simulators were not installed.

## Limits of verification and current capabilities

Actual PX4/jMAVSim/Gazebo startup and live telemetry reception through this newly packaged UI still need a first-run session check. Their profiles reuse the user's existing installations and launch arrangements. The process manager and relay ownership were tested independently; this was not an end-to-end simulated flight test.

QGroundControl provides flight controls. This release does not implement direct arming/offboard commands, a ROS graph browser, an AI training/model/dataset platform, embedded external simulator windows, or universal automatic tool installation. Custom command profiles provide extension points for those future adapters. The existing UDP 19410 firewall rule must match the current WSL gateway.

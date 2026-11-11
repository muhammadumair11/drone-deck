# Drone Deck

A Windows desktop workspace for drone autonomy work, built with Rune, the GPU UI framework in this repository (`crates/rune`). The interface runs natively on Windows; Linux flight tools (PX4, ROS 2, MAVSDK, the Micro XRCE-DDS Agent, AI runtimes) run in a WSL 2 distribution, Ubuntu 24.04 by default.

Drone Deck prepares those tools, starts and stops them in the right order, keeps their output, records ROS data, manages models, extraction and inference pipelines and projects, and monitors a physical vehicle read-only. It is organised around one active system: **Local simulation**, **ROS & sensor capture**, **Offline AI workspace** or **Physical vehicle**. QGroundControl remains the flight-control interface for real vehicles. Drone Deck sends flight commands (arm, disarm, takeoff, land, return, hold, mode) and opens the PX4 console only for simulations it started itself; physical vehicles are read-only from Drone Deck. When QGroundControl or the scripts address is enabled for a physical system, the link is a two-way bridge and those programs can command the vehicle through it. Flight actions and the console have been tested only against a loopback fake vehicle; see [known issues](docs/known-issues.md).

## Build

Requirements: Windows 10/11 x64, Rust (the repository's `rust-toolchain.toml`), and Microsoft C++ Build Tools with a Windows SDK. From the repository root:

```powershell
cargo build --manifest-path drone-deck/Cargo.toml
cargo test  --manifest-path drone-deck/Cargo.toml
drone-deck\Build.cmd   # release: Drone Deck.exe + runtime\ + assets\ at the repo root, and dist\DroneDeck-<version>\ + zip
```

`drone-deck\Build.ps1 -Installer` also builds a per-user setup program when Inno Setup 6.3 or newer is installed. The installer and the executable are unsigned, and the 0.9.2 candidate installer has been compiled locally; fresh-machine installation acceptance remains pending; see [Releasing](docs/release.md).

## Run

- Development: `cargo run --manifest-path drone-deck/Cargo.toml` (helper scripts are read from `drone-deck/scripts`).
- Release: open `Drone Deck.exe`. Keep `runtime\` and `assets\` beside it.
- Headless preview: `drone-deck.exe --scene <name> --snap out.png --size 1440x900 [--tree out.json]`. Preview mode never launches processes, installs, downloads or writes your data.

User data lives in `%LOCALAPPDATA%\DroneDeck\` (override with `DRONE_DECK_DATA_DIR`, or a `portable.txt` beside the executable). On first start, data from an older `workspace\` folder beside the executable is copied there.

First time: open **Guided setup**, choose what you want to do and let **Preparation** check this computer. Nothing installs until you review and apply the plan.

## Documentation

- [User guide](docs/user-guide.md): systems, every page, recordings, models, setup, settings.
- [Troubleshooting](docs/troubleshooting.md): WSL, the MAVLink router and vehicle link, ports, QGroundControl GPU-safe mode, data locations, logs and jobs.
- [Known issues](docs/known-issues.md): open defects and what is not yet verified.
- [Architecture](docs/architecture.md): modules, process model, helper scripts, data files.
- [Releasing](docs/release.md): installer, CI release job, crash reports and diagnostics, code signing.
- [Changelog](CHANGELOG.md) · [Roadmap](ROADMAP.md) · [Platform plan](PLATFORM-PLAN.md) · [Icon](ICON.md) · [Rune revision](RUNE_REVISION.md)

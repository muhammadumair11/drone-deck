# Drone Deck

A Windows desktop workspace for autonomous drone development, built with the Rune UI framework. Drone Deck connects simulation, vehicle telemetry, ROS 2, recordings, models and inference workflows in one place.

Repository: [muhammadumair11/drone-deck](https://github.com/muhammadumair11/drone-deck).

## Install and run

Download `DroneDeck-<version>-setup.exe` from [Releases](https://github.com/muhammadumair11/drone-deck/releases). The per-user installer provides a Start menu shortcut and an uninstaller; user data stays under `%LOCALAPPDATA%\DroneDeck`. The installer and executable are currently unsigned.

The ZIP is a portable program package: keep `Drone Deck.exe`, `runtime/` and `assets/` together. Application data still uses LocalAppData unless portable mode is explicitly enabled.

Linux flight tools run in WSL 2, with Ubuntu 24.04 as the initial supported environment. Guided setup detects existing tools and reviews missing prerequisites before installation.

## Build from source

Use Windows 10/11 x64, the Rust toolchain pinned in `rust-toolchain.toml`, and Microsoft C++ Build Tools with a Windows SDK. Inno Setup 6.3 or newer is needed for the setup executable.

```powershell
git clone https://github.com/muhammadumair11/drone-deck.git "Drone Deck"
cd "Drone Deck"
cargo test --locked --manifest-path drone-deck/Cargo.toml
drone-deck\Build.ps1 -Installer
```

The build creates a local executable and `dist/DroneDeck-<version>/`, ZIP and installer outputs. Version tags matching the manifest trigger the release workflow after its checks pass.

## Documentation

- [Application README](drone-deck/README.md)
- [User guide](drone-deck/docs/user-guide.md)
- [Known issues and validation limits](drone-deck/docs/known-issues.md)
- [Architecture](drone-deck/docs/architecture.md)
- [Release packaging](drone-deck/docs/release.md)
- [Changelog](drone-deck/CHANGELOG.md)

Flight commands inside Drone Deck are restricted to app-owned simulations. Physical connections support monitoring and read tools; an enabled QGroundControl/scripts bridge can forward those programs' commands to the vehicle. Real-environment acceptance remains documented in Known issues.

The bundled Rune framework retains its own identity and licensing. Its original repository introduction is preserved in [RUNE-README.md](RUNE-README.md); the app's pinned framework revision is recorded in [RUNE_REVISION.md](drone-deck/RUNE_REVISION.md).

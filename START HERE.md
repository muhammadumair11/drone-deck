# Drone Deck

Drone Deck lives in [`drone-deck/`](drone-deck/README.md). Build it by running `drone-deck\Build.cmd` (it calls `Build.ps1` with a one-off execution-policy bypass), which places **Drone Deck.exe** (with `runtime\` and `assets\`) in this folder, or run it directly with `cargo run --manifest-path drone-deck/Cargo.toml`.

1. **First time:** open **Guided setup** in the rail, choose what you want to try, name the system and let **Preparation** check this computer. Review the plan, tick the acknowledgement and apply it. Nothing installs from a check alone.
2. **System** shows the active system. **Start workspace** starts its tools in order; QGroundControl is the flight-control window, MAVSDK telemetry is read-only.
3. **Tools & config**, **Data & models**, **Activity** and **Settings** (Ctrl+,) hold configuration, data, history and preferences.

**Stop session** stops processes Drone Deck owns. It is not a vehicle landing command.

Read next: [user guide](drone-deck/docs/user-guide.md) · [troubleshooting](drone-deck/docs/troubleshooting.md) · [known issues](drone-deck/docs/known-issues.md) · [changelog](drone-deck/CHANGELOG.md)

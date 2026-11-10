# Drone Deck

Open **Drone Deck.exe** (after building it with `drone-deck\Build.cmd`), or run `cargo run --manifest-path drone-deck/Cargo.toml` from the repository root.

1. **First time:** open **Guided setup** in the rail, choose what you want to try, name the system and let **Preparation** check this computer. Review the plan, tick the acknowledgement and apply it. Nothing installs from a check alone.
2. **System** shows the active system. **Start workspace** starts its tools in order; the native vehicle link shows the live vehicle, and QGroundControl is the flight-control window. Flight actions and the PX4 console appear only for a simulation Drone Deck started; a physical vehicle is read-only from Drone Deck (QGroundControl or scripts enabled for it are bridged two-way and can command it).
3. **Tools & config** holds every tool's command, paths, detection and installation review. **Data & models** holds recordings, models, pipelines (frame extraction and inference on replay), projects and flight logs. **Activity** keeps history, process output and installation jobs. **Settings** (Ctrl+,) holds the Linux environment, zoom, the Hugging Face token and the profile registry.

**Stop session** stops processes Drone Deck owns. It is not a vehicle landing command.

Read next: [user guide](docs/user-guide.md) · [troubleshooting](docs/troubleshooting.md) · [known issues](docs/known-issues.md) · [README](README.md)

# Drone Deck

Open **Drone Deck.exe** in this directory, or the **Drone Deck** shortcut in Desktop → Simulator Launchers.

1. Close any session started by the old VBS launcher before starting a new managed session.
2. Click **Detect tools**, then **Start SIH workspace**. PX4, the relay and jMAVSim run as one managed workspace; jMAVSim opens its own visualizer window.
3. Run **MAVSDK telemetry** for read-only vehicle data, and **QGroundControl** for missions and flight controls.
4. Choose a tool's **Details**, then **Configure manually**, to edit paths and commands through the settings form. **Integrations → Edit profiles** exposes the complete registry for adding more tools.
5. Add command profiles for ROS nodes, AI inference, training, dataset recording, or other simulators. Tools that are not installed need setup; official guidance and optional confirmed package downloads are available in the inspector.

**Stop session** stops owned processes. Normal app close also stops them. It is not a vehicle landing command.

See [application documentation](drone-deck/README.md), [planned capabilities](drone-deck/ROADMAP.md), and [verification results](verification/VERIFICATION.md).

The current app provides process orchestration, logs, settings and a read-only MAVSDK adapter. Direct in-app flight commands, ROS graph visualization, integrated model/dataset registries, and automatic installation of every tool require further adapters. QGroundControl remains the flight-control interface.

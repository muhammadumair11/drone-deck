# Drone Deck

Open **Drone Deck.exe** in this directory, or the **Drone Deck** shortcut in Desktop → Simulator Launchers.

Open **Settings** from the toolbar or sidebar, or press **Ctrl+,**. Use the bottom-right **− / percentage / +** controls to resize the interface. **Ctrl+−** zooms out, **Ctrl++** (or **Ctrl+=**) zooms in, and **Ctrl+0** resets to 100%. The compact default is 85%; your choice is saved. See [Settings and zoom](drone-deck/SETTINGS-AND-ZOOM.md).

**Recordings** provides rosbag2 + MCAP sensor capture and local replay. Click **Check dependencies**, select ROS topics, then **Start MCAP recording** and **Stop & finalize**. ROS sensor publishers/PX4-to-ROS middleware are separate prerequisites. See the [recording guide](drone-deck/RECORDINGS.md).

**Setup** checks Windows and WSL prerequisites, lists installed/available Linux distributions, detects existing tools, and offers reviewed installation recipes with persistent progress/logs. See the [setup guide](drone-deck/SETUP.md). No installation starts from detection alone. Active installation jobs continue if the app closes; use their separate cancellation controls.

1. Close any session started by the old VBS launcher before starting a new managed session.
2. Click **Detect tools**, then **Start SIH workspace**. PX4, the relay and jMAVSim run as one managed workspace; jMAVSim opens its own visualizer window.
3. Run **MAVSDK telemetry** for read-only vehicle data, and **QGroundControl** for missions and flight controls.
4. Choose a tool's **Details**, then **Configure manually**, to edit paths and commands through the settings form. **Integrations → Edit profiles** exposes the complete registry for adding more tools.
5. Add command profiles for ROS nodes, AI inference, training, dataset recording, or other simulators. Tools that are not installed need setup; official guidance and optional confirmed package downloads are available in the inspector.

**Stop session** stops owned processes. Normal app close also stops them. It is not a vehicle landing command.

See [application documentation](drone-deck/README.md), [planned capabilities](drone-deck/ROADMAP.md), and [verification results](verification/VERIFICATION.md).

Version 0.7 adds **AI & data → Models / Datasets & evaluation / Projects**, **Settings → Hugging Face**, reviewed complete workflow setup, and **Flight operations → Physical vehicle** monitoring. See [the 0.7 guide](drone-deck/STUDIO-0.7.md) for use and remaining limits. Optional runtimes are installed only after an explicit Install action. QGroundControl remains the flight-control interface.

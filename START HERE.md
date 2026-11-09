# Drone Deck

Open **Drone Deck.exe** in this directory, or the **Drone Deck** shortcut in Desktop → Simulator Launchers.

Version 0.8 reorganises the app around one **active system** (Local simulation, ROS & sensor capture, Offline AI workspace, Physical vehicle). The rail has **System**, **Tools & config**, **Data & models**, **Activity** and **Guided setup**; **Settings** is at the bottom of the rail or **Ctrl+,**. See [the 0.8 layout guide](drone-deck/UI-0.8.md) for where every 0.7 page moved.

1. First time: open **Guided setup**, choose what you want to try, name the system, then let **Preparation** check this computer. Review the plan, tick the acknowledgement and apply it. Nothing installs from a check alone, and installation jobs continue if the app closes.
2. Close any session started by the old VBS launcher before starting a new managed session.
3. On **System**, **Start workspace** starts PX4 SIH, then the relay once SIH is ready, then jMAVSim once state is forwarding. Use the QGroundControl and MAVSDK branches for flight controls and read-only telemetry.
4. In **Tools & config**, expand a tool with **Configure** to edit its command, paths and working directory, detect it, review its installation or download its package (always confirmed, never run). **Add a tool** creates profiles for ROS nodes, inference, training, recording or other simulators.
5. **Data & models** holds rosbag2/MCAP recordings and replay, Hugging Face models, datasets & evaluation and editable projects. **Activity** keeps a history of requested actions, process output and installation jobs.

**Stop session** stops owned processes. Normal app close also stops them. It is not a vehicle landing command. Zoom with the bottom-right **− / percentage / +** controls or **Ctrl+− / Ctrl++ / Ctrl+0** (default 100%); see [Settings and zoom](drone-deck/SETTINGS-AND-ZOOM.md).

Further guides: [recordings](drone-deck/RECORDINGS.md), [setup recipes](drone-deck/SETUP.md), [0.7 AI & physical monitoring](drone-deck/STUDIO-0.7.md), [application documentation](drone-deck/README.md), [planned capabilities](drone-deck/ROADMAP.md). QGroundControl remains the flight-control interface.

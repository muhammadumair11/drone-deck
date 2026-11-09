# Drone Deck 0.8: system workspace

The interface now follows `Design/`: a black, uppercase DIN layout organised around one **active system** rather than tool categories. Bahnschrift, the DIN typeface that ships with Windows, stands in for D-DIN. The default zoom is now 100%.

## Layout

The **Active system** selector at the top of every management page switches between four saved contexts: **Local simulation**, **ROS & sensor capture**, **Offline AI workspace** and **Physical vehicle**. Switching is refused while owned processes, recordings or installations run.

| Rail | What it holds |
|---|---|
| 01 System | The active system's connections (planned topology; a solid wire only where traffic was observed), the workspace start/stop, QGroundControl and MAVSDK branches, live read-only telemetry, the native vehicle monitor (Physical vehicle), launch progress and the runtime list. |
| 02 Tools & config | Every tool for the active system (or all registered tools), filters and search. Each row expands to its command, guidance, launch-profile form, detection, installation review, docs, folder and a confirmed package download. **Add a tool** creates a profile. |
| 03 Data & models | Recordings (capture, library, isolated replay), Models (Hugging Face review, verified downloads, local library, token access), Datasets & evaluation, Projects. |
| 04 Activity | History of requested actions (saved in `workspace/activity.json`, exportable), process output per tool, installation jobs. |
| Guided setup ↗ | Intention → The system → Preparation → First run. Preparation checks this computer, shows what will be reused or installed, asks for your review, then applies the plan with progress, pause and retry. First run asks for observed evidence before each step is confirmed. |
| Settings | Environment (Windows and WSL readiness, Linux distribution), display & zoom, Hugging Face token, the profile registry (JSON). Ctrl+, opens it. |

## Where the 0.7 pages went

| 0.7 | 0.8 |
|---|---|
| Workspace | System |
| Simulators, Development, Integrations | Tools & config (filters, **All registered tools**) |
| Flight operations → Physical vehicle | System with **Physical vehicle** active |
| AI & data → Models / Datasets / Projects | Data & models tabs |
| Recordings | Data & models → Recordings |
| Setup (workflow presets, checks, batch install) | Guided setup → Preparation |
| Setup (WSL, distribution, jobs) | Settings → Environment; Activity → Installations |
| Process console | Activity → Process output |
| Integration inspector, metric cards, panel toggles | Removed; details expand in each tool row |

A system is marked **prepared** only when Guided setup's Preparation finishes with every tool reused or installed. Process running, installed, configured, connected and healthy remain separate states throughout.

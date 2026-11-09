> In 0.8 these controls moved; see [UI-0.8.md](UI-0.8.md) for the new locations.

> Version 0.7 adds reviewed complete-workflow installation queues and simulator/ROS/CPU-AI recipes. See [0.7 setup guide](STUDIO-0.7.md#setup-a-whole-workflow).

> Version 0.6 adds workflow checklists, repaired discovery/retry handling, reviewed post-install configuration and plain command forms. See [Setup changes](SETUP-0.6.md) and [repair verification](../verification/repairs-0.6/RESOLUTION.md).

# Environment and managed setup — v0.5

Open **Setup** in the sidebar, **Settings → Environment & installation setup**, or **Integrations → Set up environments & install tools**.

## Windows and WSL

Click **Scan Windows and WSL** to inspect Windows build/architecture, observable virtualization availability, RAM, graphics, user-drive free space, and installed distributions. The app opens before Linux is installed. Linux launch actions direct you to Setup if the configured WSL 2 distribution is absent; native Windows tools remain accessible.

Select an installed distribution and click **Use installed environment** to save it. Configuration changes are blocked during active managed processes/recordings. Existing profiles are retained. Changing the distribution does not make tools from another distribution available automatically: detect/adopt their actual locations separately.

**Available distributions** queries Microsoft's live WSL catalog. Ubuntu 24.04 is the recipe baseline; other distributions are labeled Advanced because the Linux install recipes currently validate Ubuntu 24.04 only. A catalog/network error is displayed explicitly. The catalog is not fabricated while offline.

**Install WSL** and **Install selected distribution** open an installation review. Nothing installs until **Install reviewed tool** is clicked. The WSL component step can show a Windows administrator/UAC prompt. A required restart is reported; the app does not restart Windows. Reopen and rescan after restarting. Distribution installation leaves first-run Linux user setup as an explicit step. **Initialize Linux user** opens its interactive terminal; complete setup, then rescan. User-tool creation requires a regular Linux user rather than root.

Virtualization/Windows checks are guidance rather than a guarantee of successful installation; firmware settings, organizational restrictions, optional-feature state and Windows policies can require manual action. Existing WSL 1 distributions are not silently converted or removed.

## Detect, install, or configure

Select a catalog entry:

1. **Detect existing tool** checks known locations and versions without updating them. An optional existing-location field accepts a Linux path for WSL tools or a full Windows executable/JAR path. Current discovery covers ROS Jazzy/MCAP, MAVSDK virtual environments, PX4 checkouts/builds, DDS Agent, QGroundControl, ArduPilot checkouts, Gazebo and a local jMAVSim JAR.
2. **Apply detected launch configuration** saves the selected launch path when the adapter can validate it. Other profiles are preserved. A detected path is bound to its originating WSL distribution. Launch validation is separate from vehicle connection, ROS message compatibility and sensor-data readiness.
3. **Review automatic setup** displays the source/version, target, dependencies and stages. **Install reviewed tool** starts the managed recipe. Existing compatible candidates are offered for reuse rather than upgraded. Otherwise app-owned environments/source folders are created under `~/.local/share/drone-deck/tools`; shared prerequisite packages may be installed through the selected WSL distribution's root account.
4. **Official setup / download** opens the upstream instructions; **Configure manually** opens the app's integration form. These routes remain available when a maintained automatic recipe is unavailable.

Recipe implementations are included for WSL, distribution installation, ROS Jazzy/rosbag2/MCAP, MAVSDK 4.1.0, PX4 v1.17.0 SIH, DDS Agent 2.4.3 and QGroundControl v5.1.5 AMD64. These recipes still require clean-machine validation; their presence is not a claim of an end-to-end tested fresh-OS installation.

PX4 setup uses prerequisites adapted from the upstream Ubuntu script, a private Python environment with the pinned source revision's requirements, and `DONT_RUN=1` to build SIH without starting it. Gazebo and firmware cross-compilers are omitted. DDS Agent 2.4.3 is specifically for a compatible DDS-v2 client; DDS-v3 firmware requires a different version/configuration. Installing the Agent alone does not build a matching `px4_msgs` workspace or establish a vehicle connection.

The QGroundControl recipe obtains the pinned installer from the official release, checks its published SHA-256 or a trusted Authenticode signature, and opens the Windows installer for user interaction. If neither verification method is available, it leaves the downloaded package unexecuted and reports **Needs input**. A custom installation folder may need manual detection afterward.

ArduPilot, jMAVSim and Gazebo currently have detection/guided/manual routes rather than maintained automatic installation recipes. Existing working launch profiles continue to work. Unknown versions are not promoted to automatic adoption merely because a file exists.

## Jobs, cancellation, and recovery

Jobs persist in `workspace/setup/jobs/<id>` as a plan, atomic status file and log. The UI reads status/progress and tails the selected log. Installer output is streamed into that log. Source builds show stages and logs without inventing a download size or time estimate; QGroundControl reports byte progress when its size is known.

Jobs continue if Drone Deck closes. Reopening restores the job list and checks worker process identity. This differs from simulator/recording processes, which are stopped on normal app close. Only one setup worker is allowed at a time, including across app instances. **Stop session** controls flight-tool processes; setup cancellation is a separate action.

**Cancel after current step** writes a request that the worker checks at safe boundaries. Package managers and source-build commands finish their current step; the app does not kill them mid-change. A streaming installer download checks cancellation between chunks. Installed dependencies are preserved. Cancellation is not an uninstall or rollback.

**Review retry** restores the recipe/environment into the review panel. Retrying rechecks existing work and builds on app-owned locations without resetting user source changes. It does not promise byte-level resume for every tool. If a download/build failed, inspect the log; unsupported or modified states can require manual recovery. A job with no live matching worker is presented as interrupted after a short startup grace period.

## Verification and limits

The update was checked with Rust UI/state tests, PowerShell native-argument/catalog tests, a non-installing Windows process/status fixture, Python ownership/discovery tests, real read-only host/catalog queries and discovery of the existing tool installations. Native headless previews cover desktop, compact and no-WSL layouts.

No new software was installed to validate this update. Automatic approval review rejected exercising the real installer entry point as a reuse test because a failed probe could install packages. A dedicated non-installing fixture was used instead. Fresh Windows/WSL installation, UAC/reboot completion and new source builds remain unverified on a clean machine. No destructive fresh-OS simulation was performed on the working laptop.

Hugging Face settings/model downloads, dataset management/evaluation, full offline-project readiness, physical-drone management, remote execution and complete alternate-stack adapters remain later plan items. This release is the environment/installer foundation.

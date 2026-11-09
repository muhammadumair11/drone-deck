# Setup changes in Drone Deck 0.6

Open Setup and choose a workflow, then click **Check this workflow**. Checks reuse no installations automatically and download nothing. The result distinguishes verified installations from missing components and compatibility work.

For each component, **Use this installation** applies its verified launch path. **Review setup** shows the destination, privileges and components before an explicit Install action. The review's **Configure after success** option connects the verified result to the workspace without a separate Apply step; turn it off to apply manually. Completed configuration is recorded so reopening the app does not overwrite later edits repeatedly.

**Browse for existing tool** supports Windows executables/JARs and WSL locations. Custom integrations use a normal form: write a Linux command for WSL, or one Windows argument per line. **Advanced: edit JSON** remains available for full registry editing. Standard tool installation actions now lead to the same Setup system.

Installations and dependent tools are coordinated: stop active tools/recordings before installing shared dependencies, and wait for an installation before launching dependent tools. A long package operation is preserved at its safe step boundary; a Needs attention status identifies a step that exceeds its time budget or goes quiet. It does not silently kill an active package manager.

The SIH launch sequence now waits for vehicle status and live relay packets before starting the visualizer. QGroundControl still reports flight readiness; a running process is not treated as proof of a healthy vehicle connection.

The current laptop's existing SIH, jMAVSim, QGroundControl and MAVSDK chain was checked together. Clean-machine installations and new simulator/AI/hardware adapters remain separate validation/delivery work. See the [repair verification](../verification/repairs-0.6/RESOLUTION.md).

# Drone Deck 0.6.0 repair verification

9 November 2026. This repair release addresses the reported runtime/discovery defects and simplifies setup. The original issue review remains a historical record; this file records implementation and validation.

## Implemented

- SET-01: QGroundControl discovery checks an EXE/PE header, product identity, and version. An explicit invalid location is not silently replaced with another candidate. The installer refuses to treat an unverified file as a reusable installation.
- SET-02–04: discovery continues past incomplete/incompatible PX4 and MAVSDK candidates; PX4 selects a build by executable availability, not directory presence.
- SET-05: Gazebo detection uses a supplied executable/directory location.
- SET-06: retry restores both the original distribution and custom location.
- SET-07: logged read-only commands enforce their deadline. Shared installation transactions opt into preservation; elapsed/stalled steps publish a persistent Needs attention marker rather than silently ignoring the time budget or killing a package manager midway through changes. The marker clears when the transaction completes or output resumes.
- SET-08: QGroundControl's visible installer is reported as needing user input; cancellation is checked after it completes, and cannot silently become Ready.
- SET-09: advanced JSON edits, the normal integration form, detected-tool adoption, and setup/launch actions respect active-workload restrictions. Installer jobs block dependent tool launches; active tools/recordings block new installations.
- SET-10: a managed PX4 retry explicitly restores recursive submodules before building, without resetting local source edits. Invalid/non-repository leftovers still require manual review rather than deletion.
- SIM-01/04: WSL launches use explicit execution to preserve variables, quotes, and process substitutions for the intended child shell. Existing saved commands remain intact.
- SIM-03: the supervisor keeps child input open to avoid PX4's immediate-EOF prompt loop. Log readers cap allocation while reading, before newline termination.
- RUN-01: SIH workspace startup waits for SIH vehicle status, then a relay packet, before launching jMAVSim. Missing readiness, exit, and timeout stop the sequence with a specific message. A running Java process is not labeled proof of successful rendering or flight readiness.
- jMAVSim existing-tool adoption now verifies its JAR and Java runtime and configures the selected native launch path.

## Setup usability

Setup offers workflow checklists for lightweight simulation, PX4/ROS, and advanced ArduPilot development. These are component checklists with explicit remaining connection work, not claims that every listed integration is complete. Existing tools are checked first; each missing component leads to its setup/location action.

The standard inspector now routes installation to Setup rather than a competing raw package-download flow. The manual integration form accepts plain Linux commands or one Windows argument per line. New custom/AI tools can be added through a form; whole-registry JSON remains Advanced. Existing-location browsing converts selected WSL/Windows paths appropriately. Installation review includes destinations, privileges, and component/package details.

The review offers an explicit configure-after-success option. Applying a completed job is recorded persistently so reopening the app does not repeatedly overwrite later profile changes. Selecting another distribution clears cached discovery/telemetry indicators and requires rediscovery of its actual paths.

## Validation completed so far

- 31 Rust tests pass, including responsive layout across screen sizes, friendly arguments, environment locks, retry restoration, workflow states, startup ordering, bounded output, and existing recording/QGroundControl behavior.
- Five isolated Python discovery/retry regression tests pass. Installation commands are mocked in the retry test; no dependencies are installed.
- Two real Linux supervisor tests pass: owned-group cleanup/duplicate rejection and child input remaining open without EOF.
- Five PowerShell checks pass: reject unrelated file, verify the installed QGroundControl identity, enforce a logged-command timeout, preserve a reviewed harmless transaction, acknowledge installer cancellation after completion. All setup PowerShell scripts parse successfully.
- Existing jMAVSim JAR and installed Java 21 were discovered and validated without downloading anything.
- A live SIH start with the repaired supervisor produced one prompt instead of a prompt flood, emitted readiness, and reached Ready for takeoff. A 22-second startup sample showed PX4 at about 31% of one logical CPU, versus the earlier 155–159% observation; these are observations, not a controlled benchmark.
- The repaired relay received and forwarded real SIH packets and published its Forwarding state. Both temporary owned processes were stopped afterward.
- Read-only argument inspection confirmed that `$DRONE_DECK_SCRIPTS` and `$(pgrep -x px4)` reached explicit WSL execution literally.

## Validation still required / scope limits

Fresh Windows/WSL installation, administrator/reboot completion, clean source builds, and real QGroundControl installation were not exercised on this working laptop. No software was installed for this repair. Cancellation while a real package manager is active remains intentionally deferred to its safe step boundary.

The packaged GUI check is complete as recorded below. A new takeoff-to-landing cycle has not been claimed.

Automatic installers for Gazebo/ArduPilot/jMAVSim packaging, a complete firmware-matched ROS project, Hugging Face/model management, datasets/evaluation, physical-vehicle adapters, and remote execution remain product milestones. They were identified as capability gaps in the review; they are not silently labeled implemented by this bug-fix release.

## Packaged deployment and integrated check

- Replaced the root executable with version 0.6.0; its SHA-256 matches the tested release build. No older EXE backup was retained.
- Launched the actual deployed app and clicked Start SIH workspace. The pipeline showed SIH Ready, relay Forwarding, and Java process running, in that order.
- Inspected the native jMAVSim window: the quadrotor was visible in the rendered world, with approximately 48 FPS in the observed view. No claim is made about measured animation during a new flight.
- Opened QGroundControl through Drone Deck and connected the existing WSL SIH link. The ground station displayed Ready / Hold, satellite reception, battery status and the simulated vehicle position. No graphics initialization dialog appeared.
- Started the existing MAVSDK read-only profile from Drone Deck. It connected to autopilot system 1 and populated Connected/altitude telemetry alongside QGroundControl and jMAVSim.
- The deployed Setup workflow check found all four existing components: PX4, jMAVSim/Java, QGroundControl and MAVSDK. No Use installation or Install action was executed; saved launch profiles were not changed.
- Stopped the owned test session through Drone Deck. Read-only process checks found no remaining test PX4, relay, visualizer, QGroundControl or telemetry processes. Drone Deck remains open on Setup.
- The saved profile hash remains `AE6B0040EECEC3647862FC36F5B8DC6B02A7045E9A13A9566702CBCA930D4F30`. The 65% zoom setting was preserved.
- Responsive headless rendering was checked at desktop and 390×844 sizes; native maximized layout was inspected as well.

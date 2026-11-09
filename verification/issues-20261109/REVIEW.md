# Drone Deck issue review — 9 November 2026

Reviewed version: 0.5.1, Windows desktop with Ubuntu tools in WSL 2.

**Disposition: findings only. No fixes, installations, upgrades, flight commands, or configuration changes were performed during this review.** The earlier flight test is summarized separately below. Priorities are proposed discussion order, not permission to implement changes.

## Assessment

The application currently provides a process launcher, recording workspace, and setup foundation. It does not yet provide a single guided, verified setup experience for a complete drone workflow. Several installation and discovery paths contain defects; other confusing steps are unfinished product capabilities rather than broken implementations.

Evidence labels:

- **Reproduced:** exercised an existing helper against isolated, disposable fixtures. No real installer was invoked.
- **Observed earlier:** seen during the preceding SIH/QGroundControl session.
- **Code verified:** followed the deployed implementation; the real installation/failure scenario was not executed.
- **Capability gap:** an agreed or useful workflow that is absent or explicitly manual today.

## Confirmed discovery defects

| ID / priority | Finding and practical effect | Evidence |
| --- | --- | --- |
| SET-01 / High | QGroundControl discovery accepts an unrelated file as a compatible installation. A manually supplied `.txt` file returns `found=true`, `compatible=true`, and an empty version. Applying that result can replace the working executable path with an unusable one; the install branch also treats `found` alone as an existing installation. | **Reproduced.** `runtime/setup/windows-tools.ps1:1–18`; reuse branch `run-job.ps1:62–65`. See `windows-results.json`. |
| SET-02 / High | An incomplete `~/PX4-Autopilot` checkout hides a working app-managed PX4 build. Discovery stops at the first Git checkout even when its binary is missing, rather than continuing to the usable candidate. | **Reproduced.** `runtime/setup/linux-tools.py:54–62`. See `probe-results.json`. |
| SET-03 / Medium | An empty/stale `build/px4_sitl_sih` directory causes discovery to choose that target even when only the default SITL binary exists. A usable installation is reported incompatible. | **Reproduced.** `runtime/setup/linux-tools.py:57–60`. |
| SET-04 / Medium | An incompatible first MAVSDK environment hides a compatible project environment. A managed 3.x candidate stops the search before a project 4.1.0 environment can be considered. | **Reproduced.** `runtime/setup/linux-tools.py:42–52`. |
| SET-05 / Medium | Gazebo discovery ignores the optional existing-location field. It searches `PATH` regardless of the supplied path, so entering a custom executable does not help detection. | **Reproduced.** `runtime/setup/linux-tools.py:80–85`. |

The Python fixtures mock version commands and filesystem discovery. They establish the selection/validation defects, not compatibility of actual installed flight software. All four Python diagnostic cases reproduced; two additional PowerShell diagnostics reproduced SET-01 and SET-07 below.

## Recovery and configuration defects

| ID / priority | Finding and practical effect | Evidence |
| --- | --- | --- |
| SET-06 / Medium | **Review retry loses the original custom location.** Retry calls `choose`, which clears the location, then restores the recipe and distribution but never restores `plan.location`. The retried job can probe a different installation from the original one. | **Code verified.** `drone-deck/src/setup/mod.rs`, `choose` and `retry` at approximately lines 121 and 297. |
| SET-07 / High | **Logged installation commands ignore the supplied timeout.** `Invoke-Native` waits indefinitely whenever a log path is supplied. A stalled build/package command can retain the setup lock indefinitely; cancellation is checked only after the command returns. Finishing a package-manager transaction safely is reasonable, but there is no distinct stalled-job diagnosis or recovery path. | **Reproduced with a harmless process:** a two-second fixture completed successfully despite a one-second timeout. `runtime/setup/common.ps1:26–28`; `windows-results.json`. No actual installer was hung or terminated. |
| SET-08 / Medium | **A cancellation request during the visible QGroundControl installer is not checked afterward.** The worker waits for the installer and then can report Ready despite the pending cancellation request. Cancellation does not mean rollback; the issue is the unacknowledged request and misleading final state. | **Code verified.** `runtime/setup/windows-tools.ps1:59–68`. The real installer was not launched. |
| SET-09 / High | **Advanced JSON editing bypasses the active-process distribution lock.** The normal form and detected-tool adoption block distribution changes while managed tools run. `State.apply` only blocks a distribution change for active recording/replay, and otherwise checks removal of running profile IDs. Keeping the IDs permits switching the shared WSL distribution while processes still belong to the previous one. | **Code verified.** `drone-deck/src/main.rs:192` (`apply`), contrasted with `profile_editor.rs:34–47` and `setup/profiles.rs:2`. No live profile was changed. |
| SET-10 / Medium | **PX4 retry does not explicitly recover interrupted recursive submodule checkout.** If the first recursive clone leaves `.git` and the correct tag but incomplete submodules, retry skips cloning and proceeds toward the build without an explicit submodule recovery stage. Automatic retry is therefore not a dependable recovery path for this failure. | **Code verified recovery gap.** `runtime/setup/linux-tools.py:127–139`. Not reproduced by interrupting a real download; exact build behavior remains untested. |

## Previously observed simulation issues retained for discussion

| ID / priority | Finding | Evidence / limits |
| --- | --- | --- |
| SIM-01 / High | WSL launch processing expands helper variables too early. The relay launch loses `$DRONE_DECK_SCRIPTS`, leaving `/relay.py`; the relay exited with code 2. The MAVSDK telemetry launch uses the same variable pattern and is exposed to the same defect, although that adapter was not run in the preceding flight test. | **Observed earlier** relay failure plus read-only argument reproduction. Backend uses `wsl --` rather than the explicit-exec launch used by setup helpers: `drone-deck/src/backend.rs:248–253,319–328`. |
| SIM-02 / High | jMAVSim opens but receives no live vehicle-state messages with the relay failed. A running visualizer process is not evidence that it is connected. | **Observed earlier:** `waiting for first message from: /0.0.0.0:19410`. This is a consequence of the broken transport, not a separately established rendering defect. |
| SIM-03 / High | PX4 floods stdout with repeated `pxh>` prompts. Linux reported roughly 155–159% CPU, approximately 1.6 logical cores, including before takeoff. The backend limits a line only after `BufRead::lines` has already accumulated it, so the displayed 3,000-character limit does not bound an unterminated input line in memory. | **Observed earlier** prompt flooding/CPU; **code verified** buffering weakness in `backend.rs:355–370`. EOF-driven prompting is a likely cause, not a completed root-cause proof. No out-of-memory failure was observed. |
| SIM-04 / High | The duplicate-PX4 guard is expanded prematurely. The actual supervisor command contained `test -z ""` instead of the intended process lookup. | **Observed earlier** process command line. A second PX4 instance was not deliberately started. |
| RUN-01 / Medium | Workspace launch has no dependency/readiness graph: it requests SIH, relay, and jMAVSim launches, and marks each Running on process spawn. It does not establish that the transport is connected or that flight readiness passes. | **Code verified**, `main.rs:152–164`, `backend.rs:359–387`; consistent with the earlier running-but-disconnected visualizer. |

Earlier flight result: QGroundControl launched successfully, connected through the saved WSL SIH link, and showed Ready. The local PX4 process had `PX4_SIMULATOR=sihsim` and `PX4_SIM_MODEL=sihsim_quadx`; preflight checks passed. After the user held QGroundControl's takeoff confirmation, PX4 reported armed, airborne, approximately 2.84 m above takeoff, with no failsafe. Desktop automation was then stopped with Escape. **Landing and a complete takeoff-to-landing cycle remain unverified.** This review did not resume flight control.

## Why setup still feels complicated

1. **There is no workflow-level setup wizard.** The user must choose WSL, select/use a distribution, initialize its user, choose individual tools, detect/install them, apply each launch configuration, and then determine connection readiness. The app does not walk a user through a complete “PX4 SIH + visualizer + QGroundControl” or “PX4 + ROS 2” result.
2. **There are two download systems.** Setup provides persistent installation jobs; the legacy inspector has a separate configured-package download action. The latter downloads a file but does not install/configure it or provide the same managed job lifecycle. Similar labels lead to different outcomes. Source: `backend.rs:433–478` and the Setup page.
3. **Installation completion and workspace configuration are separate.** Even a verified install needs a separate Apply action. Keeping approval for changes to existing tools is appropriate, but there is no guided final step explaining exactly what remains before the chosen workflow works.
4. **The manual route expects command-authoring knowledge.** It requires Windows versus Linux paths, a JSON argument array, Bash commands, probes, and a shared distribution name. The location field has no file/folder picker. Adding a custom tool drops into the whole-profile JSON editor. Source: `profile_editor.rs`, `ui/pages.rs`.
5. **Readiness labels disagree.** The global Detect tools action largely tests file existence or a shell probe, whereas Setup uses separate discovery/compatibility checks. “Installed,” “compatible,” “running,” “connected,” and “healthy data” do not form one consistent checklist. A valid WSL 2 registration also does not prove a regular Linux user or usable toolchain is ready.
6. **The review does not fully expose the actual change plan.** It shows recipe summaries and stages, but not a complete package list, resolved per-user destination, dependency graph, or per-step privilege requirements. A user cannot easily tell which parts are Windows, WSL-system packages, or private environments.
7. **Installation is not coordinated with live workloads.** Setup prevents another setup worker, but starting installation is not blocked by active simulation/recording processes. Adoption is blocked later, so the user can finish installing and then encounter a separate “stop workspace” barrier. Shared package changes are not coordinated with running dependents.
8. **Changing the shared WSL distribution preserves old tool paths.** This is documented, but the interface makes the user rediscover and repair every affected integration. There is no consolidated migration/validation checklist.
9. **Cancellation offers little help for a long or stalled step.** Finishing the current package operation is intentional; the UI does not separately explain whether an operation is progressing, waiting for interaction, or stuck. SET-07 compounds this.

## Capabilities still absent or intentionally manual

These are **plan/coverage gaps**, not newly discovered regressions:

- jMAVSim, Gazebo, and ArduPilot have no maintained automatic install recipes. Their discovery results remain incompatible for automatic adoption, leaving command configuration/manual validation. Installing the PX4 SIH toolchain therefore does not produce the whole visual simulation stack on a fresh machine.
- ROS installation provides Jazzy/recording packages; DDS Agent installation alone does not supply a matching `px4_msgs` workspace, user ROS project, compatible firmware/message matrix, or a checked PX4-to-ROS data connection.
- Hugging Face credentials/model downloads, dataset management, recorded-data AI evaluation, offline workflow readiness, remote execution, and dedicated physical-vehicle management remain planned. The AI page currently hosts configurable commands and a link to Recordings.
- Fresh Windows/WSL installation, UAC/reboot completion, and clean PX4/DDS source builds remain unverified end to end. The existing persisted setup job reviewed was a ROS **reuse** result, not evidence of a clean installation.

## Proposed order for a later agreed repair pass

1. Repair simulation transport/argument handling, duplicate-process checks, and console/resource behavior; complete a short takeoff/hover/landing test with live visualizer state.
2. Correct discovery validation, candidate fallback, target selection, retry state, and configuration locks.
3. Give long jobs explicit waiting/stalled/cancellation states and bounded diagnostics while preserving package-manager integrity.
4. Replace the scattered setup entry points with a workflow wizard: **choose goal → check prerequisites → reuse or install missing components → review configuration → verify connections → ready**. Keep an Advanced option for raw commands/JSON.
5. Add and validate the missing complete recipes/adapters, then deliver the AI and hardware milestones as explicitly scoped capabilities.

## Verification and preservation

- Six isolated diagnostic observations reproduced; only disposable fixtures and audit files were written.
- All six deployed `runtime/setup` files match their bundled source copies by hash.
- The saved `workspace/profiles.json` SHA-256 remains `AE6B0040EECEC3647862FC36F5B8DC6B02A7045E9A13A9566702CBCA930D4F30`, matching the pre-review value.
- No application source, executable, saved profile, or UI preference was edited. No real installation, update, firewall change, or simulator process manipulation was performed in this review.
- No additional Windows desktop automation was used in this review.

Audit files: `probe_diagnostics.py`, `probe-results.json`, `windows_diagnostics.ps1`, and `windows-results.json` beside this report. Fixtures do not establish clean-machine installer success; code-verified scenarios remain candidates for dedicated reproduction during the later repair pass.

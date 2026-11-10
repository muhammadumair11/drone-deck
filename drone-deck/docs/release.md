# Releasing Drone Deck

For maintainers. How a release is built, what the installer does, how crash reports and diagnostics work, and what is still missing (code signing).

## Building a release

```powershell
drone-deck\Build.ps1                 # release build, local deploy, dist\DroneDeck-<version>\ and its zip
drone-deck\Build.ps1 -Installer      # the same, plus dist\DroneDeck-<version>-setup.exe
drone-deck\Build.ps1 -NoLocalDeploy -Installer   # what CI runs
```

`-Installer` compiles `drone-deck\installer\drone-deck.iss` with Inno Setup 6.3 or newer (`ISCC.exe`; the script uses the `x64compatible` architecture name). The script looks for `ISCC.exe` on `PATH`, then under `Program Files (x86)`, `Program Files` and `%LOCALAPPDATA%\Programs` (where `winget install JRSoftware.InnoSetup` puts a per-user install). If it is not found, the installer step is skipped with a message and the package folder and zip are still produced.

The installer packs the `dist\DroneDeck-<version>\` folder exactly as `Build.ps1` produced it, so the zip and the installer always ship the same files.

**CI.** Pushing a tag `v<version>` runs the `release` job in `.github/workflows/drone-deck.yml`: it checks the tag against `drone-deck/Cargo.toml`, installs Inno Setup with `choco install innosetup`, runs `Build.ps1 -NoLocalDeploy -Installer` under Windows PowerShell 5.1, checks that the setup executable exists and attaches the zip and `DroneDeck-<version>-setup.exe` to the GitHub Release.

## What the installer does

- Installs **for the current user only**: no administrator prompt (`PrivilegesRequired=lowest`), into `%LOCALAPPDATA%\Programs\DroneDeck`.
- Adds a Start menu shortcut, and a desktop shortcut when that task is ticked.
- On upgrade, replaces `runtime\`, `assets\` and `docs\` whole, so helper scripts a newer version no longer ships do not linger. It asks a running Drone Deck to close first (Restart Manager).
- The uninstaller removes the program folder only. **User data stays**: `%LOCALAPPDATA%\DroneDeck\workspace` (profiles, systems, activity, setup jobs, exports), `\logs`, `\models`, `\tools`, the Hugging Face token in Windows Credential Manager, and everything inside the WSL distribution. A portable `workspace\` next to the exe (with `portable.txt`) is also left in place.
- `AppId` in the script is fixed; changing it would make Windows treat a new version as a different application.

## Code signing (not done yet)

The executable and the setup program are **unsigned**. Windows SmartScreen therefore shows "Windows protected your PC" on first run of a downloaded setup, and some antivirus products score unsigned installers lower. Nothing in the build depends on signing, so it can be added without changing the installer layout.

Follow-up, once a certificate is available (an OV or EV code-signing certificate, or Azure Trusted Signing):

1. Store the certificate (or the Trusted Signing credentials) as GitHub Actions secrets; never commit it.
2. In the `release` job, sign `Drone Deck.exe` with `signtool sign /fd SHA256 /tr <timestamp URL> /td SHA256` after `cargo build` and before packaging, so the zip and installer carry the signed exe.
3. Add a `SignTool=` entry to `[Setup]` in `drone-deck.iss` (Inno passes it the setup and uninstaller to sign) and define the sign tool with `ISCC /S`.
4. Verify with `signtool verify /pa /v` in CI before the release upload.

## Crash reports, app log and diagnostics

All of this lives in `src/diagnostics/` and writes to the **logs folder**: `%LOCALAPPDATA%\DroneDeck\logs` beside the default data root, or `<data root>\logs` when `DRONE_DECK_DATA_DIR` or `portable.txt` moves the data root. Headless runs and tests never write there.

- **App log** `app.log`: one line per event (start, startup notices, job-object failures, panics, diagnostics exports), each line capped at 4,096 characters. At 1 MiB it rolls to `app.1.log`, then `app.2.log`; at most three files (~3 MiB). If rolling fails (a viewer or scanner holding a rolled file), `app.log` keeps growing to 2 MiB and further lines are dropped until a roll succeeds.
- **Crash reports** `crash-<YYYYMMDD-HHMMSS>.txt`: a panic hook (installed first thing in `main`) writes the message, location, thread, OS version and a backtrace, then runs the default hook. At most three reports per run and ten in the folder. A panic before the first `State` is built is not recorded (logging is enabled by `diagnostics::startup`).
- **Next launch**: the newest report not yet offered is announced in the startup notice, recorded in the activity history, and shown as a strip at the top of Activity with *Open logs folder* and *Dismiss*. `crash-acknowledged.txt` remembers which report was offered, so it is offered once.
- **Export diagnostics** (Activity): writes `<data root>\exports\drone-deck-diagnostics-<stamp>.zip` and opens that folder. Contents: `about.txt` (version, OS, data root, host scan with WSL distributions, job object), `state.txt` (owned process status and readiness, installation jobs), `profiles.json`, `activity.json`, `process-output.log`, `data/systems.json`, `data/ui-preferences.json`, `logs/app*.log`, the five newest crash reports, and `plan.json`/`state.json`/`output.log` of the five newest installation jobs. Each file is cut to its last 512 KiB. The zip is written by a small store-only writer (`diagnostics/zip.rs`), no extra dependencies.
- **Redaction** (`diagnostics/redact.rs`): `%LOCALAPPDATA%`, `%APPDATA%`, `%TEMP%`, `%USERPROFILE%` and `%OneDrive%` paths are replaced by those names in every spelling (`\`, `/`, JSON-escaped); any other `\Users\<name>` or `/home/<name>` segment becomes `<user>`; the Windows user, computer and domain names become placeholders as whole words; `hf_…`, `ghp_…` and `github_pat_…` tokens are cut, and so are the values of `KEY=value`, `key: value` and `"key": "value"` pairs whose key names a token, password, secret or API key (`HF_TOKEN=…`, `--api-key=…`). Every entry, including the in-memory ones, is cut to its last 512 KiB before redaction. Process output can still mention project or file names, so the export notice asks the user to review before sharing.

## Windows child processes (job object)

`src/winjob.rs` keeps one Job Object for the life of the app, created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The backend starts every Windows-native child (QGroundControl and other Windows profiles) suspended, adds it to the job and only then resumes it, so every process the child starts is in the job too. When Drone Deck exits for any reason, including a crash or Task Manager, Windows closes the handle and ends those children and their descendants. WSL children (`wsl.exe`) are not added: killing `wsl.exe` does not stop the Linux processes, which the WSL agent owns. A failed assignment is written to `app.log` and the child keeps running outside the job; a child that cannot be resumed is killed and reported as a start failure. Resuming uses `NtResumeProcess`, an undocumented `ntdll` export; it has been tested only on the Windows 10 development machine.

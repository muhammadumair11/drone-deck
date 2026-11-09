# Drone Deck 0.2 — responsive native UI

The Rune UI now follows `Design/styles.css`: white surfaces, warm-gray navigation, blue actions, black active navigation and dark command/log blocks. Installed Segoe UI and monospace fonts are used without downloading font packages. The design's prototype-only simulated telemetry and mission data were replaced with actual app state and existing command integrations.

The previous 1100×800 minimum and fixed 1440×960 opening size were removed. Startup uses the desktop work area; after creation the app also clamps its size to the actual monitor's work area and DPI scale. The native minimum is now 360×320.

- Below 900 logical pixels, navigation becomes a keyboard-accessible drawer; Escape and outside clicks dismiss it.
- Metrics reflow from four columns to two to one. Pipeline steps stack when the available panel width is narrow.
- The inspector moves below the main content on smaller windows. Details buttons bring it into view; it can also be hidden.
- Page content scrolls vertically within the window. The header and status bar remain reachable. The console is optional and scrolls its own output.
- Short windows omit the navigation footer so the navigation list remains usable.
- Tool filtering, command copying and project-folder opening use real state/actions. No new simulator or AI packages are installed by this update.
- Settings and page navigation reset the page scroll. Confirmations are centered modal dialogs, including on short screens.

The process backend and saved profile schema remain unchanged. Existing workspace profiles are preserved. This update adds no direct flight commands or new vehicle-control behavior.

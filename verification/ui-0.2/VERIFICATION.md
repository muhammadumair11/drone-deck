# Drone Deck 0.2 UI verification

Date: 2026-11-09, Asia/Karachi.

The native Rune UI was ported from the supplied Design HTML/CSS/JS reference. The light palette, blue actions, warm-gray navigation, pipeline panels, dark command blocks, and dark process console use the exported visual system. Prototype-simulated mission/vehicle data was not added to the production backend.

## Passed checks

- Eight native Rust tests passed: existing process/output capture, profile editing, settings validation, preview execution guards, plus live resizing across every page, compact navigation/search, monitor work-area sizing math, and centered confirmation/Escape handling.
- Workspace renders cover 360×800, 390×844, 430×932, 600×960, 820×1180, 1024×768, 1366×768, 1440×900, 1920×1080 and a short 640×360 window.
- All other pages and settings forms were rendered at compact, tablet and desktop sizes.
- Scripted compact navigation, typed search and selection of Gazebo details passed. Desktop → compact → desktop resizing and drawer Escape dismissal were exercised headlessly.
- Thirty-one native PNG/element-tree scenarios were checked. No horizontal element overflow or tooling warnings were found. Desktop, compact settings, compact details and current-screen startup renders were visually inspected against the exported design.
- The current desktop produced a 1216×576 logical startup viewport. The native window also clamps to its actual monitor work area using the window's DPI scale after creation.

All checks were headless except for a harmless hidden Windows echo process used by the process-manager regression test. No simulator was launched and no flight commands were sent. Real multi-monitor dragging and physical flight behavior were not tested.

The upstream Rune release build retains its existing unused-internal-method warning. No new application warnings or dependencies were introduced. Production source files remain below the 800-line limit. The framework's Git-based size checker is unavailable in this exported tree (Windows CRLF shell scripts and missing Git metadata); scoped application file counts were checked directly. The deployed executable also passed a compact settings smoke render and matches the tested binary's SHA-256 fingerprint.

## Preserved data

The process backend, runtime scripts, simulator installations and profile schema were not changed. The saved workspace profiles were verified unchanged during deployment. The previous executable is retained as `Drone Deck.before-ui-20261109.exe`; close the current app before using it for rollback.

Use `drone-deck/UI-UPDATE.md` for the responsive behavior and `workspace-1440x900.png` for the desktop preview.

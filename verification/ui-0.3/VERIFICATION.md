# Drone Deck v0.3 — Settings and zoom

Validated with the native Windows release build using Rune's headless renderer and input scripts. No simulator or flight-control process was started.

- Ten Rust tests passed: responsive layout, zoom bounds, scaled pointer targets, keyboard shortcuts, Settings navigation, and existing process/profile behavior.
- Native previews inspected at 1216×576, 1216×700, and 390×844.
- Scripted keyboard zoom rendered at 50%, 100%, and 150%; global footer controls remained visible.
- A combined keyboard/pointer script opened Settings and finished at 95%, confirmed in the rendered tree.
- Existing flight/tool profiles and backend source were preserved.

Default zoom is 85%. Interactive choices are persisted in `workspace/ui-preferences.json`. Headless runs do not read or write that preference.

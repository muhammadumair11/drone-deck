# Drone Deck icon

The existing charcoal-and-white D branding now has a vector master, a transparent 256px PNG, and a Windows ICO with 16, 20, 24, 32, 40, 48, 64, 128 and 256px frames.

- The sidebar uses the vector logo.
- Rune loads the embedded PNG for the title bar, taskbar and Alt+Tab.
- `build.rs` embeds the ICO into the executable with the existing Windows SDK resource compiler. No additional Rust dependency is required.
- The Windows shortcut uses `assets/drone-deck.ico` beside the executable.

Regenerate the raster assets on Windows with `powershell -NoProfile -File scripts/generate-icon.ps1`. Build normally with `Build.ps1`; it copies the assets alongside the executable. `RC` can override the resource compiler path.

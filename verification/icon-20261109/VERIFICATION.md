# Drone Deck — native icon update

- All ten existing Rust tests passed.
- The Windows resource loader verified the embedded icon group and all nine size resources: 16, 20, 24, 32, 40, 48, 64, 128, and 256px.
- Windows extracted the executable's icon successfully.
- The native release build was rendered headlessly to confirm the sidebar logo. The monochrome vector monogram uses Rune's existing drawing support without adding SVG runtime dependencies.
- The installed shortcut now uses the packaged multi-resolution ICO. Flight profiles and runtime commands were preserved.

No new packages or applications were installed.

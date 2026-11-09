# Settings and zoom — v0.3

Open **Settings** from the top toolbar or navigation, or press **Ctrl+,**. Application settings are now separate from tool-specific settings under **Integrations → Details → Configure manually**.

The bottom-right zoom controls are always available:

- **−** / **Ctrl+−**: zoom out by 5%.
- **+** / **Ctrl++** or **Ctrl+=**: zoom in by 5%.
- Click the percentage or press **Ctrl+0** to reset to 100%.
- **Settings → Display & zoom → Compact 85%** returns to the smaller default.

The range is 50–150%. Zoom scales the entire UI, updates the available layout space and reflows responsive panels. It does not change Windows display scaling. The choice is stored in `workspace/ui-preferences.json`; existing flight/tool profiles are unchanged.

The Settings page also exposes workspace-panel visibility and a direct route to integration settings. On very short windows, the toolbar can scroll so that bottom-right zoom/recovery controls stay reachable.

Ten tests pass, covering zoom bounds, keyboard commands, settings navigation, responsive resizing, scaled pointer hit targets and the existing process/profile behavior. Native headless previews disable preference writes and external process launches.

# Settings and zoom — v0.8

Open **Settings** from the bottom of the rail (or the navigation drawer on narrow windows), or press **Ctrl+,**. It holds Environment (Windows and WSL readiness, Linux distribution), Display & zoom, the Hugging Face token and the profile registry. Tool-specific paths and commands live in **Tools & config**: expand a tool with **Configure**.

The bottom-right zoom controls are always available:

- **−** / **Ctrl+−**: zoom out by 5%.
- **+** / **Ctrl++** or **Ctrl+=**: zoom in by 5%.
- Click the percentage or press **Ctrl+0** to reset to 100%.

The range is 50–150% and the default is 100%. Zoom scales the entire UI and reflows the layout: below 720 logical pixels the rail becomes a drawer opened with **Menu**. It does not change Windows display scaling. The choice is stored in `workspace/ui-preferences.json`; a saved 85% from earlier versions is kept until you change it.

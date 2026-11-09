# QGroundControl GPU-safe startup fix — v0.5.1

- Reproduced the OpenGL graphics-backend error using QGC's installed GPU Safe Mode shortcut. Matching its working directory alone did not resolve the error.
- Launched QGC with process-local `QT_QUICK_BACKEND=software` and `QT_OPENGL=software`, clearing conflicting RHI hints. Observed its actual main window and rendered map through the computer-use skill.
- Added the same renderer configuration to Drone Deck's managed QGC launch path when the profile explicitly contains `-swrast`/`--swrast`. Normal graphics selection and explicit working-directory choices remain available.
- All 25 Rust tests passed, including process-local graphics settings, normal-mode/working-directory preservation and exclusion of unrelated tools.
- After deployment, reopened Drone Deck and clicked its actual QGroundControl Run button. Observed the managed QGC main window rendering successfully with no graphics initialization dialog; Drone Deck recorded the GPU-safe launch and Running status. Both apps were left open for the user.
- Removed the five older standalone backup EXEs at the user's request. Only the current Drone Deck executable remains in the application root.
- Saved tool profiles and vehicle settings were not reset or edited. No new software or graphics drivers were installed.

The fix selects Qt Quick's software adaptation. GPU-dependent 3D/shader/video features may be limited; this check establishes normal GUI/map startup, not those features.

References: [QGC's swrast option](https://github.com/mavlink/qgroundcontrol/blob/v5.1.5/src/Utilities/QGCCommandLineParser.cc), [Qt Quick software adaptation](https://doc.qt.io/qt-6/qtquick-visualcanvas-adaptations-software.html).

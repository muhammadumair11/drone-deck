# Drone Deck UI/UX design

This directory contains an editable responsive prototype for the Drone Deck desktop application. Open `index.html` in a modern browser to explore it.

## Product intent

Drone Deck is treated as an operator workspace for local autonomy development. Its core responsibility is to make the state of the simulation stack legible: what is installed, what is running, how the vehicle is connected, what each tool owns, and what happened during the session.

The design favors operational clarity over decorative density. A primary action is visible at each step, statuses use text and shape as well as color, detailed commands stay available without dominating the page, and process output remains tied to a named source.

## Information architecture

- **Workspace** is the operational home. It combines health summaries, the PX4/SIH startup pipeline, tool controls, integration details, and console output.
- **Simulators** organizes available flight environments and their intended use.
- **Flight operations** groups readiness, mission, and safety workflows.
- **Development** surfaces build targets, runtime environments, and logs.
- **AI & data** separates datasets, model runs, and evaluations.
- **Integrations** makes external dependencies discoverable and configurable.

The sidebar is the single primary navigation model on large screens. It becomes an off-canvas drawer on narrow screens; no competing bottom navigation or tab system is introduced.

## Primary operator flow

1. Detect local tools and review availability.
2. Inspect the active PX4/SIH profile and setup requirements.
3. Start the SIH workspace.
4. Confirm the flight stack, transport, and visualizer are running.
5. Launch optional session tools such as QGroundControl or MAVSDK telemetry.
6. Review, filter, export, or clear process output.
7. Stop the session, releasing every process owned by the application.

The prototype simulates this flow locally. Session startup has a short intermediate state, owned processes are stopped together, and the workspace summaries always derive from the same state as the controls.

## Responsive behavior

- **Wide desktop:** persistent sidebar, four status summaries, two-column workspace, and a sticky integration inspector.
- **Laptop:** reduced navigation width and a single-column workspace when the inspector would crowd the launch flow.
- **Tablet:** off-canvas navigation with a visible menu control; feature collections use two columns.
- **Phone / narrow window:** status summaries, pipeline stages, cards, console controls, and tool actions stack in reading order. Essential actions remain visible and at least 44px high.

The layout reorganizes at each width rather than scaling the desktop canvas. It avoids horizontal page scrolling and does not disable browser zoom.

## Interaction and state model

### Session

- `stopped`: vehicle offline, pipeline stopped, Start enabled, Stop disabled.
- `starting`: startup feedback is visible and repeated activation is prevented.
- `running`: pipeline reports running, vehicle summaries update, Stop is enabled.

### Tools

- `unchecked`: detection has not confirmed the tool and Run is unavailable.
- `ready`: the tool is installed and can be launched.
- `running`: the status and action change together; the action becomes Stop.

### Feedback

- Console messages use timestamp, source, and content as separate fields.
- Toasts confirm lightweight actions without blocking the workflow.
- Dialogs contain secondary details and return keyboard focus to the originating control.
- Export and download actions produce local files instead of decorative buttons.

## Accessibility decisions

- Semantic navigation, landmarks, headings, buttons, labels, and a native dialog are used.
- A skip link reaches the main workspace.
- Keyboard focus is visible on every interactive control.
- Status meaning is expressed with text and shape, not color alone.
- Console output is announced as a live log without interrupting the operator.
- Motion is limited to state feedback and respects reduced-motion preferences.
- Body copy remains at a readable size and controls retain touch-friendly targets.

## Visual direction

The interface uses restrained productivity styling: warm neutral surfaces, near-black text, whisper borders, compact radii, and one blue interaction accent. Green, amber, and red are reserved for status semantics. The visual density is intentionally moderate so the app remains efficient on engineering workstations without becoming fragile on smaller screens.

Design values are centralized in `styles.css`. Reusable patterns cover buttons, navigation, status summaries, panels, pipeline stages, tool rows, console output, cards, dialogs, and feedback.

## Integration assumptions

Only the existing UI screenshot was available; the application source was not supplied. The prototype therefore does not claim a specific desktop framework or process API.

When integrating with the real application:

- Replace simulated timers with events from the native process manager.
- Keep one authoritative session state for cards, pipeline stages, buttons, and console output.
- Route process output through stable tool identifiers rather than display labels.
- Connect tool detection to explicit configured paths and return actionable failures.
- Preserve stop ownership: the application should stop only processes it started.
- Replace example commands, ports, and environment labels with verified project configuration.
- Keep navigation and component structure while adapting event handlers to the project's framework.

## Files

- `index.html` — application shell and accessible interface structure.
- `styles.css` — centralized tokens, components, and responsive behavior.
- `app.js` — local interaction, navigation, session, tool, dialog, and export behavior.
- `README.md` — product rationale, UX model, responsive rules, and integration guidance.

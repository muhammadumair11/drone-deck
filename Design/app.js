(function () {
  "use strict";

  const toolCatalog = [
    { id: "qgc", name: "QGroundControl", description: "Vehicle setup, missions, parameters, and flight control", command: "qgroundcontrol.exe", port: "UDP 14550", installed: false, running: false },
    { id: "mavsdk", name: "MAVSDK telemetry", description: "Read-only live vehicle data and health stream", command: "python telemetry_bridge.py", port: "UDP 14540", installed: false, running: false },
    { id: "mission", name: "Mission monitor", description: "Local mission validation and event timeline", command: "drone-deck mission watch", port: "Local IPC", installed: true, running: false }
  ];

  const views = {
    simulators: {
      title: "Simulators", eyebrow: "Simulation catalog", headline: "Choose the right flight environment.",
      copy: "Review local and connected simulation targets, then bring a selected profile into the workspace.", action: "Create simulator profile",
      cards: [
        ["PX4 / SIH", "Fast local feedback", "Internal physics with low startup overhead for control-loop work."],
        ["jMAVSim", "Visual verification", "A lightweight native visualizer for vehicle attitude and movement."],
        ["Gazebo", "Environment fidelity", "Scene-based simulation for sensors, obstacles, and multi-vehicle tests."]
      ]
    },
    flight: {
      title: "Flight operations", eyebrow: "Mission readiness", headline: "Move from checks to flight with confidence.",
      copy: "Keep vehicle health, mission state, and operator decisions in one predictable sequence.", action: "Open preflight checklist",
      cards: [
        ["Preflight", "12 readiness checks", "Confirm power, sensors, position, firmware, and safety settings."],
        ["Mission timeline", "No active mission", "Review waypoints and events before the vehicle is armed."],
        ["Safety events", "Monitoring enabled", "Surface warnings and required operator acknowledgements." ]
      ]
    },
    development: {
      title: "Development", eyebrow: "Local toolchain", headline: "Build, run, and inspect autonomy software.",
      copy: "Expose the commands, environments, and output that matter without burying operators in terminal setup.", action: "Open development task",
      cards: [
        ["Build targets", "PX4 SITL · Quad X", "Keep frequently used build profiles ready and reproducible."],
        ["Environment", "Ubuntu 24.04 on WSL", "Track dependencies, paths, and detected tool versions."],
        ["Logs & traces", "Local session history", "Connect runtime output to the tool and session that produced it."]
      ]
    },
    ai: {
      title: "AI & data", eyebrow: "Autonomy intelligence", headline: "Turn flight data into useful decisions.",
      copy: "Organize datasets, model runs, and evaluation results around the vehicle behavior they affect.", action: "Create analysis run",
      cards: [
        ["Datasets", "No dataset connected", "Attach telemetry, imagery, or labeled event data for analysis."],
        ["Model runs", "Workspace is idle", "Track inputs, configuration, output, and evaluation together."],
        ["Evaluations", "Define acceptance criteria", "Compare model behavior against explicit safety thresholds."]
      ]
    },
    integrations: {
      title: "Integrations", eyebrow: "Connected systems", headline: "Make every dependency visible.",
      copy: "Detect, configure, and troubleshoot the services that bridge flight software, tools, and data.", action: "Add integration",
      cards: [
        ["PX4 Autopilot", "Configuration available", "Local source tree and SITL target settings."],
        ["MAVSDK", "Not detected", "Telemetry SDK and port mapping for read-only vehicle data."],
        ["QGroundControl", "Not detected", "Ground station launch path and UDP connection settings."]
      ]
    }
  };

  const initialState = {
    activeView: "workspace",
    session: "stopped",
    vehicle: "Offline",
    altitude: "—",
    flightMode: "—",
    tools: toolCatalog,
    logs: [],
    filter: "all",
    sessionRun: 0
  };

  const state = restoreState();
  const shell = document.querySelector(".app-shell");
  const workspaceView = document.querySelector("#workspace-view");
  const secondaryView = document.querySelector("#secondary-view");
  const detailDialog = document.querySelector("#detail-dialog");
  let lastDialogTrigger = null;

  function restoreState() {
    try {
      const stored = JSON.parse(localStorage.getItem("drone-deck-design-state") || "null");
      if (!stored) return structuredClone(initialState);
      return {
        ...structuredClone(initialState),
        ...stored,
        session: "stopped",
        vehicle: "Offline",
        altitude: "—",
        flightMode: "—",
        tools: toolCatalog.map((tool) => ({ ...tool, ...(stored.tools || []).find((item) => item.id === tool.id), running: false }))
      };
    } catch (_error) {
      return structuredClone(initialState);
    }
  }

  function persistState() {
    try {
      localStorage.setItem("drone-deck-design-state", JSON.stringify({ activeView: state.activeView, tools: state.tools, logs: state.logs.slice(-80) }));
    } catch (_error) {
      // The prototype remains fully usable when storage is unavailable.
    }
  }

  function icon(name) {
    const paths = {
      arrow: '<path d="M5 12h14M13 6l6 6-6 6"/>',
      simulator: '<path d="M4 7h16v10H4zM8 21h8M12 17v4M8 11h.01M16 11h.01"/>',
      flight: '<path d="m12 2 3 7 7 3-7 3-3 7-3-7-7-3 7-3 3-7Z"/>',
      code: '<path d="m8 9-3 3 3 3M16 9l3 3-3 3M14 5l-4 14"/>',
      data: '<path d="M4 18V9m5 9V5m5 13v-6m5 6V3"/>',
      link: '<path d="M10 13a5 5 0 0 0 7.5.5l2-2a5 5 0 0 0-7-7l-1.2 1.2M14 11a5 5 0 0 0-7.5-.5l-2 2a5 5 0 0 0 7 7l1.2-1.2"/>'
    };
    return `<svg class="od-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths[name] || paths.arrow}</svg>`;
  }

  function render() {
    renderNavigation();
    if (state.activeView === "workspace") {
      workspaceView.hidden = false;
      secondaryView.hidden = true;
      document.querySelector("#view-title").textContent = "Workspace";
      document.querySelector("#breadcrumb").textContent = "Drone projects / Local lab";
      renderWorkspace();
    } else {
      workspaceView.hidden = true;
      secondaryView.hidden = false;
      renderSecondary(state.activeView);
    }
    updateGlobalActions();
    persistState();
  }

  function renderNavigation() {
    document.querySelectorAll("[data-view]").forEach((button) => {
      const active = button.dataset.view === state.activeView;
      button.classList.toggle("is-active", active);
      if (active) button.setAttribute("aria-current", "page");
      else button.removeAttribute("aria-current");
    });
  }

  function renderWorkspace() {
    renderStats();
    renderPipeline();
    renderTools();
    renderConsole();
    const startButton = document.querySelector('[data-action="start-session"]');
    startButton.disabled = state.session !== "stopped";
    startButton.textContent = state.session === "starting" ? "Starting workspace…" : state.session === "running" ? "Workspace running" : "Start SIH workspace";
  }

  function renderStats() {
    const runningCount = state.tools.filter((tool) => tool.running).length + (state.session === "running" ? 3 : 0);
    const stats = [
      ["Processes", String(runningCount).padStart(2, "0"), "Managed by this application"],
      ["Vehicle link", state.vehicle, "MAVSDK · read only"],
      ["Altitude", state.altitude, "Relative to takeoff"],
      ["Flight mode", state.flightMode, "Reported by PX4"]
    ];
    document.querySelector("#stats-grid").innerHTML = stats.map(([label, value, helper]) => `
      <article class="stat-card od-stat" style="--od-gap: 10px">
        <span class="eyebrow">${label}</span>
        <strong class="stat-value od-truncate">${value}</strong>
        <span class="stat-helper">${helper}</span>
      </article>`).join("");
  }

  function pipelineStatus(index) {
    if (state.session === "running") return ["Running", "is-running"];
    if (state.session === "starting") return index === 0 ? ["Starting", "is-starting"] : ["Queued", ""];
    return ["Stopped", ""];
  }

  function renderPipeline() {
    const steps = [
      ["01 / Flight stack", "PX4 + SIH", "Physics & flight controller"],
      ["02 / Transport", "UDP relay", "WSL → Windows · 19410"],
      ["03 / Visualizer", "jMAVSim", "Native Java · display only"]
    ];
    document.querySelector("#pipeline").innerHTML = steps.map((step, index) => {
      const status = pipelineStatus(index);
      const block = `<article class="pipeline-step${index === 0 ? " is-active" : ""}"><span class="pipeline-label">${step[0]}</span><h3>${step[1]}</h3><p>${step[2]}</p><span class="status-chip ${status[1]}"><span class="status-dot"></span>${status[0]}</span></article>`;
      return index < steps.length - 1 ? `${block}<span class="pipeline-arrow">${icon("arrow")}</span>` : block;
    }).join("");
  }

  function renderTools() {
    document.querySelector("#tool-list").innerHTML = state.tools.map((tool) => {
      const availability = tool.running ? ["Running", "badge-success"] : tool.installed ? ["Ready", "badge-success"] : ["Unchecked", "badge-neutral"];
      return `<article class="tool-row">
        <div class="tool-info"><span class="status-dot${tool.running ? " is-ready" : ""}"></span><div><h3>${tool.name}</h3><p class="od-clamp-2">${tool.description}</p></div></div>
        <div class="od-cluster tool-actions"><span class="badge ${availability[1]}">${availability[0]}</span><button class="button button-tertiary" type="button" data-tool-detail="${tool.id}">Details</button><button class="button ${tool.running ? "button-danger" : "button-primary"}" type="button" data-tool-toggle="${tool.id}"${!tool.installed ? " disabled" : ""}>${tool.running ? "Stop" : "Run"}</button></div>
      </article>`;
    }).join("");
  }

  function renderConsole() {
    const output = document.querySelector("#console-output");
    const entries = state.logs.filter((entry) => state.filter === "all" || entry.source === state.filter);
    if (!entries.length) {
      output.innerHTML = '<div class="console-empty">No matching process output yet.<br>Start a session or run a configured tool.</div>';
      return;
    }
    output.innerHTML = entries.map((entry) => `<div class="log-line ${entry.level ? `is-${entry.level}` : ""}"><span class="log-time">${entry.time}</span><span class="log-source">${entry.label}</span><span class="log-message">${entry.message}</span></div>`).join("");
    output.scrollTop = output.scrollHeight;
  }

  function renderSecondary(viewId) {
    const view = views[viewId];
    document.querySelector("#view-title").textContent = view.title;
    document.querySelector("#breadcrumb").textContent = `Drone projects / ${view.title}`;
    const iconName = viewId === "simulators" ? "simulator" : viewId === "flight" ? "flight" : viewId === "development" ? "code" : viewId === "ai" ? "data" : "link";
    secondaryView.innerHTML = `<div class="secondary-layout">
      <section class="secondary-hero"><div><span class="eyebrow accent-text">${view.eyebrow}</span><h2>${view.headline}</h2><p>${view.copy}</p></div><button class="button button-primary" type="button" data-secondary-action="${view.title}">${view.action}</button></section>
      <section class="feature-grid" aria-label="${view.title} overview">${view.cards.map((card) => `<article class="feature-card"><span class="feature-icon">${icon(iconName)}</span><div class="od-field" style="--od-gap: 6px"><span class="eyebrow">${card[1]}</span><h3>${card[0]}</h3></div><p>${card[2]}</p><button class="button button-tertiary" type="button" data-card-action="${card[0]}">View details</button></article>`).join("")}</section>
    </div>`;
  }

  function updateGlobalActions() {
    const stopButton = document.querySelector('[data-action="stop-session"]');
    stopButton.disabled = state.session === "stopped";
    document.querySelector("#footer-status").textContent = state.session === "running" ? "Local simulation active · telemetry bridge ready" : state.session === "starting" ? "Starting local simulation…" : "Preview mode · external processes simulated";
  }

  function addLog(source, label, message, level) {
    state.logs.push({ source, label, message, level: level || "", time: new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" }) });
    if (state.logs.length > 120) state.logs.shift();
    if (state.activeView === "workspace") renderConsole();
  }

  function startSession() {
    if (state.session !== "stopped") return;
    const runId = ++state.sessionRun;
    state.session = "starting";
    addLog("session", "SESSION", "Starting PX4 SIH workspace…", "warning");
    render();
    window.setTimeout(() => {
      if (runId !== state.sessionRun || state.session !== "starting") return;
      addLog("session", "PX4", "Flight stack ready; UDP relay bound to port 19410.", "success");
      state.session = "running";
      state.vehicle = "Connected";
      state.altitude = "0.0 m";
      state.flightMode = "Standby";
      render();
      toast("SIH workspace is ready");
    }, 1100);
  }

  function stopSession() {
    if (state.session === "stopped") return;
    state.sessionRun += 1;
    state.session = "stopped";
    state.vehicle = "Offline";
    state.altitude = "—";
    state.flightMode = "—";
    state.tools.forEach((tool) => { tool.running = false; });
    addLog("session", "SESSION", "Workspace stopped. Owned processes were released.", "warning");
    render();
    toast("Session stopped safely");
  }

  function detectTools() {
    addLog("session", "DETECT", "Scanning configured Windows and WSL paths…");
    state.tools.forEach((tool) => { tool.installed = true; });
    render();
    window.setTimeout(() => {
      addLog("session", "DETECT", "Three supported tools are ready.", "success");
      if (state.activeView === "workspace") renderTools();
      toast("Tool detection complete");
    }, 500);
  }

  function toggleTool(id) {
    const tool = state.tools.find((item) => item.id === id);
    if (!tool || !tool.installed) return;
    tool.running = !tool.running;
    addLog(tool.id, tool.name.toUpperCase().replace(/\s+/g, "-"), tool.running ? `${tool.name} started with ${tool.port}.` : `${tool.name} stopped.`, tool.running ? "success" : "warning");
    renderWorkspace();
    updateGlobalActions();
    persistState();
  }

  function showToolDetails(id, trigger) {
    const tool = state.tools.find((item) => item.id === id);
    if (!tool) return;
    lastDialogTrigger = trigger;
    document.querySelector("#dialog-eyebrow").textContent = "Session tool";
    document.querySelector("#dialog-title").textContent = tool.name;
    document.querySelector("#dialog-content").innerHTML = `<p>${tool.description}</p><dl class="detail-grid"><dt>Status</dt><dd>${tool.running ? "Running" : tool.installed ? "Ready" : "Not checked"}</dd><dt>Command</dt><dd><code>${tool.command}</code></dd><dt>Connection</dt><dd>${tool.port}</dd><dt>Ownership</dt><dd>Managed by Drone Deck for this session</dd></dl>`;
    detailDialog.showModal();
  }

  function showInfo(title, eyebrow, html, trigger) {
    lastDialogTrigger = trigger;
    document.querySelector("#dialog-eyebrow").textContent = eyebrow;
    document.querySelector("#dialog-title").textContent = title;
    document.querySelector("#dialog-content").innerHTML = html;
    detailDialog.showModal();
  }

  function toast(message) {
    const region = document.querySelector("#toast-region");
    const item = document.createElement("div");
    item.className = "toast";
    item.textContent = message;
    region.append(item);
    window.setTimeout(() => item.remove(), 3200);
  }

  function downloadFile(name, content, type) {
    const blob = new Blob([content], { type });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = name;
    document.body.append(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
  }

  function handleAction(action, trigger) {
    if (action === "open-sidebar") { setSidebar(true, true); }
    if (action === "close-sidebar") { setSidebar(false, true); }
    if (action === "start-session") startSession();
    if (action === "stop-session") stopSession();
    if (action === "detect-tools") detectTools();
    if (action === "clear-console") { state.logs = []; renderConsole(); persistState(); toast("Console cleared"); }
    if (action === "export-console") {
      const lines = state.logs.map((item) => `[${item.time}] ${item.label}: ${item.message}`).join("\n");
      downloadFile("drone-deck-session.log", lines || "No process output recorded.", "text/plain");
      toast("Console log exported");
    }
    if (action === "download-config") {
      downloadFile("drone-deck.local.json", JSON.stringify({ profile: "PX4 / SIH", target: "sihsim_quadx", transport: "udp", port: 19410 }, null, 2), "application/json");
      toast("Configuration package downloaded");
    }
  }

  document.addEventListener("click", (event) => {
    const viewButton = event.target.closest("[data-view]");
    if (viewButton) {
      state.activeView = viewButton.dataset.view;
      render();
      if (window.innerWidth <= 900) setSidebar(false, false);
      document.querySelector("#main-content").focus();
      return;
    }
    const actionButton = event.target.closest("[data-action]");
    if (actionButton) { handleAction(actionButton.dataset.action, actionButton); return; }
    const toolToggle = event.target.closest("[data-tool-toggle]");
    if (toolToggle) { toggleTool(toolToggle.dataset.toolToggle); return; }
    const toolDetail = event.target.closest("[data-tool-detail]");
    if (toolDetail) { showToolDetails(toolDetail.dataset.toolDetail, toolDetail); return; }
    const inspector = event.target.closest("[data-inspector]");
    if (inspector) {
      const content = inspector.dataset.inspector === "detect" ? "<p>Check the configured WSL path, build target, Java runtime, and UDP availability before starting the session.</p>" : inspector.dataset.inspector === "docs" ? "<p>Use the guided sequence to install dependencies, build PX4 SITL, verify jMAVSim, and confirm the transport port.</p>" : "<p>Manual configuration exposes the source path, launch command, target model, transport mode, and port mapping.</p>";
      showInfo(inspector.textContent.trim().replace(/^\d\s*/, ""), "PX4 / SIH setup", content, inspector);
      return;
    }
    const secondaryAction = event.target.closest("[data-secondary-action]");
    if (secondaryAction) { showInfo(secondaryAction.dataset.secondaryAction, "Planned workflow", "<p>This prototype keeps the next-step workflow local and reversible. Production integration should connect this action to the desktop app's native command layer.</p>", secondaryAction); return; }
    const cardAction = event.target.closest("[data-card-action]");
    if (cardAction) { showInfo(cardAction.dataset.cardAction, "Capability details", "<p>This area is structured for real project data, states, and commands once the native application source is connected.</p>", cardAction); }
  });

  document.querySelector("#console-filter").addEventListener("change", (event) => { state.filter = event.target.value; renderConsole(); });
  detailDialog.addEventListener("close", () => { if (lastDialogTrigger && document.contains(lastDialogTrigger)) lastDialogTrigger.focus(); });
  detailDialog.addEventListener("click", (event) => { if (event.target === detailDialog) detailDialog.close(); });

  function updateClock() {
    document.querySelector("#clock").textContent = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  updateClock();
  window.setInterval(updateClock, 30000);

  function setSidebar(open, moveFocus) {
    const isCompact = window.innerWidth <= 900;
    const expanded = isCompact && open;
    shell.dataset.sidebar = expanded ? "open" : "closed";
    shell.classList.toggle("is-drawer-open", expanded);
    document.querySelector(".menu-button").setAttribute("aria-expanded", String(expanded));
    if (!moveFocus) return;
    const focusTarget = expanded ? document.querySelector(".sidebar-close") : document.querySelector(".menu-button");
    if (focusTarget && focusTarget.offsetParent !== null) focusTarget.focus();
  }

  let compactLayout = window.innerWidth <= 900;
  setSidebar(!compactLayout, false);
  window.addEventListener("resize", () => {
    const nextCompactLayout = window.innerWidth <= 900;
    if (nextCompactLayout !== compactLayout) {
      compactLayout = nextCompactLayout;
      setSidebar(!compactLayout, false);
    }
  });
  render();
}());

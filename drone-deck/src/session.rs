//! Staged workspace launch: each profile starts only after the previous one reports
//! readiness (SIH `Ready`, relay `Forwarding`, others `Running`).
use crate::*;
pub(crate) struct Launch {
    stages: &'static [&'static str],
    stage: usize,
    started: Instant,
    requested: bool,
    /// Whole seconds of `started.elapsed()` last shown, so the wait redraws once a second.
    shown: u64,
}
impl Launch {
    pub(crate) fn new(stages: &'static [&'static str]) -> Self {
        Self {
            stages,
            stage: 0,
            started: Instant::now(),
            requested: false,
            shown: 0,
        }
    }
    /// The profile currently being started.
    pub(crate) fn current(&self) -> &'static str {
        self.stages[self.stage.min(self.stages.len().saturating_sub(1))]
    }
    pub(crate) fn stages(&self) -> &'static [&'static str] {
        self.stages
    }
    /// Index of the stage being started.
    pub(crate) fn stage(&self) -> usize {
        self.stage
    }
    /// Time since the current stage was requested.
    pub(crate) fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}
pub(crate) fn poll(s: &mut State) -> bool {
    let Some(mut launch) = s.session.take() else {
        return false;
    };
    if launch.stages.is_empty() {
        return true;
    }
    let id = launch.current();
    let Some(index) = s.config.profiles.iter().position(|p| p.id == id) else {
        s.notice = format!("Workspace needs the {id} profile. Configure it in Tools & config.");
        return true;
    };
    if !launch.requested {
        s.start(index);
        launch.requested = true;
        launch.started = Instant::now();
        s.session = Some(launch);
        return true;
    }
    let status = s.status.get(id).map(String::as_str).unwrap_or("");
    if status.starts_with("Exited")
        || ["Failed", "Blocked", "Configure command"].contains(&status)
        || s.readiness.get(id).is_some_and(|r| r == "Failed")
    {
        s.notice = format!(
            "Workspace stopped at {id}. Check its process output; later tools were not started."
        );
        activity::record(s, format!("Workspace stopped at {id}: {status}"));
        return true;
    }
    let ready = match id {
        "sih" => s.readiness.get(id).is_some_and(|r| r == "Ready"),
        "relay" => s.readiness.get(id).is_some_and(|r| r == "Forwarding"),
        _ => status == "Running",
    };
    if ready {
        if launch.stage + 1 >= launch.stages.len() {
            s.notice = if launch.stages.contains(&"jmavsim") {
                "SIH initialized; relay forwarding live state; visualizer process running. Verify its view. Flight readiness is shown in QGroundControl.".into()
            } else {
                format!(
                    "Workspace processes started ({}). Verify live data before relying on it.",
                    launch.stages.join(" → ")
                )
            };
        } else {
            launch.stage += 1;
            launch.requested = false;
            s.session = Some(launch);
        }
        return true;
    }
    if launch.started.elapsed() > Duration::from_secs(60) {
        s.notice = format!(
            "No readiness confirmation from {id} within 60 seconds. Check ports and logs; existing processes were preserved."
        );
        return true;
    }
    let secs = launch.started.elapsed().as_secs();
    let tick = secs != launch.shown;
    launch.shown = secs;
    s.session = Some(launch);
    tick
}

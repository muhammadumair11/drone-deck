//! Staged workspace launch: each stage starts only after the previous one reports readiness.
//! Profiles are ready on SIH `Ready` or, for the others, `Running`; the virtual stages `router`
//! and `link` (no process of their own) are started and checked by `vehicle::stage`.
use crate::vehicle::{Stage, Step};
use crate::*;
pub(crate) struct Launch {
    stages: Vec<&'static str>,
    stage: usize,
    started: Instant,
    requested: bool,
    /// Whole seconds of `started.elapsed()` last counted, so the wait redraws once a second
    /// while the System page shows it.
    shown: u64,
}
impl Launch {
    pub(crate) fn new(stages: impl Into<Vec<&'static str>>) -> Self {
        Self {
            stages: stages.into(),
            stage: 0,
            started: Instant::now(),
            requested: false,
            shown: 0,
        }
    }
    /// The stage currently being started.
    pub(crate) fn current(&self) -> &'static str {
        self.stages[self.stage.min(self.stages.len().saturating_sub(1))]
    }
    pub(crate) fn stages(&self) -> &[&'static str] {
        &self.stages
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

/// Ends the launch at `id` with a specific message.
fn stopped(s: &mut State, id: &str, why: &str) -> bool {
    s.notice = format!(
        "Workspace stopped at {id}: {why} Check its process output; later tools were not started."
    );
    activity::record(s, format!("Workspace stopped at {id}: {why}"));
    s.launch_failure = Some((id.to_string(), why.to_string()));
    true
}

/// The system the launch runs for.
fn launch_owner(s: &State) -> String {
    s.systems
        .owner("session")
        .unwrap_or(s.systems.active_id())
        .to_string()
}

/// Requests stage `id`: `Ok(false)` while the Windows simulator port is still being checked
/// off the UI thread (asked again on a later frame); `Err` ends the launch with that reason.
fn request(s: &mut State, id: &'static str) -> Result<bool, String> {
    match vehicle::port_check(s, id) {
        vehicle::Port::Checking => return Ok(false),
        vehicle::Port::Taken(why) => return Err(why),
        vehicle::Port::Free => {}
    }
    if let Some(stage) = vehicle::stage(s, id, Step::Start) {
        return match stage {
            Stage::Failed(e) => Err(e),
            _ => Ok(true),
        };
    }
    let Some(index) = s.config.profiles.iter().position(|p| p.id == id) else {
        return Err(format!(
            "the {id} profile is missing. Configure it in Tools & config."
        ));
    };
    if let Some(conflict) = vehicle::simulator_conflict(s, id, &launch_owner(s)) {
        return Err(conflict);
    }
    s.start(index);
    Ok(true)
}

/// Whether stage `id` is ready; `Err` when it failed.
fn ready(s: &mut State, id: &str) -> Result<bool, String> {
    if let Some(stage) = vehicle::stage(s, id, Step::Ready) {
        return match stage {
            Stage::Ready => Ok(true),
            Stage::Waiting => Ok(false),
            Stage::Failed(e) => Err(e),
        };
    }
    let status = s.status.get(id).map(String::as_str).unwrap_or("");
    if status.starts_with("Exited")
        || ["Failed", "Blocked", "Configure command"].contains(&status)
        || s.readiness.get(id).is_some_and(|r| r == "Failed")
    {
        return Err(format!("{status}."));
    }
    Ok(match id {
        "sih" => s.readiness.get(id).is_some_and(|r| r == "Ready"),
        _ => status == "Running",
    })
}

pub(crate) fn poll(s: &mut State) -> bool {
    let Some(mut launch) = s.session.take() else {
        return false;
    };
    if launch.stages.is_empty() {
        return true;
    }
    let id = launch.current();
    if !launch.requested {
        match request(s, id) {
            Err(e) => return stopped(s, id, &e),
            Ok(false) => {
                s.session = Some(launch);
                return false;
            }
            Ok(true) => {}
        }
        launch.requested = true;
        launch.started = Instant::now();
        s.session = Some(launch);
        return true;
    }
    match ready(s, id) {
        Err(e) => return stopped(s, id, &e),
        Ok(true) => {
            if launch.stage + 1 >= launch.stages.len() {
                s.notice = if launch.stages.contains(&"link") {
                    format!(
                        "Workspace started ({}). The vehicle link is up; check the live view for a fresh heartbeat before relying on it.",
                        launch.stages.join(" → ")
                    )
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
        Ok(false) => {}
    }
    if launch.started.elapsed() > Duration::from_secs(60) {
        let detail = match id {
            "link" | "router" => s
                .vehicle
                .error
                .clone()
                .map_or_else(String::new, |e| format!(" Last error: {e}.")),
            _ => String::new(),
        };
        s.notice = format!(
            "No readiness confirmation from {id} within 60 seconds. Check ports and logs; existing processes were preserved.{detail}"
        );
        return true;
    }
    // The seconds count is drawn on the System page only (`launch_progress`).
    let secs = launch.started.elapsed().as_secs();
    let tick = secs != launch.shown && s.page == crate::ui::Page::System;
    launch.shown = secs;
    s.session = Some(launch);
    tick
}

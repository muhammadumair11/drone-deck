//! Existing operations dispatched by the first-run task identity.
use super::*;
fn status(s: &State, id: &str) -> String {
    s.status.get(id).cloned().unwrap_or_default()
}
fn vehicle_up(s: &State) -> bool {
    systems::vehicle_profile(s.systems.current()).is_some_and(|id| systems::adapter::ready(s, id))
}
fn ros_up(s: &State) -> bool {
    let sys = s.systems.current();
    !sys.simulated()
        || (vehicle_up(s)
            && (!sys.consumers.ros.agent
                || sys
                    .adapter()
                    .and_then(|a| a.ros.as_ref())
                    .is_some_and(|ros| status(s, ros.agent) == "Running")))
}
fn topics(s: &State) -> Vec<String> {
    s.recorder
        .discovered
        .iter()
        .map(|t| &t.name)
        .filter(|n| !["/rosout", "/parameter_events", "/clock", "/tf_static"].contains(&n.as_str()))
        .filter(|n| !n.split('/').any(|part| part.starts_with('_')))
        .cloned()
        .collect()
}
fn recording(s: &State) -> Option<&crate::recordings::Session> {
    let id = s.recorder.selected.as_ref()?;
    s.recorder.sessions.iter().find(|r| &r.id == id)
}
fn data_page(s: &mut State, tab: usize) {
    s.ui.data_tab = tab;
    s.navigate(ui::Page::Data);
    s.notice = "Complete the task here, then return through Guided setup to confirm it.".into();
}
pub(super) fn request(s: &mut State, task: Task) -> bool {
    use Task::*;
    match task {
        Start => {
            s.vehicle.owned_by(s.systems.active_id())
                || s.bundle(systems::session_stages(s.systems.current()))
                || (s.session.is_some()
                    && s.systems.owner("session") == Some(s.systems.active_id()))
        }
        GroundStation => {
            let Some(i) = s.config.profiles.iter().position(|p| p.id == "qgc") else {
                s.notice = "Configure QGroundControl in Tools & config.".into();
                return false;
            };
            s.start(i);
            if ["Starting", "Running"].contains(&status(s, "qgc").as_str()) {
                s.guided.launched = Some("qgc".into());
                true
            } else {
                false
            }
        }
        LiveView => s.vehicle.owned_by(s.systems.active_id()),
        Ros => {
            if s.systems.current().simulated() && !ros_up(s) {
                s.bundle(systems::session_stages(s.systems.current()));
            }
            s.recorder.capture_domain =
                TextEdit::new(s.systems.current().consumers.ros.domain.to_string());
            s.recorder.discovered.clear();
            crate::recordings::send(s, if s.recorder.ready { "topics" } else { "check" });
            s.guided.last_query = Some(Instant::now());
            true
        }
        Record => {
            if s.recorder.recording {
                s.guided.prep_error =
                    "Stop the existing recording before starting this task.".into();
                return false;
            }
            if s.recorder.topics.text.trim().is_empty() {
                s.recorder.topics = TextEdit::new(topics(s).join(" "));
            }
            crate::recordings::send(s, "record");
            s.recorder.pending
        }
        Finalize => {
            crate::recordings::send(s, if s.recorder.recording { "stop" } else { "list" });
            s.guided.last_query = Some(Instant::now());
            true
        }
        Recording => {
            if let systems::Vehicle::Replay {
                recording: Some(id),
            } = &s.systems.current().vehicle
            {
                s.recorder.selected = Some(id.clone());
            }
            crate::recordings::send(s, "list");
            s.guided.last_query = Some(Instant::now());
            true
        }
        Playback => {
            crate::recordings::send(s, "replay");
            s.recorder.pending
        }
        Inputs | Project => {
            data_page(s, 3);
            true
        }
        Infer => {
            data_page(s, 2);
            crate::studio::pipelines::refresh(s);
            true
        }
        Endpoint => {
            let e = s.systems.current().endpoint().unwrap_or("");
            if e.is_empty() || !systems::endpoint_ok(e) {
                s.guided.prep_error = "Choose a valid endpoint in Vehicle, then save again.".into();
                false
            } else {
                s.hardware.endpoint = TextEdit::new(e);
                true
            }
        }
        Connect => {
            if !crate::hardware::connected(s) {
                s.hardware.mode = 1;
                crate::hardware::start(s);
            }
            crate::hardware::connected(s)
        }
        Logs => {
            if s.vehicle.tlog().is_some() {
                s.open(crate::vehicle::tlog_dir().display().to_string());
            }
            true
        }
    }
}
pub(super) fn observe(s: &State, task: Task) -> Option<String> {
    use Task::*;
    match task {
        Start if !systems::session_stages(s.systems.current()).contains(&"link") => {
            vehicle_up(s).then(|| "Simulator adapter reports ready.".into())
        }
        Start | LiveView | Connect => fresh(s),
        GroundStation => {
            let st = status(s, "qgc");
            (s.guided.launched.as_deref() == Some("qgc")
                && ["Running", "Exited 0"].contains(&st.as_str()))
            .then(|| {
                format!("QGroundControl launched ({st}); vehicle appearance confirmed by you.")
            })
        }
        Ros => {
            let names = topics(s);
            (ros_up(s) && !names.is_empty())
                .then(|| format!("{} application topics: {}", names.len(), names.join(", ")))
        }
        Record => s
            .recorder
            .recording
            .then(|| format!("Recording {}", s.recorder.topics.text)),
        Finalize | Recording => {
            if s.recorder.recording {
                return None;
            }
            let r = recording(s).filter(|r| r.status == "Finalized" && r.messages > 0)?;
            if task == Finalize && s.guided.run_recording.as_ref() != Some(&r.id) {
                return None;
            }
            Some(format!(
                "Recording {}: {} messages, Finalized",
                r.id, r.messages
            ))
        }
        Playback => (s.recorder.playing || s.guided.playback_seen)
            .then(|| "Recorded topics played on the isolated replay domain.".into()),
        Inputs => {
            let (m, i) = (s.studio.model_file.text.trim(), s.studio.inputs.text.trim());
            (!m.is_empty() && !i.is_empty()).then(|| format!("Model {m}; inputs {i}"))
        }
        Project => {
            let id = s
                .guided
                .launched
                .as_deref()
                .filter(|id| id.starts_with("project-"))?;
            (status(s, id) == "Exited 0").then(|| format!("{id} completed successfully"))
        }
        Infer => {
            if s.studio.pipes.job.is_some() {
                return None;
            }
            let r = s
                .studio
                .pipes
                .inference_runs()
                .find(|r| r.status == "Completed" && !s.guided.prior_runs.contains(&r.name))?;
            Some(format!(
                "Completed inference run {}: {}",
                r.name,
                r.detections
                    .map_or("no detection count recorded".into(), |n| format!(
                        "{n} detection messages"
                    ))
            ))
        }
        Endpoint => {
            let e = s.systems.current().endpoint()?;
            (!e.is_empty() && systems::endpoint_ok(e)).then(|| format!("Endpoint {e}"))
        }
        Logs => s
            .vehicle
            .tlog()
            .map(|p| format!("Telemetry log {}", p.display())),
    }
}
fn fresh(s: &State) -> Option<String> {
    let snap = s.vehicle.shown_for(s.systems.active_id())?;
    let v = snap.vehicle.as_ref().filter(|_| snap.fresh)?;
    Some(format!(
        "Fresh {} vehicle {}:{}; mode {}",
        v.autopilot.name(),
        v.system_id,
        v.component_id,
        v.mode
    ))
}

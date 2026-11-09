use crate::*;
pub(crate) struct Launch {
    stage: usize,
    started: Instant,
    requested: bool,
}
impl Default for Launch {
    fn default() -> Self {
        Self {
            stage: 0,
            started: Instant::now(),
            requested: false,
        }
    }
}
pub(crate) fn poll(s: &mut State) -> bool {
    let Some(mut launch) = s.session.take() else {
        return false;
    };
    let id = ["sih", "relay", "jmavsim"][launch.stage];
    let Some(index) = s.config.profiles.iter().position(|p| p.id == id) else {
        s.notice = format!("Workspace needs the {id} profile. Open Setup to configure it.");
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
            "Workspace stopped at {id}. Check its console output; later tools were not started."
        );
        return true;
    }
    let ready = match id {
        "sih" => s.readiness.get(id).is_some_and(|r| r == "Ready"),
        "relay" => s.readiness.get(id).is_some_and(|r| r == "Forwarding"),
        _ => status == "Running",
    };
    if ready {
        if launch.stage == 2 {
            s.notice="SIH initialized; relay forwarding live state; visualizer process running. Verify its view. Flight readiness is shown in QGroundControl.".into();
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
    s.session = Some(launch);
    false
}

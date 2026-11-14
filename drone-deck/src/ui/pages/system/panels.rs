//! The System page is built from the active system's pieces: one panel for each piece it has,
//! none for a piece it doesn't. `panels` decides which, from the system alone; each panel's
//! view lives with the feature it shows. Adding a listener is one `Panel` and one line here.
use super::*;
use systems::System;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Panel {
    /// The connection diagram with every configured branch, and the addresses to copy.
    Connections,
    /// The vehicle: a simulator with its flight controls, a real drone's read-only link, or
    /// the recording a replay system plays.
    Vehicle,
    LiveView,
    GroundStation,
    Scripts,
    Ros,
    Recorder,
    Foxglove,
    Inference,
    /// This system's processes and their output.
    Processes,
}

/// The panels `sys` shows, in page order.
pub(crate) fn panels(sys: &System) -> Vec<Panel> {
    let c = &sys.consumers;
    let live = !sys.is_replay();
    let mut out = vec![Panel::Connections, Panel::Vehicle];
    if live && c.live_view {
        out.push(Panel::LiveView);
    }
    if live && c.qgc {
        out.push(Panel::GroundStation);
    }
    if live && c.scripts_udp.is_some() {
        out.push(Panel::Scripts);
    }
    if c.ros.agent || (live && c.ros.recorder) {
        out.push(Panel::Ros);
    }
    if c.ros.recorder {
        out.push(Panel::Recorder);
    }
    if c.foxglove {
        out.push(Panel::Foxglove);
    }
    if c.inference.is_some() {
        out.push(Panel::Inference);
    }
    out.push(Panel::Processes);
    out
}

/// The listeners `sys` does not use, in words, for the line under the panels.
pub(crate) fn unused(sys: &System) -> Vec<&'static str> {
    let shown = panels(sys);
    [
        (Panel::LiveView, "live view"),
        (Panel::GroundStation, "QGroundControl"),
        (Panel::Scripts, "MAVSDK scripts"),
        (Panel::Ros, "ROS 2"),
        (Panel::Recorder, "Recorder"),
        (Panel::Foxglove, "Foxglove"),
        (Panel::Inference, "inference"),
    ]
    .into_iter()
    .filter(|(p, _)| !shown.contains(p))
    .map(|(_, name)| name)
    .collect()
}

/// One panel's view.
pub(super) fn view(s: &State, vp: &Viewport, panel: Panel) -> El {
    match panel {
        Panel::Connections => col()
            .key("panel-connections")
            .w_full()
            .min_w(0.)
            .child(topology::section(s, vp))
            .child(details::section(s, vp)),
        Panel::Vehicle => vehicle(s, vp),
        Panel::LiveView => vehicle::view::live(s, vp),
        Panel::GroundStation => listeners::ground_station(s, vp),
        Panel::Scripts => listeners::scripts(s, vp),
        Panel::Ros => listeners::ros(s, vp),
        Panel::Recorder => listeners::recorder(s, vp),
        Panel::Foxglove => listeners::foxglove(s, vp),
        Panel::Inference => listeners::inference(s, vp),
        Panel::Processes => runtime::section(s, vp),
    }
}

/// The vehicle panel: a simulator Drone Deck started (flight controls, and the console when
/// the autopilot has one), a real drone (its read-only link and tools; never flight
/// controls) or a recording (its replay).
fn vehicle(s: &State, vp: &Viewport) -> El {
    let sys = s.systems.current();
    let mut c = col().key("panel-vehicle").w_full().min_w(0.);
    if sys.is_physical() {
        return c
            .child(hardware::monitor(s, vp))
            .child(vehicle::view::tools(s, vp));
    }
    if sys.is_replay() {
        return c.child(crate::recordings::replay_panel(s, vp));
    }
    if systems::compat::may_command(sys) {
        c = c.child(vehicle::view::actions(s, vp));
        if sys.adapter().is_some_and(|a| a.shell) {
            c = c.child(vehicle::view::console(s, vp));
        }
    }
    c
}

#[cfg(test)]
#[path = "panels_tests.rs"]
mod tests;

/// Headless scenes of the workspace for the mockup combinations (`--scene workspace-…`).
pub(crate) fn scene(s: &mut State, combo: &str) {
    use crate::systems::model::{Autopilot, World, starter};
    use crate::systems::{AI, PHYSICAL, ROS, SIMULATION, Vehicle};
    let mut sys = match combo {
        "gazebo" => starter(ROS, "", "Gazebo camera lab"),
        "real" => starter(PHYSICAL, "", "Bench quad"),
        "recording" => starter(AI, "", "Detector bench"),
        _ => starter(SIMULATION, "", ""),
    };
    let c = &mut sys.consumers;
    match combo {
        "jmavsim" => {
            sys.vehicle = Vehicle::Simulated {
                autopilot: Autopilot::Px4,
                world: World::new("jmavsim", &[]),
            };
            c.qgc = false;
            c.scripts_udp = None;
            c.ros.agent = true;
            c.ros.recorder = true;
            c.ros.topics = vec![
                "/fmu/out/vehicle_status_v1".into(),
                "/camera/image_raw".into(),
            ];
            sys.name = sys.suggested_name();
        }
        "gazebo" => c.live_view = true,
        "real" => {
            sys.vehicle = Vehicle::physical("serial:COM7:57600");
            c.qgc = true;
            c.scripts_udp = Some("127.0.0.1:14540".into());
        }
        "ardupilot" => {
            sys.vehicle = Vehicle::ardupilot("copter");
            c.live_view = false;
            c.qgc = false;
            sys.name = "ArduCopter + MAVSDK".into();
        }
        _ => {}
    }
    sys.prepared = true;
    let id = s.systems.add(sys);
    s.select_system(&id);
    crate::systems::follow_active(s);
    s.navigate(Page::System);
}

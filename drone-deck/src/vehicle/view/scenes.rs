//! Headless scenes of the vehicle panels, built on `vehicle::preview` (no network, no
//! process). Each fixture is a fake snapshot; the status bar says the app is in preview.
use crate::mavlink::{CommandKind, CommandResult, SimulationEvidence, StatusText};
use crate::systems::{self, Source};
use crate::{State, TextEdit, vehicle};
use std::time::{Duration, Instant, SystemTime};

const EARTH_M: f64 = 6_371_000.;

/// Corners of a survey-like loop in metres east / north of home; the vehicle ends at the last.
const PATH: [[f32; 2]; 7] = [
    [0., 0.],
    [0., 6.],
    [32., 6.],
    [32., 38.],
    [-12., 38.],
    [-12., 16.],
    [18., 22.],
];

fn simulation(s: &mut State) -> String {
    if !s.systems.current().source.simulated() {
        let id = s
            .systems
            .list
            .iter()
            .find(|x| matches!(x.source, Source::Sih))
            .map(|x| x.id.clone());
        if let Some(id) = id {
            s.select_system(&id);
        }
    }
    s.systems.active_id().to_string()
}

fn run_stack(s: &mut State) {
    for id in ["sih", "qgc"] {
        s.systems.claim(id);
        s.status.insert(id.into(), "Running".into());
    }
    s.readiness.insert("sih".into(), "Ready".into());
}

/// Fills the position trace along `PATH`, with home at its first corner.
fn fly(s: &mut State) {
    let Some(v) = s.vehicle.snapshot.as_mut().and_then(|x| x.vehicle.as_mut()) else {
        return;
    };
    let (Some(lat), Some(lon)) = (v.lat, v.lon) else {
        return;
    };
    let end = PATH[PATH.len() - 1];
    let lat0 = lat - (end[1] as f64 / EARTH_M).to_degrees();
    let lon0 = lon - (end[0] as f64 / (EARTH_M * lat.to_radians().cos())).to_degrees();
    let to_fix = |p: [f32; 2]| {
        (
            lat0 + (p[1] as f64 / EARTH_M).to_degrees(),
            lon0 + (p[0] as f64 / (EARTH_M * lat0.to_radians().cos())).to_degrees(),
        )
    };
    v.home = Some((
        lat0,
        lon0,
        v.alt_msl.unwrap_or(0.) - v.relative_alt.unwrap_or(0.),
    ));
    let track = &mut s.vehicle.ui.track;
    track.clear();
    for pair in PATH.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let len = ((b[0] - a[0]).hypot(b[1] - a[1])).max(1.);
        let steps = len.ceil() as usize;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let (la, lo) = to_fix([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            track.push_fix(la, lo);
        }
    }
    let (la, lo) = to_fix(end);
    v.lat = Some(la);
    v.lon = Some(lo);
}

fn record(s: &mut State, id: u64, kind: CommandKind, result: Option<CommandResult>, ago: u64) {
    let sent = Instant::now()
        .checked_sub(Duration::from_secs(ago))
        .unwrap_or_else(Instant::now);
    s.vehicle.commands.push_back(vehicle::CommandRecord {
        id,
        kind,
        result,
        sent,
    });
}

fn live(s: &mut State, fresh: bool) {
    let id = simulation(s);
    run_stack(s);
    vehicle::preview(s, &id, fresh, true);
    fly(s);
}

/// `vehicle-live`: a hovering PX4 quad with its trace.
pub(crate) fn scene_live(s: &mut State) {
    live(s, true);
}

/// `vehicle-stale`: the same vehicle, silent for 9 s.
pub(crate) fn scene_stale(s: &mut State) {
    live(s, false);
}

/// `vehicle-actions`: on the ground, disarmed, pre-arm checks passing, the mode list open and
/// the last results.
pub(crate) fn scene_actions(s: &mut State) {
    live(s, true);
    if let Some(v) = s.vehicle.snapshot.as_mut().and_then(|x| x.vehicle.as_mut()) {
        v.armed = false;
        v.landed_state = 1;
        v.relative_alt = Some(0.0);
        v.groundspeed = Some(0.0);
        v.mode = "Hold".into();
        let prearm = super::health::PREARM_BIT;
        v.sensors_present |= prearm;
        v.sensors_enabled |= prearm;
        v.sensors_health |= prearm;
    }
    record(
        s,
        1,
        CommandKind::SetMode("Hold".into()),
        Some(CommandResult::Accepted),
        42,
    );
    record(
        s,
        2,
        CommandKind::Takeoff { alt: 10. },
        Some(CommandResult::Rejected {
            result: 1,
            reason: "Temporarily rejected",
        }),
        30,
    );
    record(s, 3, CommandKind::Disarm, Some(CommandResult::Accepted), 12);
    s.vehicle.status.push_back(StatusText {
        severity: 3,
        text: "Takeoff denied: disarmed".into(),
        at: SystemTime::now(),
    });
    s.ui.open.insert("flight-modes".into());
}

/// `vehicle-console`: the console open with some PX4 shell output and history.
pub(crate) fn scene_console(s: &mut State) {
    live(s, true);
    s.ui.open.insert(super::console::KEY.into());
    s.vehicle.console = concat!(
        "nsh> ver all\n",
        "HW arch: PX4_SITL\n",
        "PX4 git-branch: main\n",
        "OS: Linux\n",
        "nsh> commander status\n",
        "INFO  [commander] Armed\n",
        "INFO  [commander] navigation mode: Hold\n",
        "nsh> listener vehicle_status -n 1\n",
        "TOPIC: vehicle_status\n",
        "    arming_state: 2\n",
        "    nav_state: 4\n",
        "nsh> ",
    )
    .into();
    for line in [
        "ver all",
        "commander status",
        "listener vehicle_status -n 1",
    ] {
        s.vehicle.ui.history.push_back(line.into());
    }
    s.vehicle.ui.console_input = TextEdit::new("param show MPC_XY_VEL_MAX");
}

/// `vehicle-gated`: a simulated link without evidence of an owned, running stack: actions and
/// the console are disabled with the reason.
pub(crate) fn scene_gated(s: &mut State) {
    let id = simulation(s);
    vehicle::preview(s, &id, true, false);
    s.vehicle.preview = Some(SimulationEvidence {
        app_router_link: true,
        simulator_source: true,
        stack_owned_running: false,
    });
    fly(s);
    s.ui.open.insert(super::console::KEY.into());
}

/// `vehicle-readonly`: a physical vehicle: live view and health, no actions, no console.
pub(crate) fn scene_readonly(s: &mut State) {
    if let Some(id) = s
        .systems
        .list
        .iter()
        .find(|x| matches!(x.source, Source::Physical { .. }))
        .map(|x| x.id.clone())
    {
        s.select_system(&id);
    } else {
        s.select_system(systems::PHYSICAL);
    }
    let id = s.systems.active_id().to_string();
    vehicle::preview(s, &id, true, false);
    fly(s);
}

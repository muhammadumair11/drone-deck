//! The vehicle on the System page: the live view (`live`: attitude, heading, position trace,
//! readouts, preflight and health), the flight actions bar and the console for an app-owned
//! simulation (`actions`, `console`), and the physical-vehicle tools (`tools`). Everything
//! reads `State::vehicle` and shows data only for the system that owns the link; stale values
//! are greyed out and labelled, never shown as live.
use crate::mavlink::{CommandKind, LinkState, VehicleSnapshot, VehicleState};
use crate::ui::{kit::*, theme::*};
use crate::{State, TextEdit};
use rune::{El, col, row};
use std::collections::VecDeque;

mod actions;
mod console;
mod health;
mod instruments;
mod scenes;
#[cfg(test)]
mod tests;
mod tools;
mod track;

pub(crate) use actions::{actions, ask};
pub(crate) use console::console;
pub(crate) use scenes::{
    scene_actions, scene_ardupilot_console, scene_console, scene_gated, scene_health_checks,
    scene_launching, scene_link_retrying, scene_live, scene_readonly, scene_stale,
};
pub(crate) use tools::tools;
pub(crate) use track::Track;

/// What the vehicle panels keep between frames (`Vehicle::ui`).
pub(crate) struct VehicleUi {
    pub takeoff_alt: TextEdit,
    /// Arm or Takeoff waiting for the confirmation dialog.
    pub pending: Option<CommandKind>,
    pub console_input: TextEdit,
    /// Console commands sent, oldest first (at most `console::HISTORY`).
    pub history: VecDeque<String>,
    /// Where Up / Down stand in `history`; `None` is the line being typed.
    pub history_at: Option<usize>,
    /// Positions since the link came up.
    pub track: Track,
    /// The heartbeat age (s, capped at `AGE_CAP + 1`) last drawn, so the age label ticks
    /// once a second while it is on screen; `None` while it isn't.
    shown_age: Option<u64>,
}

impl Default for VehicleUi {
    fn default() -> Self {
        Self {
            takeoff_alt: TextEdit::new("10"),
            pending: None,
            console_input: TextEdit::new(""),
            history: VecDeque::new(),
            history_at: None,
            track: Track::default(),
            shown_age: None,
        }
    }
}

/// Beyond this many seconds of silence the age reads "over a minute" and stops ticking, so a
/// vehicle that went quiet doesn't redraw the window every second forever.
pub(crate) const AGE_CAP: u64 = 60;

/// Seconds since the vehicle's last heartbeat.
pub(crate) fn heartbeat_age(v: &VehicleState) -> u64 {
    v.last_heartbeat.elapsed().as_secs()
}

/// How long the vehicle has been silent, as drawn: "9 s", or "over a minute" past
/// [`AGE_CAP`].
pub(crate) fn silence(v: &VehicleState) -> String {
    match heartbeat_age(v) {
        age if age > AGE_CAP => "over a minute".into(),
        age => format!("{age} s"),
    }
}

/// Folds the latest snapshot into the position trace and ticks the heartbeat age. True when
/// something drawn changed. Called from `vehicle::poll`. The age ticks only while it is on
/// screen: the System page of the system that owns the link (the live view, or a physical
/// system's connection panel), which an AI system's page does not show.
pub(crate) fn follow(s: &mut State) -> bool {
    let drawn = s.page == crate::ui::Page::System
        && s.systems.active != crate::systems::AI
        && s.vehicle.owner.as_deref() == Some(s.systems.active_id());
    let v = &mut s.vehicle;
    let Some(snap) = v.snapshot.as_ref() else {
        // No link: the next one starts a new trace.
        let changed = !v.ui.track.is_empty() || v.ui.shown_age.is_some();
        v.ui.track.clear();
        v.ui.shown_age = None;
        return changed;
    };
    let mut changed = false;
    if let Some(vehicle) = &snap.vehicle {
        changed |= v.ui.track.push(vehicle, snap.fresh);
        if drawn {
            let age = Some(heartbeat_age(vehicle).min(AGE_CAP + 1));
            if v.ui.shown_age != age {
                v.ui.shown_age = age;
                changed = true;
            }
        } else {
            v.ui.shown_age = None;
        }
    }
    changed
}

/// "Live link belongs to …" when another system owns the link, else `None`.
pub(crate) fn owner_note(s: &State) -> Option<String> {
    let owner = s.vehicle.owner.as_deref()?;
    // A link that is still connecting has no snapshot yet but already belongs to its owner;
    // a headless preview has a snapshot and no link.
    let open = s.vehicle.link.is_some() || s.vehicle.snapshot.is_some();
    if owner == s.systems.active_id() || !open {
        return None;
    }
    let name = s.systems.get(owner).map_or(owner, |x| x.label());
    Some(format!(
        "Live link belongs to {name}. Its live view and actions are on that system's page."
    ))
}

fn reading(key: &str, label: &str, value: String) -> El {
    col()
        .key(format!("reading-{key}"))
        .w(150.)
        .min_w(0.)
        .gap(6.)
        .child(meta_sized(label, 9., muted()))
        .child(meta_sized(value, 12., fg()).wrap())
}

fn opt<T>(value: Option<T>, show: impl Fn(T) -> String) -> String {
    value.map_or_else(|| "Not reported".into(), show)
}

/// Messages per second over the link, from the vehicle's message rates.
pub(crate) fn link_rate(v: &VehicleState) -> f32 {
    v.message_rates.iter().map(|(_, r)| r).sum()
}

fn readings(snap: &VehicleSnapshot, v: &VehicleState, fresh: bool) -> El {
    let battery = match (v.battery_voltage, v.battery_remaining) {
        (None, None) => "Not reported".into(),
        (b, p) => format!(
            "{} / {}",
            opt(b, |b| format!("{b:.1} V")),
            opt(p, |p| format!("{p} %"))
        ),
    };
    let r = row()
        .key("vehicle-readings")
        .w_full()
        .min_w(0.)
        .gaps(18., 24.)
        .flex_wrap()
        .py(18.)
        .border_sides([1., 0., 1., 0.], soft())
        .child(reading("mode", "Mode", v.mode.clone()))
        .child(reading(
            "armed",
            "Armed",
            if v.armed { "Armed" } else { "Disarmed" }.into(),
        ))
        .child(reading(
            "alt-rel",
            "Altitude rel",
            opt(v.relative_alt, |a| format!("{a:.1} m")),
        ))
        .child(reading(
            "alt-msl",
            "Altitude MSL",
            opt(v.alt_msl, |a| format!("{a:.1} m")),
        ))
        .child(reading(
            "speed",
            "Ground speed",
            opt(v.groundspeed, |g| format!("{g:.1} m/s")),
        ))
        .child(reading(
            "climb",
            "Climb",
            opt(v.climb, |c| format!("{c:+.1} m/s")),
        ))
        .child(reading(
            "heading",
            "Heading",
            opt(v.heading, |h| format!("{:03.0}°", h.rem_euclid(360.))),
        ))
        .child(reading("battery", "Battery", battery))
        .child(reading(
            "gps",
            "GPS",
            format!(
                "{} / {} / eph {}",
                v.gps_fix_name(),
                opt(v.satellites, |n| format!("{n} sats")),
                opt(v.eph, |e| format!("{e:.1} m"))
            ),
        ))
        .child(reading(
            "rate",
            "Link rate",
            if fresh {
                format!("{:.0} msg/s", link_rate(v))
            } else {
                "0 msg/s".into()
            },
        ))
        .child(reading(
            "heartbeat",
            "Last heartbeat",
            format!("{} ago", silence(v)),
        ))
        .child(reading(
            "vehicle",
            "Vehicle",
            format!(
                "{} {} / sys {} / {} component{} seen",
                v.autopilot.name(),
                v.vehicle_type_name(),
                v.system_id,
                snap.systems.len(),
                if snap.systems.len() == 1 { "" } else { "s" }
            ),
        ));
    if fresh { r } else { r.dim(0.45) }
}

/// The freshness line: fresh heartbeat, or how long it has been silent.
pub(crate) fn freshness(snap: &VehicleSnapshot, v: &VehicleState) -> (String, bool) {
    let age = silence(v);
    match &snap.link.state {
        LinkState::Down(reason) => (
            format!("Link down: {reason}. Retrying / last values are not live"),
            false,
        ),
        _ if snap.fresh => (format!("Heartbeat fresh / {age} ago"), true),
        _ => (
            format!("No heartbeat for {age} / last received values, not live"),
            false,
        ),
    }
}

fn instruments(s: &State, v: &VehicleState, fresh: bool, vp: &Viewport) -> El {
    let size = if vp.compact() { 160. } else { 180. };
    let dial = col()
        .key("vehicle-dial")
        .gap(10.)
        .child(instruments::attitude(v.roll, v.pitch, size))
        .child(instruments::heading_tape(v.heading, size));
    // The trace picks a darker ink itself when stale; dimming it as a layer would make its
    // overlapping segments look dotted.
    let dial = if fresh { dial } else { dial.dim(0.45) };
    let gap = 32.;
    let track_w = if vp.compact() {
        vp.content()
    } else {
        (vp.content() - size - gap).clamp(240., 720.)
    };
    let trace = col()
        .gap(8.)
        .min_w(0.)
        .child(track::view(
            &s.vehicle.ui.track,
            Some(v),
            fresh,
            track_w,
            size + 50.,
        ))
        .child(meta_sized(
            format!(
                "Position trace / {} point{} since the link came up",
                s.vehicle.ui.track.len(),
                if s.vehicle.ui.track.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ),
            9.,
            muted(),
        ));
    let body = if vp.compact() {
        col().gap(20.)
    } else {
        row().gap(gap).items_start()
    };
    body.key("vehicle-instruments")
        .w_full()
        .min_w(0.)
        .py(14.)
        .child(dial)
        .child(trace)
}

/// Whether the active system's simulation link is open but not connected (retrying after the
/// router or simulator went away) while no launch is under way: the case for [`disconnect`].
pub(crate) fn sim_link_stuck(s: &State) -> bool {
    let v = &s.vehicle;
    let up = v
        .snapshot
        .as_ref()
        .is_some_and(|snap| snap.link.state == LinkState::Connected);
    // A headless scene's preview stands in for the link.
    let open = v.link.is_some() || (s.headless && v.preview.is_some());
    v.sim
        && open
        && v.owner.as_deref() == Some(s.systems.active_id())
        && s.session.is_none()
        && (!up || v.router.error.is_some())
}

/// Closes only the simulation link: the simulator, router and other tools keep running.
pub(crate) fn disconnect_link(s: &mut State) {
    crate::vehicle::disconnect(s);
    s.notice = "Vehicle link closed; nothing was sent to the vehicle. The simulator and other tools keep running: Stop session stops them.".into();
}

/// Disconnect for a stuck simulation link (key `vehicle-disconnect`); empty otherwise.
fn disconnect(s: &State) -> El {
    if !sim_link_stuck(s) {
        return col();
    }
    row()
        .key("vehicle-disconnect")
        .w_full()
        .min_w(0.)
        .gaps(8., 16.)
        .flex_wrap()
        .center_y()
        .child(
            ghost("link-disconnect", "Disconnect link", true, disconnect_link).help(
                "Closes only the vehicle link. Processes keep running; Stop session stops everything.",
            ),
        )
        .child(
            meta_sized(
                "The link keeps retrying. Disconnect closes only the link; Stop session stops the simulator and every tool too.",
                9.,
                muted(),
            )
            .wrap(),
        )
}

/// Live view of the active system's vehicle (key `vehicle-live`). Another system's link is
/// named, not shown.
pub(crate) fn live(s: &State, vp: &Viewport) -> El {
    let section = col()
        .key("vehicle-live")
        .w_full()
        .min_w(0.)
        .pt(18.)
        .pb(26.)
        .gap(12.)
        .child(section_head(
            "Live vehicle",
            meta(if s.vehicle.endpoint.is_empty() {
                "Native MAVLink link".to_string()
            } else {
                format!("Native MAVLink / {}", s.vehicle.endpoint)
            })
            .wrap_anywhere(),
            vp.compact(),
        ));
    if let Some(note) = owner_note(s) {
        return section.child(notice(note).key("vehicle-owner"));
    }
    let section = section.child(disconnect(s));
    let Some(snap) = s.vehicle.shown_for(s.systems.active_id()) else {
        let current = s.systems.current();
        // The launch belongs to the system that started it (the active one when unrecorded).
        let active = s.systems.active_id();
        let starting = s.session.is_some()
            && s.systems.owner("session").unwrap_or(active) == active
            && crate::systems::vehicle_profile(current)
                .is_some_and(|id| crate::systems::staged(s, id));
        return section.child(notice(if starting {
            "Workspace starting; the live view appears once the vehicle link is up."
        } else if s.vehicle.owned_by(active) {
            "Connecting to the vehicle link…"
        } else if current.source.simulated() {
            "No live link. Start the workspace to see the simulated vehicle here."
        } else {
            "No live link. Connect to see the vehicle here."
        }));
    };
    let Some(v) = &snap.vehicle else {
        return section.child(notice(match &snap.link.state {
            LinkState::Down(reason) => format!("Link down: {reason}. Retrying."),
            LinkState::Connecting => "Connecting to the vehicle link…".into(),
            LinkState::Connected => "Link open; no vehicle heartbeat yet.".into(),
        }));
    };
    let (line, fresh) = freshness(snap, v);
    section
        .child(
            row()
                .key("vehicle-freshness")
                .role("status")
                .gap(8.)
                .center_y()
                .child(dot(if fresh { success() } else { warn() }))
                .child(meta_sized(line, 9., if fresh { fg() } else { warn() }).wrap()),
        )
        .child(instruments(s, v, fresh, vp))
        .child(readings(snap, v, fresh))
        .child(health::health(s, v, fresh, vp))
}

//! Flight actions for an app-owned simulation: Arm, Disarm, Takeoff, Land, Return to launch,
//! Hold and a mode picker. Shown on simulated systems only, enabled only when
//! `vehicle::authority` grants, and Arm / Takeoff ask first. QGroundControl stays the
//! flight-control interface for real vehicles; physical systems never show this bar.
use super::owner_note;
use crate::mavlink::{CommandKind, CommandResult};
use crate::ui::{Ask, kit::*, theme::*};
use crate::{State, TextEdit, vehicle};
use rune::{Color, El, col, row};

/// Takeoff altitudes the bar accepts (metres above home).
pub(crate) const TAKEOFF_RANGE: std::ops::RangeInclusive<f32> = 1.0..=100.0;
/// Command results shown, newest first.
const RESULTS: usize = 4;
const MODES_KEY: &str = "flight-modes";

/// The typed takeoff altitude, or why it can't be used.
pub(crate) fn takeoff_alt(edit: &TextEdit) -> Result<f32, String> {
    let text = edit.text.trim().trim_end_matches('m').trim();
    match text.parse::<f32>() {
        Ok(alt) if TAKEOFF_RANGE.contains(&alt) => Ok(alt),
        _ => Err(format!(
            "Takeoff altitude: {:.0}–{:.0} m above home.",
            TAKEOFF_RANGE.start(),
            TAKEOFF_RANGE.end()
        )),
    }
}

/// Why the bar is disabled for the active system, or `None` when commands are granted.
pub(crate) fn blocked(s: &State) -> Option<String> {
    owner_note(s).or_else(|| vehicle::gate_reason(s))
}

/// Sends `kind` when the bar is enabled for the active system: the link's authority is
/// checked again in `vehicle::command`, and another system's link is never commanded from
/// this page.
fn send(s: &mut State, kind: CommandKind) {
    let label = kind.label();
    if let Some(reason) = blocked(s) {
        s.notice = format!("{label} not sent: {reason}");
        return;
    }
    match vehicle::command(s, kind) {
        Ok(_) => {
            s.notice = format!("{label} sent to the simulated vehicle. Waiting for its answer.")
        }
        Err(reason) => s.notice = format!("{label} not sent: {reason}"),
    }
}

/// Why a PX4 Takeoff can't carry the confirmed height yet: PX4 takes an altitude above sea
/// level and the vehicle hasn't reported its home.
pub(crate) fn takeoff_unready(s: &State, kind: &CommandKind) -> Option<&'static str> {
    let v = s
        .vehicle
        .shown_for(s.systems.active_id())?
        .vehicle
        .as_ref()?;
    crate::mavlink::takeoff_needs_home(kind, v)
        .then_some("Home not reported yet; wait for the vehicle's home position before Takeoff.")
}

/// Arm and Takeoff wait for the confirmation dialog (`s.confirm == "flight"`).
fn ask_first(s: &mut State, kind: CommandKind) {
    if let Some(reason) = blocked(s) {
        s.notice = format!("{} not sent: {reason}", kind.label());
        return;
    }
    if let Some(reason) = takeoff_unready(s, &kind) {
        s.notice = format!("{} not sent: {reason}", kind.label());
        return;
    }
    s.vehicle.ui.pending = Some(kind);
    s.confirm = Some("flight".into());
}

fn confirmed(s: &mut State) {
    s.confirm = None;
    if let Some(kind) = s.vehicle.ui.pending.take() {
        send(s, kind);
    }
}

fn cancelled(s: &mut State) {
    s.confirm = None;
    if let Some(kind) = s.vehicle.ui.pending.take() {
        s.notice = format!(
            "{} cancelled. Nothing was sent to the vehicle.",
            kind.label()
        );
    }
}

/// The confirmation for a pending Arm or Takeoff.
pub(crate) fn ask(s: &State) -> Option<Ask> {
    let kind = s.vehicle.ui.pending.as_ref()?;
    let system = s
        .vehicle
        .owner
        .as_deref()
        .and_then(|id| s.systems.get(id))
        .map_or("this system", |x| x.label());
    let (title, action) = match kind {
        CommandKind::Takeoff { alt } => (
            format!("Take off to {alt:.0} m?"),
            format!("Take off to {alt:.0} m"),
        ),
        CommandKind::Arm => ("Arm the simulated vehicle?".to_string(), "Arm".to_string()),
        other => (format!("{}?", other.label()), other.label()),
    };
    Some(Ask {
        eyebrow: "Simulation only",
        title,
        body: format!(
            "Simulation only — sends a command to the simulated vehicle of {system} over the app's own simulator link ({}). The vehicle answers with an acknowledgement; nothing reaches a real aircraft.",
            s.vehicle.endpoint
        ),
        action,
        confirm: confirmed,
        cancel: cancelled,
    })
}

/// `linked`: the link the command went out on is still open; a command still waiting when
/// it closed never gets an answer.
fn result_line(result: Option<&CommandResult>, age: u64, linked: bool) -> (String, Color) {
    match result {
        None if !linked => (
            "No answer / the link closed before an acknowledgement".into(),
            warn(),
        ),
        None => (format!("Waiting for acknowledgement / {age}s"), fg()),
        Some(CommandResult::Accepted) => ("Accepted".into(), success()),
        Some(CommandResult::Rejected { reason, .. }) => (format!("Rejected / {reason}"), danger()),
        Some(CommandResult::TimedOut { attempts }) => (
            format!("Timed out / no acknowledgement after {attempts} attempts"),
            warn(),
        ),
        Some(CommandResult::NoVehicle) => ("Not sent / no vehicle heartbeat yet".into(), warn()),
        Some(CommandResult::Busy) => (
            "Not sent / the same command is still waiting".into(),
            warn(),
        ),
        Some(CommandResult::Unsupported) => ("Unsupported by this flight stack".into(), warn()),
        Some(CommandResult::NoHome) => ("Not sent / home not reported yet".into(), warn()),
    }
}

fn results(s: &State) -> El {
    let mut list = col().key("flight-results").w_full().min_w(0.);
    for c in s.vehicle.commands.iter().rev().take(RESULTS) {
        let age = c.sent.elapsed().as_secs();
        let linked = s.vehicle.link.is_some() || s.vehicle.preview.is_some();
        let (mut text, color) = result_line(c.result.as_ref(), age, linked);
        if c.result.is_some() {
            text = format!("{text} / sent {age}s ago");
        }
        list = list.child(
            ruled(false)
                .key(format!("flight-result-{}", c.id))
                .role("status")
                .py(8.)
                .gaps(4., 16.)
                .flex_wrap()
                .center_y()
                .child(caps(c.kind.label(), 10., 500., 0.06, fg()).w(150.))
                .child(
                    row()
                        .gap(8.)
                        .center_y()
                        .min_w(0.)
                        .child(dot(color))
                        .child(meta_sized(text, 9., fg()).wrap()),
                ),
        );
    }
    list
}

fn modes(s: &State, enabled: bool) -> El {
    let mut list = row()
        .key("flight-mode-list")
        .w_full()
        .min_w(0.)
        .gaps(0., 14.)
        .flex_wrap()
        .pt(4.);
    for (i, name) in vehicle::available_modes(s).into_iter().enumerate() {
        list = list.child(ghost(format!("mode-{i}"), name, enabled, move |s| {
            s.ui.open.remove(MODES_KEY);
            send(s, CommandKind::SetMode(name.to_string()));
        }));
    }
    col()
        .w_full()
        .min_w(0.)
        .gap(2.)
        .child(meta_sized("Set flight mode", 9., muted()))
        .child(list)
}

/// The flight actions bar (key `vehicle-actions`). Empty for a system that is not simulated.
pub(crate) fn actions(s: &State, vp: &Viewport) -> El {
    if !s.systems.current().source.simulated() {
        return col();
    }
    let reason = blocked(s);
    let on = reason.is_none();
    let alt = takeoff_alt(&s.vehicle.ui.takeoff_alt);
    let current_mode = s
        .vehicle
        .shown_for(s.systems.active_id())
        .and_then(|snap| snap.vehicle.as_ref())
        .map(|v| v.mode.clone());
    let has_modes = !vehicle::available_modes(s).is_empty();
    let modes_open = on && has_modes && s.ui.open.contains(MODES_KEY);
    let mut alt_field = field(&s.vehicle.ui.takeoff_alt, "takeoff-alt", "10")
        .on_edit(|s: &mut State| &mut s.vehicle.ui.takeoff_alt)
        .label("Takeoff altitude in metres")
        .w(76.);
    if !on {
        alt_field = alt_field.disabled(true).read_only(true).dim(0.5);
    }
    let takeoff = row()
        .key("flight-takeoff-group")
        .gap(8.)
        .center_y()
        .child(alt_field)
        .child(meta_sized("m", 9., muted()))
        .child(pill(
            "flight-takeoff",
            "Takeoff",
            on && alt.is_ok(),
            |s| match takeoff_alt(&s.vehicle.ui.takeoff_alt) {
                Ok(alt) => ask_first(s, CommandKind::Takeoff { alt }),
                Err(e) => s.notice = e,
            },
        ));
    let bar = row()
        .key("flight-bar")
        .w_full()
        .min_w(0.)
        .gaps(10., 10.)
        .flex_wrap()
        .center_y()
        .child(pill("flight-arm", "Arm", on, |s| {
            ask_first(s, CommandKind::Arm)
        }))
        .child(pill("flight-disarm", "Disarm", on, |s| {
            send(s, CommandKind::Disarm)
        }))
        .child(takeoff)
        .child(pill("flight-land", "Land", on, |s| {
            send(s, CommandKind::Land)
        }))
        .child(pill("flight-rtl", "Return", on, |s| {
            send(s, CommandKind::ReturnToLaunch)
        }))
        .child(pill("flight-hold", "Hold", on, |s| {
            send(s, CommandKind::Hold)
        }))
        .child(ghost(
            "flight-mode",
            if modes_open { "Mode ▴" } else { "Mode ▾" },
            on && has_modes,
            |s| {
                if !s.ui.open.remove(MODES_KEY) {
                    s.ui.open.insert(MODES_KEY.into());
                }
            },
        ));
    let mut section = col()
        .key("vehicle-actions")
        .w_full()
        .min_w(0.)
        .pt(18.)
        .pb(22.)
        .gap(12.)
        .child(section_head(
            "Flight actions",
            meta(match (&current_mode, s.vehicle.fresh()) {
                (Some(mode), true) => format!("Simulation only / mode {mode}"),
                (Some(_), false) => "Simulation only / no fresh heartbeat".into(),
                (None, _) => "Simulation only".into(),
            })
            .wrap(),
            vp.compact(),
        ))
        .child(bar);
    if modes_open {
        section = section.child(modes(s, on));
    }
    section = section.child(match (&reason, &alt) {
        (Some(why), _) => notice(why.clone()).key("flight-gate").role("status"),
        (None, Err(e)) => notice(e.clone()).key("flight-gate").role("status"),
        (None, Ok(_)) => notice(
            "Arm and Takeoff ask first. Results arrive as the vehicle acknowledges; QGroundControl remains the flight-control interface for real vehicles.",
        )
        .key("flight-gate"),
    });
    section.child(results(s))
}

//! Physical-vehicle tools on the System page (key `vehicle-tools`): parameters (read, export,
//! compare with a `.params` file) and onboard logs (list, download into Flight logs).
//!
//! Read-only by construction: the only vehicle requests here are PARAM_REQUEST_LIST/READ,
//! LOG_REQUEST_LIST/DATA/END. No element writes a parameter, erases a log or sends a flight
//! command, and the vehicle API offers no call that would.
use crate::State;
use crate::hardware;
use crate::mavlink::LinkState;
use crate::ui::{kit::*, theme::*};
use rune::{El, col, el, row};

mod logs;
mod params;
#[cfg(test)]
mod tests;

/// Why vehicle requests can't be sent now (`None`: they can). Previews count as connected.
fn blocked(s: &State) -> Option<&'static str> {
    if !hardware::connected(s) {
        return Some("Connect first.");
    }
    let snap = s.vehicle.shown_for(s.systems.active_id())?;
    if snap.link.state != LinkState::Connected {
        return Some("The link is not up yet.");
    }
    if snap.vehicle.is_none() {
        return Some("Waiting for an autopilot heartbeat.");
    }
    None
}

/// Whether the vehicle data in `s.vehicle` (parameters, log list) belongs to the active system.
fn mine(s: &State) -> bool {
    s.vehicle.owner.as_deref() == Some(s.systems.active_id())
}

/// A 2px progress rule: `done` of `total` in ink, the rest in the soft rule.
fn bar(key: &str, done: u64, total: u64) -> El {
    let total = total.max(1);
    let done = done.min(total);
    row()
        .key(key)
        .w_full()
        .min_w(0.)
        .h(2.)
        .a11y_hidden()
        .child(el().h(2.).grow(done as f32).fill(fg()))
        .child(el().h(2.).grow((total - done) as f32).fill(rule()))
}

fn percent(done: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        done.min(total) * 100 / total
    }
}

/// The physical tools of the active system: shown below the live view.
pub(crate) fn tools(s: &State, vp: &Viewport) -> El {
    let section = col()
        .key("vehicle-tools")
        .w_full()
        .min_w(0.)
        .pt(18.)
        .pb(26.);
    let active = s.systems.active_id();
    if let Some(other) = s
        .vehicle
        .owner
        .as_deref()
        .filter(|o| *o != active && s.vehicle.link.is_some())
    {
        let name = s.systems.get(other).map_or(other, |x| x.label());
        return section
            .child(section_head(
                "Vehicle tools",
                meta("Parameters / onboard logs"),
                vp.compact(),
            ))
            .child(notice(format!(
                "Live link belongs to {name}. Disconnect it there to read this vehicle's parameters and logs."
            )));
    }
    section
        .child(params::section(s, vp))
        .child(logs::section(s, vp))
}

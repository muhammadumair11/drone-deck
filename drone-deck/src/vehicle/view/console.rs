//! The vehicle console (MAVLink shell over SERIAL_CONTROL): bounded output, an input line
//! (Return sends, Up / Down walk the history), disabled with the gate's reason. PX4 answers
//! with its NuttX shell; collapsed by default.
use super::actions::blocked;
use crate::mavlink::Autopilot;
use crate::ui::{kit::*, theme::*};
use crate::{State, TextEdit, activity, vehicle};
use rune::{El, Key, KeyEvent, ScrollSpec, Tx, col, row};

/// Lines of output drawn (the full text stays in `Vehicle::console`, capped at 64 KiB).
const SHOWN_LINES: usize = 160;
/// Commands kept for Up / Down.
pub(crate) const HISTORY: usize = 50;
pub(crate) const KEY: &str = "vehicle-console";

/// The tail of the console text that is drawn.
pub(crate) fn tail(text: &str, lines: usize) -> &str {
    let body = text.strip_suffix('\n').unwrap_or(text);
    match body.rmatch_indices('\n').nth(lines.saturating_sub(1)) {
        Some((i, _)) => &text[i + 1..],
        None => text,
    }
}

/// Sends the typed line and keeps it in the history.
pub(crate) fn submit(s: &mut State) {
    let line = s.vehicle.ui.console_input.text.trim().to_string();
    if line.is_empty() {
        return;
    }
    if let Some(reason) = blocked(s) {
        s.notice = format!("Console: {reason}");
        return;
    }
    match vehicle::console_send(s, &line) {
        Ok(()) => {
            let ui = &mut s.vehicle.ui;
            if ui.history.back() != Some(&line) {
                ui.history.push_back(line.clone());
                while ui.history.len() > HISTORY {
                    ui.history.pop_front();
                }
            }
            ui.history_at = None;
            ui.console_input = TextEdit::new("");
            activity::record(
                s,
                format!("Console command on the simulated vehicle: {line}"),
            );
        }
        Err(reason) => s.notice = format!("Console: {reason}"),
    }
}

/// Up / Down in the input walk the history; true when the key was used.
pub(crate) fn history_key(s: &mut State, ev: &KeyEvent, _: &mut Tx) -> bool {
    let ui = &mut s.vehicle.ui;
    if ui.history.is_empty() {
        return false;
    }
    let last = ui.history.len() - 1;
    let at = match (&ev.key, ui.history_at) {
        (Key::Up, None) => Some(last),
        (Key::Up, Some(i)) => Some(i.saturating_sub(1)),
        (Key::Down, Some(i)) if i < last => Some(i + 1),
        (Key::Down, Some(_)) => None,
        _ => return false,
    };
    ui.history_at = at;
    ui.console_input = TextEdit::new(at.map_or("", |i| ui.history[i].as_str()));
    true
}

fn body(s: &State, reason: Option<String>) -> El {
    let on = reason.is_none();
    let ardupilot = s
        .vehicle
        .shown_for(s.systems.active_id())
        .and_then(|snap| snap.vehicle.as_ref())
        .is_some_and(|v| v.autopilot == Autopilot::ArduPilot);
    let mut out = col()
        .key("console-output")
        .role("log")
        .label("Vehicle console output")
        .w_full()
        .min_w(0.)
        .max_h(280.)
        .padding(12.)
        .border(1., soft())
        .scroll(ScrollSpec { stick_bottom: true });
    out = if s.vehicle.console.is_empty() {
        out.child(meta_sized(
            "No console output yet. Try help, ver all or commander status.",
            9.,
            muted(),
        ))
    } else {
        out.child(code(tail(&s.vehicle.console, SHOWN_LINES), 11., fg()).wrap_anywhere())
    };
    let mut input = field(&s.vehicle.ui.console_input, "console-input", "nsh> command")
        .on_edit(|s: &mut State| &mut s.vehicle.ui.console_input)
        .label("Console command")
        .on_submit(|s: &mut State, _| submit(s))
        .on_input_key(history_key)
        .grow(1.);
    if !on {
        input = input.disabled(true).read_only(true).dim(0.5);
    }
    let controls = row()
        .w_full()
        .min_w(0.)
        .gaps(8., 12.)
        .flex_wrap()
        .center_y()
        .child(row().grow(1.).min_w(200.).child(input))
        .child(pill("console-send", "Send", on, submit))
        .child(ghost(
            "console-clear",
            "Clear output",
            !s.vehicle.console.is_empty(),
            |s| s.vehicle.console.clear(),
        ))
        .child(ghost(
            "console-close",
            "End shell session",
            on,
            |s| match vehicle::console_close(s) {
                Ok(()) => s.notice = "Console session ended on the vehicle.".into(),
                Err(e) => s.notice = format!("Console: {e}"),
            },
        ));
    let mut block = col()
        .w_full()
        .min_w(0.)
        .gap(12.)
        .child(p(
            "Commands run on the simulated flight controller's shell. Return sends; Up and Down recall earlier commands.",
            10.,
        ));
    if ardupilot {
        block = block.child(notice(
            "ArduPilot has no MAVLink shell; this console answers on PX4 only.",
        ));
    }
    block = block.child(out).child(controls);
    if let Some(why) = reason {
        block = block.child(notice(why).key("console-gate").role("status"));
    }
    block
}

/// The console disclosure (key `vehicle-console`). Empty for a system that is not simulated.
pub(crate) fn console(s: &State, _vp: &Viewport) -> El {
    if !s.systems.current().source.simulated() {
        return col();
    }
    let reason = blocked(s);
    col().w_full().min_w(0.).pb(18.).child(disclosure(
        s,
        KEY,
        "Vehicle console / MAVLink shell",
        || body(s, reason),
    ))
}

//! The controls of a system draft: vehicle kind, autopilot, world with its details, and the
//! consumers. Settings → Systems and guided setup show the same controls; each call names
//! the draft it edits with an accessor, so the element keys and rules stay the same in both.
use super::kit::*;
use super::theme::*;
use crate::systems::adapter::{self, AUTOPILOTS, WORLDS};
use crate::systems::compat::Consumer;
use crate::systems::{Draft, System, VEHICLES, Vehicle};
use crate::*;

/// Where the draft a control edits lives.
pub(crate) type At = for<'a> fn(&'a mut State) -> &'a mut Draft;

pub(crate) fn text_field(
    edit: &TextEdit,
    key: &str,
    label: &str,
    placeholder: &str,
    on: impl for<'a> Fn(&'a mut State) -> &'a mut TextEdit + 'static,
) -> El {
    labeled(label, field(edit, key, placeholder).on_edit(on))
}

pub(crate) fn pair(a: El, b: El, compact: bool) -> El {
    if compact {
        return col().w_full().min_w(0.).gap(16.).child(a).child(b);
    }
    row()
        .w_full()
        .min_w(0.)
        .gap(24.)
        .items_start()
        .child(col().grow(1.).w(0.).min_w(0.).child(a))
        .child(col().grow(1.).w(0.).min_w(0.).child(b))
}

fn note(t: &str) -> El {
    p(t, 11.).max_w(620.)
}

/// Simulated / Physical vehicle / Recording replay (`system-vehicle-<i>`).
pub(crate) fn vehicle_tabs(d: &Draft, at: At) -> El {
    tabs(
        "system-vehicle",
        &VEHICLES,
        d.vehicle,
        move |s: &mut State, i| {
            at(s).vehicle = i;
        },
    )
}

/// Choose a physical vehicle's endpoint instead of typing it: serial ports from a scan
/// (`endpoint-scan`, `endpoint-port-<name>`), the baud rate of the chosen port
/// (`endpoint-baud-<n>`) and common network endpoints (`endpoint-preset-<i>`). `set` stores
/// the endpoint chosen.
pub(crate) fn endpoint_picker<F>(s: &State, current: &str, enabled: bool, set: F) -> El
where
    F: Fn(&mut State, String) + Clone + 'static,
{
    use crate::hardware::ports::{self, BAUDS, DEFAULT_BAUD, PRESETS};
    let scan = &s.hardware.ports;
    let chosen = ports::serial_of(current);
    let mut out = col()
        .key("endpoint-picker")
        .w_full()
        .min_w(0.)
        .gap(6.)
        .child(
            row()
                .gaps(4., 20.)
                .center_y()
                .flex_wrap()
                .child(meta("Serial devices"))
                .child(ghost(
                    "endpoint-scan",
                    if scan.scanning() {
                        "Looking…"
                    } else if scan.found.is_some() {
                        "Scan again"
                    } else {
                        "Find serial devices"
                    },
                    enabled && !scan.scanning() && !s.headless,
                    ports::scan,
                )),
        );
    match &scan.found {
        None if scan.error.is_empty() => {
            out = out.child(p(
                "Find serial devices lists the COM ports with their USB names.",
                11.,
            ))
        }
        Some(found) if found.is_empty() => {
            out = out.child(p(
                "No serial devices found. Check the cable and driver, then scan again.",
                11.,
            ))
        }
        _ => {}
    }
    if !scan.error.is_empty() {
        out = out.child(body(scan.error.clone(), 11., danger()).wrap());
    }
    for port in scan.found.iter().flatten() {
        let picked = chosen.as_ref().is_some_and(|(p, _)| *p == port.name);
        let baud = chosen.as_ref().map_or(DEFAULT_BAUD, |(_, b)| *b);
        let (name, set) = (port.name.clone(), set.clone());
        out = out.child(ghost(
            format!("endpoint-port-{}", port.name),
            &format!(
                "{}{} / {}",
                if picked { "✓ " } else { "" },
                port.name,
                port.detail
            ),
            enabled,
            move |s| set(s, ports::endpoint(&name, baud)),
        ));
    }
    if let Some((port, baud)) = &chosen {
        let mut bauds = row()
            .gaps(4., 16.)
            .center_y()
            .flex_wrap()
            .child(meta("Baud"));
        for b in BAUDS {
            let (port, set) = (port.clone(), set.clone());
            bauds = bauds.child(ghost(
                format!("endpoint-baud-{b}"),
                &if b == *baud {
                    format!("✓ {b}")
                } else {
                    b.to_string()
                },
                enabled,
                move |s| set(s, ports::endpoint(&port, b)),
            ));
        }
        out = out.child(bauds);
    }
    let mut presets = row()
        .gaps(4., 16.)
        .center_y()
        .flex_wrap()
        .child(meta("Network"));
    for (i, (name, value)) in PRESETS.iter().enumerate() {
        let set = set.clone();
        presets = presets.child(ghost(
            format!("endpoint-preset-{i}"),
            &if current == *value {
                format!("✓ {name}")
            } else {
                name.to_string()
            },
            enabled,
            move |s| set(s, value.to_string()),
        ));
    }
    out.child(presets)
}

/// The flight stack a physical vehicle runs (`system-hint-<i>`): Not sure, PX4 or ArduPilot.
/// Labels only; the vehicle's heartbeat is what identifies it.
fn hint_tabs(d: &Draft, at: At) -> El {
    let current = match d.hint {
        None => 0,
        Some(k) => 1 + AUTOPILOTS.iter().position(|a| *a == k).unwrap_or(0),
    };
    let mut names = vec!["Not sure"];
    names.extend(AUTOPILOTS.iter().map(|k| adapter::autopilot_name(*k)));
    tabs("system-hint", &names, current, move |s: &mut State, i| {
        at(s).hint = i.checked_sub(1).map(|i| AUTOPILOTS[i]);
    })
}

/// The endpoint of a physical vehicle (`system-endpoint`), chosen with the picker, and the
/// flight stack it runs.
pub(crate) fn physical(s: &State, d: &Draft, at: At) -> El {
    col()
        .w_full()
        .min_w(0.)
        .gap(12.)
        .child(caps("Flight stack", 10., 400., 0.08, muted()))
        .child(hint_tabs(d, at))
        .child(
            text_field(
                &d.endpoint,
                "system-endpoint",
                "Vehicle endpoint",
                "serial:COM4:115200",
                move |s| &mut at(s).endpoint,
            )
            .max_w(440.),
        )
        .child(endpoint_picker(
            s,
            d.endpoint.text.trim(),
            true,
            move |s, value| at(s).endpoint = TextEdit::new(&value),
        ))
        .child(note(
            "serial:COM4:115200, udp:0.0.0.0:14550 or tcp:192.168.4.1:5760. Making the system active fills the connection's endpoint. Drone Deck itself only sends read requests; QGroundControl or scripts enabled below can command the vehicle through the link.",
        ))
}

/// The recording a replay plays (`system-recording`).
pub(crate) fn replay(d: &Draft, at: At) -> El {
    col()
        .w_full()
        .min_w(0.)
        .gap(12.)
        .child(
            text_field(
                &d.recording,
                "system-recording",
                "Recording (optional)",
                "Chosen at replay when empty",
                move |s| &mut at(s).recording,
            )
            .max_w(440.),
        )
        .child(note(
            "Replay runs on a separate ROS domain so it never mixes with live topics.",
        ))
}

/// PX4 / ArduPilot (`system-autopilot-<i>`).
pub(crate) fn autopilot_tabs(d: &Draft, at: At) -> El {
    let names: Vec<&str> = AUTOPILOTS
        .iter()
        .map(|k| adapter::autopilot_name(*k))
        .collect();
    let current = AUTOPILOTS
        .iter()
        .position(|k| *k == d.autopilot)
        .unwrap_or(0);
    tabs(
        "system-autopilot",
        &names,
        current,
        move |s: &mut State, i| {
            at(s).autopilot = AUTOPILOTS[i];
        },
    )
}

/// The details the chosen adapter takes, two to a row (`system-param-<key>`).
fn params(d: &Draft, at: At, compact: bool) -> El {
    let Some(a) = d.adapter() else {
        return el();
    };
    let field = |p: &'static adapter::Param| {
        let key = p.key;
        text_field(
            &d.params[key],
            &format!("system-param-{key}"),
            p.label,
            p.default,
            move |s| {
                at(s)
                    .params
                    .get_mut(key)
                    .expect("every adapter param has a field")
            },
        )
    };
    let mut out = col().w_full().min_w(0.).gap(16.);
    for pair_of in a.params.chunks(2) {
        out = out.child(match pair_of {
            [one, two] => pair(field(one), field(two), compact),
            [one] => field(one).max_w(440.),
            _ => el(),
        });
    }
    out
}

/// World rows (`system-world-<id>`) and the chosen adapter's details. A world without an
/// adapter for the chosen autopilot stays listed, disabled, with the reason. `base` is the
/// saved system, whose unsupported world saving keeps unchanged.
pub(crate) fn worlds(d: &Draft, base: Option<&System>, at: At, compact: bool) -> El {
    let mut rows = col().w_full().min_w(0.).max_w(620.);
    for (id, name, about) in WORLDS {
        let ok = adapter::world_for(d.autopilot, id);
        let detail = match &ok {
            Ok(_) => about.to_string(),
            Err(why) => why.clone(),
        };
        rows = rows.child(crate::setup::option_row(
            format!("system-world-{id}"),
            name,
            &detail,
            d.world == id,
            ok.is_ok(),
            move |s| at(s).world = id.to_string(),
        ));
    }
    let mut out = col().w_full().min_w(0.).gap(12.).child(rows);
    match adapter::world_for(d.autopilot, &d.world) {
        Ok(_) => out = out.child(params(d, at, compact)),
        Err(why) => {
            let kept = base.is_some_and(|b| {
                matches!(&b.vehicle, Vehicle::Simulated { autopilot, world }
                    if autopilot.kind() == d.autopilot && world.id == d.world)
            });
            let next = if kept {
                "Saving keeps it as it is, but it cannot start in this version."
            } else {
                "Choose another world."
            };
            out = out.child(
                body(format!("{why} {next}"), 11., warn())
                    .key("system-world-unsupported")
                    .role("status")
                    .wrap()
                    .max_w(620.),
            )
        }
    }
    out
}

/// Under the consumers when Share on Wi-Fi is on.
const WIFI_NOTE: &str = "The simulator's MAVLink is broadcast to this computer's local network on UDP 14550. A ground station app on a phone or tablet on the same Wi-Fi connects by listening on UDP 14550; no address to enter. Anyone on that network can see and command the simulated drone, so use it on a network you trust.";

/// Under the consumers of a physical system with QGroundControl or scripts enabled.
const BRIDGE_WARNING: &str = "QGroundControl and scripts are bridged to the vehicle both ways: Drone Deck forwards what they send, so they can command this vehicle and write parameters. Drone Deck itself only reads.";

/// The consumers the draft's vehicle can feed (`consumer-<name>`) and their fields.
pub(crate) fn consumers(d: &Draft, at: At, compact: bool) -> El {
    let live = match d.vehicle {
        1 => "Live view / vehicle link and tlog",
        _ => "Live view / native vehicle link on the System page",
    };
    let toggle = |key: &str, name: &str, on: bool, f: fn(&mut Draft)| {
        checkbox(key, name, on, move |s| f(at(s)))
    };
    let mut boxes = col().w_full().min_w(0.).child(toggle(
        "consumer-qgc",
        "QGroundControl / flight controls",
        d.qgc,
        |d| d.qgc = !d.qgc,
    ));
    if d.feeds(Consumer::LiveView) {
        boxes = boxes.child(toggle("consumer-live-view", live, d.live_view, |d| {
            d.live_view = !d.live_view
        }));
    }
    if d.feeds(Consumer::RosAgent) {
        boxes = boxes.child(toggle(
            "consumer-ros-agent",
            "ROS 2 bridge / DDS Agent",
            d.ros_agent,
            |d| d.ros_agent = !d.ros_agent,
        ));
    }
    boxes = boxes.child(toggle(
        "consumer-recorder",
        "rosbag2 / MCAP recorder",
        d.recorder,
        |d| d.recorder = !d.recorder,
    ));
    if d.recorder && d.vehicle == 0 {
        boxes = boxes.child(toggle(
            "consumer-record-on-start",
            "Record when the session starts",
            d.record_on_start,
            |d| d.record_on_start = !d.record_on_start,
        ));
    }
    boxes = boxes
        .child(toggle(
            "consumer-inference",
            "ONNX inference project",
            d.inference,
            |d| d.inference = !d.inference,
        ))
        .child(toggle("consumer-foxglove", "Foxglove", d.foxglove, |d| {
            d.foxglove = !d.foxglove
        }));
    if d.feeds(Consumer::Scripts) {
        boxes = boxes.child(toggle(
            "consumer-scripts",
            "User scripts / MAVLink over UDP (two-way)",
            d.scripts,
            |d| d.scripts = !d.scripts,
        ));
    }
    if d.feeds(Consumer::Wifi) {
        boxes = boxes.child(toggle(
            "consumer-wifi",
            "Share on Wi-Fi / phones and tablets (two-way)",
            d.wifi,
            |d| d.wifi = !d.wifi,
        ));
    }
    let mut fields = col().w_full().min_w(0.).gap(16.);
    if (d.ros_agent && d.feeds(Consumer::RosAgent)) || d.recorder {
        fields = fields.child(text_field(
            &d.domain,
            "system-domain",
            "ROS domain",
            "0",
            move |s| &mut at(s).domain,
        ));
    }
    if d.inference {
        fields = fields.child(text_field(
            &d.project,
            "system-project",
            "Project (optional)",
            "Any project",
            move |s| &mut at(s).project,
        ));
    }
    if d.scripts && d.feeds(Consumer::Scripts) {
        fields = fields.child(text_field(
            &d.scripts_udp,
            "system-scripts",
            "Scripts address",
            "127.0.0.1:14540",
            move |s| &mut at(s).scripts_udp,
        ));
        if let Some(problem) = d.scripts_problem() {
            fields = fields.child(
                body(problem, 11., danger())
                    .key("system-scripts-invalid")
                    .role("status")
                    .wrap()
                    .max_w(620.),
            );
        } else if let Some(caution) = d.caution() {
            fields = fields.child(
                body(caution, 11., warn())
                    .key("system-scripts-network")
                    .role("status")
                    .wrap()
                    .max_w(620.),
            );
        }
    }
    if d.wifi && d.feeds(Consumer::Wifi) {
        fields = fields.child(
            body(WIFI_NOTE, 11., warn())
                .key("system-wifi-note")
                .role("status")
                .wrap()
                .max_w(620.),
        );
    }
    let consumers = pair(boxes, fields, compact);
    if d.vehicle == 1 && (d.qgc || d.scripts) {
        return col().w_full().min_w(0.).gap(12.).child(consumers).child(
            body(BRIDGE_WARNING, 11., warn())
                .key("system-bridge-warning")
                .wrap()
                .max_w(620.),
        );
    }
    consumers
}

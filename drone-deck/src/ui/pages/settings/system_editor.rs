//! The inline editor of one system: name, vehicle (an autopilot in a world, a physical
//! vehicle or a recording) with its details, and consumers.
use super::*;
use crate::systems::adapter::{self, AUTOPILOTS, WORLDS};
use crate::systems::compat::Consumer;
use crate::systems::{Draft, System, VEHICLES};

fn draft(s: &mut State) -> &mut Draft {
    s.systems.editor.get_or_insert_with(Draft::default)
}

fn text_field(
    edit: &TextEdit,
    key: &str,
    label: &str,
    placeholder: &str,
    on: impl for<'a> Fn(&'a mut State) -> &'a mut TextEdit + 'static,
) -> El {
    labeled(label, field(edit, key, placeholder).on_edit(on))
}

fn pair(a: El, b: El, compact: bool) -> El {
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

/// The details the chosen adapter takes, two to a row (keys `system-param-<key>`).
fn params(d: &Draft, compact: bool) -> El {
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
                draft(s)
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

/// Autopilot tabs (`system-autopilot-<i>`) and world rows (`system-world-<id>`). A world
/// without an adapter for the chosen autopilot stays listed, disabled, with the reason.
fn simulated(d: &Draft, base: &System, compact: bool) -> El {
    let names: Vec<&str> = AUTOPILOTS
        .iter()
        .map(|k| adapter::autopilot_name(*k))
        .collect();
    let current = AUTOPILOTS
        .iter()
        .position(|k| *k == d.autopilot)
        .unwrap_or(0);
    let mut worlds = col().w_full().min_w(0.).max_w(620.);
    for (id, name, about) in WORLDS {
        let ok = adapter::world_for(d.autopilot, id);
        let detail = match &ok {
            Ok(_) => about.to_string(),
            Err(why) => why.clone(),
        };
        worlds = worlds.child(crate::setup::option_row(
            format!("system-world-{id}"),
            name,
            &detail,
            d.world == id,
            ok.is_ok(),
            move |s| draft(s).world = id.to_string(),
        ));
    }
    let mut out = col()
        .w_full()
        .min_w(0.)
        .gap(12.)
        .child(caps("Autopilot", 10., 400., 0.08, muted()))
        .child(tabs(
            "system-autopilot",
            &names,
            current,
            |s: &mut State, i| {
                draft(s).autopilot = AUTOPILOTS[i];
            },
        ))
        .child(caps("World", 10., 400., 0.08, muted()))
        .child(worlds);
    match adapter::world_for(d.autopilot, &d.world) {
        Ok(_) => out = out.child(params(d, compact)),
        Err(why) => {
            let kept = matches!(&base.vehicle, crate::systems::Vehicle::Simulated { autopilot, world }
                if autopilot.kind() == d.autopilot && world.id == d.world);
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

fn vehicle_details(d: &Draft, base: &System, compact: bool) -> El {
    let note = |t: &str| p(t, 11.).max_w(620.);
    match d.vehicle {
        1 => col()
            .w_full()
            .min_w(0.)
            .gap(12.)
            .child(
                text_field(
                    &d.endpoint,
                    "system-endpoint",
                    "Vehicle endpoint",
                    "serial:COM4:115200",
                    |s| &mut draft(s).endpoint,
                )
                .max_w(440.),
            )
            .child(note(
                "serial:COM4:115200, udp:0.0.0.0:14550 or tcp:192.168.4.1:5760. Making the system active fills the connection's endpoint. Drone Deck itself only sends read requests; QGroundControl or scripts enabled below can command the vehicle through the link.",
            )),
        2 => col()
            .w_full()
            .min_w(0.)
            .gap(12.)
            .child(
                text_field(
                    &d.recording,
                    "system-recording",
                    "Recording (optional)",
                    "Chosen at replay when empty",
                    |s| &mut draft(s).recording,
                )
                .max_w(440.),
            )
            .child(note("Replay runs on a separate ROS domain so it never mixes with live topics.")),
        _ => simulated(d, base, compact),
    }
}

fn consumers(d: &Draft, compact: bool) -> El {
    let live = match d.vehicle {
        1 => "Live view / vehicle link and tlog",
        _ => "Live view / native vehicle link on the System page",
    };
    let toggle = |key: &str, name: &str, on: bool, f: fn(&mut Draft)| {
        checkbox(key, name, on, move |s| f(draft(s)))
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
    boxes = boxes
        .child(toggle(
            "consumer-recorder",
            "rosbag2 / MCAP recorder",
            d.recorder,
            |d| d.recorder = !d.recorder,
        ))
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
    let mut fields = col().w_full().min_w(0.).gap(16.);
    if (d.ros_agent && d.feeds(Consumer::RosAgent)) || d.recorder {
        fields = fields.child(text_field(
            &d.domain,
            "system-domain",
            "ROS domain",
            "0",
            |s| &mut draft(s).domain,
        ));
    }
    if d.inference {
        fields = fields.child(text_field(
            &d.project,
            "system-project",
            "Project (optional)",
            "Any project",
            |s| &mut draft(s).project,
        ));
    }
    if d.scripts && d.feeds(Consumer::Scripts) {
        fields = fields.child(text_field(
            &d.scripts_udp,
            "system-scripts",
            "Scripts address",
            "127.0.0.1:14540",
            |s| &mut draft(s).scripts_udp,
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

/// Under the consumers of a physical system with QGroundControl or scripts enabled.
const BRIDGE_WARNING: &str = "QGroundControl and scripts are bridged to the vehicle both ways: Drone Deck forwards what they send, so they can command this vehicle and write parameters. Drone Deck itself only reads.";

pub(super) fn view(s: &State, sys: &System, compact: bool) -> El {
    let Some(d) = s.systems.editor.as_ref() else {
        return el();
    };
    let vehicle_tabs = tabs(
        "system-vehicle",
        &VEHICLES,
        d.vehicle,
        |s: &mut State, i| {
            draft(s).vehicle = i;
        },
    );
    col()
        .key("system-editor")
        .w_full()
        .min_w(0.)
        .gap(16.)
        .pb(24.)
        .child(
            text_field(&d.name, "system-name", "Name", "My system", |s| &mut draft(s).name)
                .max_w(440.),
        )
        .child(
            col()
                .w_full()
                .min_w(0.)
                .child(caps("Vehicle", 10., 400., 0.08, muted()))
                .child(vehicle_tabs)
                .child(vehicle_details(d, sys, compact)),
        )
        .child(
            col()
                .w_full()
                .min_w(0.)
                .gap(8.)
                .child(caps("Consumers", 10., 400., 0.08, muted()))
                .child(consumers(d, compact)),
        )
        .child(
            body(d.error.clone(), 11., danger())
                .key("system-error")
                .role("status")
                .w_full(),
        )
        .child(notice(format!(
            "Template {}. Components, connections and the workspace launch follow the vehicle and consumers; nothing is installed or started by saving.",
            crate::systems::KINDS[sys.template_index()]
        )))
        .child(
            row()
                .gaps(8., 16.)
                .center_y()
                .flex_wrap()
                .child(pill("system-save", "Save system", true, super::system_list::save_editor))
                .child(ghost("system-cancel", "Cancel", true, |s| s.systems.editor = None)),
        )
}

//! The inline editor of one system: name, source with its details, and consumers.
use super::*;
use crate::systems::{Draft, SOURCES, System};

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

fn source_details(d: &Draft, compact: bool) -> El {
    let note = |t: &str| p(t, 11.).max_w(620.);
    match d.source {
        1 => col()
            .w_full()
            .min_w(0.)
            .gap(12.)
            .child(pair(
                text_field(&d.world, "system-world", "Gazebo world", "default", |s| {
                    &mut draft(s).world
                }),
                text_field(&d.model, "system-model", "Vehicle model", "x500_mono_cam", |s| {
                    &mut draft(s).model
                }),
                compact,
            ))
            .child(note(
                "Saved with the system. x500_mono_cam carries a camera; the launch profile still decides the command that runs.",
            )),
        2 => col()
            .w_full()
            .min_w(0.)
            .gap(12.)
            .child(
                text_field(&d.vehicle, "system-vehicle", "ArduPilot vehicle", "copter", |s| {
                    &mut draft(s).vehicle
                })
                .max_w(440.),
            )
            .child(note("copter, plane or rover. ArduPilot SITL cannot run beside another simulator.")),
        3 => col()
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
        4 => col()
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
        _ => note("PX4 software-in-the-loop with internal physics. No world or model to choose."),
    }
}

fn consumers(d: &Draft, compact: bool) -> El {
    let live = match d.source {
        3 => "Live view / vehicle link and tlog",
        4 => "Live view",
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
    if matches!(d.source, 0 | 3) {
        boxes = boxes.child(toggle("consumer-live-view", live, d.live_view, |d| {
            d.live_view = !d.live_view
        }));
    }
    if matches!(d.source, 0..=2) {
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
    if d.source != 4 {
        boxes = boxes.child(toggle(
            "consumer-scripts",
            "User scripts / MAVLink over UDP (two-way)",
            d.scripts,
            |d| d.scripts = !d.scripts,
        ));
    }
    let mut fields = col().w_full().min_w(0.).gap(16.);
    if (d.ros_agent && matches!(d.source, 0..=2)) || d.recorder {
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
    if d.scripts && d.source != 4 {
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
    if d.source == 3 && (d.qgc || d.scripts) {
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
    let source_tabs = tabs("system-source", &SOURCES, d.source, |s: &mut State, i| {
        draft(s).source = i;
    });
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
                .child(caps("Source", 10., 400., 0.08, muted()))
                .child(source_tabs)
                .child(source_details(d, compact)),
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
            "Template {}. Components, connections and the workspace launch follow the source and consumers; nothing is installed or started by saving.",
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

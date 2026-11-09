//! The physical-vehicle monitor on the System page: a native endpoint, identified vehicles and
//! read-only parameter backups. The active system decides that this monitor is shown.
use super::{parameters, start};
use crate::ui::{kit::*, theme::*};
use crate::*;

const FRESH: Duration = Duration::from_secs(10);

/// Starts or stops the native monitor and records the request.
pub(crate) fn toggle(s: &mut State) {
    if systems::running(s, "hardware") {
        s.send(Request::Stop("hardware".into()));
        s.notice = "Stopping the vehicle monitor. No command is sent to the vehicle.".into();
        activity::record(s, "Stopped the native vehicle monitor");
        return;
    }
    if s.setup.batch.is_some() || s.setup.jobs.iter().any(|j| j.alive) {
        s.hardware.message =
            "Finish or cancel the running installation before monitoring starts.".into();
        return;
    }
    s.hardware.mode = 1;
    start(s);
    if systems::running(s, "hardware") {
        let endpoint = s.hardware.endpoint.text.trim().to_string();
        activity::record(s, format!("Started vehicle monitoring on {endpoint}"));
    }
}

fn autopilot(vehicle: &serde_json::Value) -> &'static str {
    match vehicle["fields"]["autopilot"].as_u64() {
        Some(12) => "PX4",
        Some(3) => "ArduPilot",
        _ => "Other autopilot",
    }
}

fn fields(vehicle: &serde_json::Value) -> String {
    let Some(map) = vehicle["fields"].as_object() else {
        return String::new();
    };
    map.iter()
        .map(|(k, v)| match v.as_str() {
            Some(t) => format!("{k} {t}"),
            None => format!("{k} {v}"),
        })
        .collect::<Vec<_>>()
        .join("   ")
}

fn source(s: &State, running: bool, compact: bool) -> El {
    let h = &s.hardware;
    let actions = row()
        .gaps(8., 20.)
        .center_y()
        .flex_wrap()
        .child(pill(
            "monitor-start",
            if running {
                "Stop monitor"
            } else {
                "Start monitoring"
            },
            true,
            toggle,
        ))
        .child(ghost("scan-serial", "Find serial devices", true, |s| {
            crate::studio::request(s, "ports")
        }));
    col()
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(p(
            "Listen without sending flight commands. Use a dedicated telemetry endpoint; another app may already own the serial port. A signed packet's signature is not authenticated by this monitor.",
            11.,
        ))
        .child(labeled(
            "Vehicle endpoint",
            field(&h.endpoint, "hardware-endpoint", "udp:127.0.0.1:14555")
                .on_edit(|s: &mut State| &mut s.hardware.endpoint),
        ))
        .child(
            code(
                "serial:COM4:115200   udp:127.0.0.1:14555   tcp:localhost:5760",
                10.,
                muted(),
            )
            .wrap_anywhere(),
        )
        .child(if running {
            meta_sized("Endpoint changes apply the next time monitoring starts", 9., muted()).wrap()
        } else {
            el()
        })
        .child(actions)
        .child(if h.message != super::Hardware::default().message {
            body(&h.message, 11., fg())
                .key("hardware-message")
                .role("status")
                .max_w(if compact { 10_000. } else { 620. })
        } else {
            el()
        })
}

fn ports(s: &State, running: bool) -> El {
    let mut list = col()
        .key("serial-ports")
        .w_full()
        .min_w(0.)
        .gap(4.)
        .child(meta("Windows serial devices"));
    let Some(ports) = s.studio.library["ports"].as_array() else {
        return list.child(p(
            "Not scanned. Find serial devices lists COM ports with their Windows names.",
            11.,
        ));
    };
    if ports.is_empty() {
        return list.child(p(
            "No serial devices found. Check the cable and driver, then scan again.",
            11.,
        ));
    }
    for port in ports {
        let id = port["DeviceID"].as_str().unwrap_or("").to_string();
        let name = port["Name"].as_str().unwrap_or("Unnamed device");
        let endpoint = format!("serial:{id}:115200");
        list = list.child(
            ghost(
                format!("port-{id}"),
                &format!("Use {id} / {name}"),
                !running,
                move |s| s.hardware.endpoint = TextEdit::new(endpoint.clone()),
            )
            .help(format!("Sets the endpoint to serial:{id}:115200")),
        );
    }
    list
}

fn vehicle(key: &str, v: &serde_json::Value, age: Option<Duration>, selected: bool) -> El {
    let fresh = age.is_some_and(|a| a < FRESH);
    let (system, component) = key.split_once(':').unwrap_or((key, "?"));
    let heard = match age {
        Some(a) => format!("Heartbeat {}s ago", a.as_secs()),
        None => "No heartbeat".into(),
    };
    let pick = key.to_string();
    let title = format!("System {system} / component {component} / {}", autopilot(v));
    col()
        .key(format!("vehicle-{key}"))
        .role("button")
        .label(title.clone())
        .focusable(true)
        .focus_selected(selected)
        .cursor(rune::Cursor::Pointer)
        .w_full()
        .min_w(0.)
        .px(8.)
        .py(14.)
        .gap(8.)
        .border_sides([1., 0., 0., 0.], soft())
        .fill(if selected { soft() } else { Color::TRANSPARENT })
        .hover(soft())
        .focus_ring(2., 2., fg())
        .on_press(move |s: &mut State| s.hardware.selected = Some(pick.clone()))
        .child(
            row()
                .w_full()
                .min_w(0.)
                .gaps(6., 16.)
                .center_y()
                .flex_wrap()
                .child(h3(title, 15.).grow(1.))
                .child(
                    row()
                        .gap(8.)
                        .center_y()
                        .child(dot(if fresh { success() } else { warn() }))
                        .child(meta_sized(heard, 9., fg())),
                ),
        )
        .child(code(fields(v), 10., muted()).wrap_anywhere())
}

fn vehicles(s: &State, running: bool, compact: bool) -> El {
    let h = &s.hardware;
    let mut list = col()
        .key("hardware-vehicles")
        .w_full()
        .min_w(0.)
        .mt(8.)
        .child(
            row()
                .w_full()
                .min_w(0.)
                .gaps(6., 24.)
                .flex_wrap()
                .justify_between()
                .pb(10.)
                .child(h3("Identified vehicles", 16.))
                .child(meta(format!("{} identified", h.vehicles.len()))),
        );
    if h.vehicles.is_empty() {
        return list.child(if running {
            empty(
                "vehicles-empty",
                "Listening / no heartbeat",
                "No autopilot heartbeat on this endpoint yet. Ground stations and companion computers are ignored; only autopilots identify a vehicle.",
            )
        } else {
            empty(
                "vehicles-empty",
                "Not monitoring",
                "Start monitoring to listen for an autopilot heartbeat. A vehicle is identified only after its heartbeat arrives.",
            )
        });
    }
    let mut keys: Vec<&String> = h.vehicles.keys().collect();
    keys.sort();
    for key in keys {
        let age = h.seen.get(key).map(|t| t.elapsed());
        list = list.child(vehicle(
            key,
            &h.vehicles[key],
            age,
            h.selected.as_ref() == Some(key),
        ));
    }
    let fresh = h
        .selected
        .as_ref()
        .is_some_and(|key| h.seen.get(key).is_some_and(|t| t.elapsed() < FRESH));
    let hint = if h.selected.is_none() {
        "Select a vehicle to read its parameters."
    } else if !fresh {
        "The selected vehicle's heartbeat is stale. Parameters need a fresh heartbeat."
    } else {
        "Reads every parameter once and saves a raw backup. Nothing is written to the vehicle."
    };
    let actions = if compact {
        col().gap(4.).items_start()
    } else {
        row().gap(20.).center_y()
    };
    list.child(
        actions
            .w_full()
            .min_w(0.)
            .pt(12.)
            .border_sides([1., 0., 0., 0.], soft())
            .child(ghost(
                "read-parameters",
                "Read parameters",
                running && fresh,
                parameters,
            ))
            .child(p(hint, 11.).grow(if compact { 0. } else { 1. })),
    )
}

pub(crate) fn monitor(s: &State, vp: &Viewport) -> El {
    let compact = vp.compact();
    let running = systems::running(s, "hardware");
    let capture = s.hardware.path.clone();
    let capture_row = col()
        .w_full()
        .min_w(0.)
        .gap(8.)
        .child(
            row()
                .gaps(4., 20.)
                .center_y()
                .flex_wrap()
                .child(ghost(
                    "open-hardware-capture",
                    "Open capture folder ↗",
                    !capture.is_empty(),
                    move |s| s.open(capture.clone()),
                ))
                .child(if s.hardware.path.is_empty() {
                    meta_sized("No capture this session", 9., muted())
                } else {
                    code(&s.hardware.path, 10., muted()).wrap_anywhere()
                }),
        )
        .child(notice(
            "Captures are native MAVLink .tlog files. Parameter backups keep raw wire values and types. Edit parameters and retrieve onboard flight logs in QGroundControl or Mission Planner.",
        ));
    col()
        .key("hardware-workspace")
        .w_full()
        .min_w(0.)
        .pt(18.)
        .pb(8.)
        .child(section_head(
            "Vehicle monitor",
            meta("Windows serial / UDP / TCP / read-only").wrap(),
            compact,
        ))
        .child(asset(
            source(s, running, compact),
            ports(s, running),
            compact,
        ))
        .child(vehicles(s, running, compact))
        .child(capture_row)
}

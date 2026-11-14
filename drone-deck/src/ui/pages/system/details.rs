//! Connection details: the addresses the active system's tools use, ready to copy into
//! your own code and tools, so nothing has to be read off a diagram and typed.
use super::*;
#[cfg(test)]
#[path = "details_tests.rs"]
mod tests;

/// One address: element key suffix, what it is for, the text to copy, and a note.
pub(super) struct Line {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
    pub note: &'static str,
}

/// The addresses of `sys`, whose simulated link arrives on TCP `sim_port`.
pub(super) fn lines(sys: &systems::System, sim_port: u16) -> Vec<Line> {
    let c = &sys.consumers;
    let mut out = Vec::new();
    if sys.simulated() {
        out.push(Line {
            key: "link",
            label: "Vehicle link",
            value: format!("tcp:127.0.0.1:{sim_port}"),
            note: "Where Drone Deck reads the simulator. Your tools get the vehicle from the lines below, not from here.",
        });
    } else if let Some(endpoint) = sys.endpoint().filter(|e| !e.trim().is_empty()) {
        out.push(Line {
            key: "link",
            label: "Vehicle endpoint",
            value: endpoint.trim().to_string(),
            note: "The vehicle connection this system opens.",
        });
    }
    if let Some(addr) = c.scripts_udp.as_deref().filter(|_| !sys.is_replay()) {
        out.push(Line {
            key: "mavsdk",
            label: "MAVSDK",
            value: format!("udpin://{addr}"),
            note: "Pass to System.connect(system_address=…). The vehicle link sends MAVLink here and forwards replies to the vehicle.",
        });
        out.push(Line {
            key: "pymavlink",
            label: "pymavlink",
            value: format!("udpin:{addr}"),
            note: "Pass to mavutil.mavlink_connection(…).",
        });
    }
    if c.qgc && !sys.is_replay() {
        out.push(Line {
            key: "qgc",
            label: "QGroundControl",
            value: "udp:127.0.0.1:14550".into(),
            note: "QGroundControl listens here by default and connects on its own.",
        });
    }
    if c.ros.agent || c.ros.recorder {
        out.push(Line {
            key: "ros",
            label: "ROS 2 domain",
            value: format!("export ROS_DOMAIN_ID={}", c.ros.domain),
            note: "Run this in the WSL terminal before your ROS 2 nodes, so they share the system's domain.",
        });
    }
    if c.foxglove {
        out.push(Line {
            key: "foxglove",
            label: "Foxglove live",
            value: "ws://localhost:8765".into(),
            note: "Open connection → Foxglove WebSocket, once the Foxglove bridge runs.",
        });
    }
    out
}

fn copy(s: &mut State, tx: &mut rune::Tx, label: &str, value: &str) {
    tx.copy(value.to_string());
    s.notice = format!("{label} address copied.");
}

/// The section (`connection-details`), with a copy button per line (`copy-<key>`).
pub(super) fn section(s: &State, vp: &Viewport) -> El {
    let sys = s.systems.current();
    let lines = lines(sys, vehicle::sim_port(s));
    if lines.is_empty() {
        return el();
    }
    let compact = vp.compact();
    let mut list = col().w_full().min_w(0.);
    for line in lines {
        let (label, value) = (line.label, line.value.clone());
        let text = col()
            .grow(1.)
            .min_w(0.)
            .gap(4.)
            .child(caps(line.label, 10., 500., 0.08, fg()))
            .child(
                code(line.value.clone(), 12., fg())
                    .key(format!("detail-{}", line.key))
                    .selectable(true)
                    .wrap_anywhere(),
            )
            .child(meta(line.note).wrap());
        let button = ghost(format!("copy-{}", line.key), "Copy", true, |_| {})
            .on_press_tx(move |s: &mut State, tx| copy(s, tx, label, &value));
        let row = if compact {
            col().gap(8.).items_start().child(text).child(button)
        } else {
            row().gap(24.).center_y().child(text).child(button)
        };
        list = list.child(ruled(true).py(14.).child(row.w_full()));
    }
    col()
        .key("connection-details")
        .w_full()
        .min_w(0.)
        .pt(24.)
        .pb(8.)
        .child(section_head(
            "Connection details",
            meta("Addresses for your own code and tools").wrap(),
            compact,
        ))
        .child(list)
}

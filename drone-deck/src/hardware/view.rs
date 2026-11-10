//! The physical-vehicle connection on the System page: the system's endpoint, connect, serial
//! devices and link status (state, receive rate, bad frames, `.tlog`, bridged peers).
//! The active system decides that this panel is shown.
use super::{DEFAULT_MESSAGE, connected, save_endpoint, saved_endpoint, toggle};
use crate::mavlink::{LinkState, VehicleSnapshot};
use crate::ui::{kit::*, theme::*};
use crate::*;
use rune::Color;

/// A file size in the unit that keeps small files from reading as 0.0 MiB.
pub(crate) fn size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.0} KiB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.)
    }
}

fn saved(s: &State) -> El {
    let name = s.systems.current().label().to_string();
    let typed = s.hardware.endpoint.text.trim();
    let text = match saved_endpoint(s) {
        Some(e) if !e.is_empty() => format!("Saved with {name}: {e}"),
        _ => format!("No endpoint saved with {name}"),
    };
    let differs = saved_endpoint(s).is_some_and(|e| e != typed) && !typed.is_empty();
    row()
        .w_full()
        .min_w(0.)
        .gaps(4., 20.)
        .center_y()
        .flex_wrap()
        .child(meta_sized(text, 9., muted()).wrap_anywhere())
        .child(if differs {
            ghost(
                "hardware-save-endpoint",
                "Save to system",
                true,
                save_endpoint,
            )
            .help("Keeps this endpoint with the system for the next connection")
        } else {
            el()
        })
}

fn source(s: &State, on: bool, compact: bool) -> El {
    let h = &s.hardware;
    let bridged = systems::physical_bridge(s.systems.current());
    let actions = row()
        .gaps(8., 20.)
        .center_y()
        .flex_wrap()
        .child(pill(
            "monitor-start",
            if on {
                "Disconnect"
            } else if bridged {
                "Connect (bridged)"
            } else {
                "Connect read-only"
            },
            true,
            toggle,
        ))
        .child(ghost("scan-serial", "Find serial devices", !on, |s| {
            crate::studio::request(s, "ports")
        }));
    let endpoint = field(&h.endpoint, "hardware-endpoint", "udp:127.0.0.1:14555");
    col()
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(p(
            if bridged {
                "Drone Deck itself sends no flight commands, parameter writes or ground-station heartbeat, but QGroundControl and scripts enabled for this system are bridged both ways and can. Use a dedicated telemetry endpoint; another app may already own the serial port."
            } else {
                "Listen without sending flight commands, parameter writes or a ground-station heartbeat. Use a dedicated telemetry endpoint; another app may already own the serial port."
            },
            11.,
        ))
        .child(labeled(
            "Vehicle endpoint",
            if on {
                endpoint.read_only(true).dim(0.6)
            } else {
                endpoint.on_edit(|s: &mut State| &mut s.hardware.endpoint)
            },
        ))
        .child(saved(s))
        .child(
            code(
                "serial:COM4:115200   udp:0.0.0.0:14550   tcp:192.168.1.10:5760",
                10.,
                muted(),
            )
            .wrap_anywhere(),
        )
        .child(actions)
        .child(if h.message != DEFAULT_MESSAGE {
            body(&h.message, 11., fg())
                .key("hardware-message")
                .role("status")
                .wrap()
                .max_w(if compact { 10_000. } else { 620. })
        } else {
            el()
        })
}

fn ports(s: &State, on: bool) -> El {
    let list = col()
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
    let mut list = list;
    for port in ports {
        let id = port["DeviceID"].as_str().unwrap_or("").to_string();
        let name = port["Name"].as_str().unwrap_or("Unnamed device");
        let endpoint = format!("serial:{id}:115200");
        list = list.child(
            ghost(
                format!("port-{id}"),
                &format!("Use {id} / {name}"),
                !on,
                move |s| s.hardware.endpoint = TextEdit::new(endpoint.clone()),
            )
            .help(format!("Sets the endpoint to serial:{id}:115200")),
        );
    }
    list
}

fn stat(key: &str, label: &str, value: String, color: Color) -> El {
    col()
        .key(key)
        .w(150.)
        .min_w(0.)
        .gap(6.)
        .child(meta_sized(label, 9., muted()))
        .child(meta_sized(value, 11., color).wrap_anywhere())
}

fn heartbeat(snap: &VehicleSnapshot) -> (String, Color) {
    match &snap.vehicle {
        None => ("None yet".into(), muted()),
        Some(_) if snap.fresh => ("Fresh".into(), success()),
        Some(v) => (
            format!("None for {}", crate::vehicle::view::silence(v)),
            warn(),
        ),
    }
}

/// What QGroundControl and scripts can do through the link, for the peers `to`.
fn peers_notice(to: &[String], connected: bool) -> El {
    notice(format!(
        "{} {}. Drone Deck itself sends only read requests (one home-position request at link-up, and the parameter and log downloads you start), but an application there (QGroundControl, a script) can send anything to the vehicle through this link, including commands and parameter writes.",
        if connected { "Forwarded to" } else { "Once connected, the link forwards to" },
        to.join(", ")
    ))
    .key("hardware-peers")
}

/// The peers the active system's consumers name, before a link reports its own.
fn configured_peers(s: &State) -> Vec<String> {
    let c = &s.systems.current().consumers;
    let mut out = Vec::new();
    if c.qgc {
        out.push(format!("QGroundControl ({})", vehicle::QGC_PEER));
    }
    if let Some(addr) = &c.scripts_udp {
        out.push(format!("scripts ({addr})"));
    }
    out
}

fn status(s: &State, on: bool, compact: bool) -> El {
    let block = col()
        .key("hardware-link")
        .w_full()
        .min_w(0.)
        .gap(12.)
        .pb(8.);
    let Some(snap) = s.vehicle.shown_for(s.systems.active_id()).filter(|_| on) else {
        let block = block.child(notice(match &s.vehicle.error {
            Some(e) if s.vehicle.owner.as_deref() == Some(s.systems.active_id()) => {
                format!("Not connected. Last error: {e}")
            }
            _ => "Not connected.".to_string(),
        }));
        let peers = configured_peers(s);
        return if peers.is_empty() {
            block
        } else {
            block.child(peers_notice(&peers, false))
        };
    };
    let l = &snap.link;
    let (state, state_color) = match &l.state {
        LinkState::Connecting => ("Connecting".to_string(), warn()),
        LinkState::Connected => ("Up".to_string(), success()),
        LinkState::Down(reason) => (format!("Down / {reason}"), danger()),
    };
    let (beat, beat_color) = heartbeat(snap);
    let rate = snap.vehicle.as_ref().map_or_else(
        || "Not measured yet".to_string(),
        |v| {
            let total: f32 = v.message_rates.iter().map(|(_, r)| r).sum();
            format!("{total:.0} msg/s")
        },
    );
    // Everything written to the vehicle counts in `tx_frames`: the read requests made here
    // and what forwarding peers (QGroundControl, scripts) sent through the bridge, which the
    // service counts in `peer_to_vehicle_frames`. Each number shows only its own part.
    let bridged = l.peer_to_vehicle_frames;
    let sent = match l.tx_frames.saturating_sub(bridged) {
        0 => "Nothing".to_string(),
        n => format!("{n} read requests"),
    };
    let readings = row()
        .key("hardware-readings")
        .w_full()
        .min_w(0.)
        .gaps(16., if compact { 16. } else { 24. })
        .flex_wrap()
        .py(16.)
        .border_sides([1., 0., 1., 0.], soft())
        .child(stat("link-state", "Link", state, state_color))
        .child(stat(
            "link-heartbeat",
            "Autopilot heartbeat",
            beat,
            beat_color,
        ))
        .child(stat("link-rate", "Receive rate", rate, fg()))
        .child(stat(
            "link-frames",
            "Frames received",
            format!("{} / {}", l.rx_frames, size(l.rx_bytes)),
            fg(),
        ))
        .child(stat(
            "link-bad",
            "Bad frames",
            l.bad_frames.to_string(),
            if l.bad_frames > 0 { warn() } else { fg() },
        ))
        .child(stat("link-sent", "Sent by Drone Deck", sent, fg()))
        .child(if l.peers.is_empty() {
            el()
        } else {
            stat(
                "link-peer-to-vehicle",
                "Sent by QGroundControl/scripts",
                format!("{bridged} frames"),
                if bridged > 0 { warn() } else { fg() },
            )
        });
    let tlog = l.tlog.clone();
    let folder = tlog
        .as_ref()
        .and_then(|p| p.parent())
        .map(|p| p.display().to_string());
    let capture = col()
        .key("hardware-capture")
        .w_full()
        .min_w(0.)
        .gap(6.)
        .child(meta_sized("Telemetry capture (.tlog)", 9., muted()))
        .child(match &tlog {
            Some(p) => code(
                format!("{} / {}", p.display(), size(l.tlog_bytes)),
                10.,
                fg(),
            )
            .wrap_anywhere(),
            None => meta_sized("No capture", 9., muted()),
        })
        .child(match &l.tlog_error {
            Some(e) => body(format!("Capture stopped: {e}"), 11., warn()).wrap(),
            None => el(),
        })
        .child(ghost(
            "open-hardware-capture",
            "Open capture folder ↗",
            folder.is_some(),
            move |s| {
                if let Some(f) = &folder {
                    s.open(f.clone())
                }
            },
        ));
    let mut block = block.child(readings).child(capture);
    if !l.peers.is_empty() {
        let peers: Vec<String> = l
            .peers
            .iter()
            .filter_map(|p| p.addr.map(|a| a.to_string()))
            .collect();
        block = block.child(peers_notice(&peers, true));
    }
    if let Some(e) = &s.vehicle.error {
        block = block.child(
            body(e, 11., warn())
                .key("hardware-error")
                .role("status")
                .wrap(),
        );
    }
    block
}

pub(crate) fn monitor(s: &State, vp: &Viewport) -> El {
    let compact = vp.compact();
    let on = connected(s);
    col()
        .key("hardware-workspace")
        .w_full()
        .min_w(0.)
        .pt(18.)
        .pb(8.)
        .child(section_head(
            "Vehicle connection",
            meta(if systems::physical_bridge(s.systems.current()) {
                "Serial / UDP / TCP / Drone Deck sends no GCS heartbeat"
            } else {
                "Serial / UDP / TCP / read-only, no GCS heartbeat"
            })
            .wrap(),
            compact,
        ))
        .child(asset(source(s, on, compact), ports(s, on), compact))
        .child(status(s, on, compact))
}

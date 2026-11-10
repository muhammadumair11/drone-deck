//! Fake snapshots for headless scenes and tests. Nothing here opens a socket.
use super::{ROUTER_UDP_PORT, SIM_TCP_PORT};
use crate::agent::RouterStatus;
use crate::mavlink::{
    Autopilot, LinkState, LinkStatus, PeerStatus, SimulationEvidence, StatusText, SystemSeen,
    VehicleSnapshot, VehicleState,
};
use crate::systems::{self, Source};
use crate::{State, TextEdit};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

/// A hovering PX4 quadcopter 12 m above home. `fresh: false` makes its last heartbeat 9 s old.
pub(crate) fn fake_snapshot(fresh: bool) -> VehicleSnapshot {
    let now = Instant::now();
    let mut v = VehicleState::new(1, 1, now);
    v.autopilot = Autopilot::Px4;
    v.vehicle_type = 2;
    v.armed = true;
    v.mode = "Hold".into();
    v.system_status = 4;
    v.lat = Some(47.397_742);
    v.lon = Some(8.545_594);
    v.alt_msl = Some(500.4);
    v.relative_alt = Some(12.4);
    v.heading = Some(92.0);
    v.roll = Some(2.5);
    v.pitch = Some(-1.5);
    v.yaw = Some(92.0);
    v.groundspeed = Some(0.3);
    v.climb = Some(0.0);
    v.battery_voltage = Some(15.9);
    v.battery_remaining = Some(81);
    v.gps_fix = 3;
    v.satellites = Some(14);
    v.eph = Some(0.8);
    v.sensors_present = 0x0020_FC2F;
    v.sensors_enabled = 0x0020_FC2F;
    v.sensors_health = 0x0020_FC2F;
    v.landed_state = 2;
    v.message_rates = vec![("ATTITUDE", 50.0), ("HEARTBEAT", 1.0), ("SYS_STATUS", 2.0)];
    v.last_heartbeat = if fresh {
        now
    } else {
        now.checked_sub(Duration::from_secs(9)).unwrap_or(now)
    };
    for (severity, text) in [
        (6, "Ready for takeoff!"),
        (4, "Preflight: GPS horizontal accuracy marginal"),
    ] {
        v.push_status(StatusText {
            severity,
            text: text.into(),
            at: SystemTime::now(),
        });
    }
    VehicleSnapshot {
        link: LinkStatus {
            endpoint: "tcp:127.0.0.1:5760".into(),
            state: LinkState::Connected,
            rx_bytes: 482_113,
            rx_frames: 9_204,
            tx_frames: 61,
            peer_to_vehicle_frames: 0,
            bad_frames: 0,
            dropped_events: 0,
            peers: vec![PeerStatus {
                addr: "127.0.0.1:14550".parse().ok(),
                rx_frames: 120,
                tx_frames: 9_204,
                error: None,
            }],
            tlog: Some(PathBuf::from(
                "workspace/logs/tlog/2026-11-10_09-41-07.tlog",
            )),
            tlog_bytes: 512_440,
            tlog_error: None,
        },
        vehicle: Some(v),
        fresh,
        systems: vec![SystemSeen {
            system_id: 1,
            component_id: 1,
            vehicle_type: 2,
            autopilot: 12,
        }],
        params_progress: None,
        log_progress: None,
    }
}

/// Headless only: shows `fake_snapshot(fresh)` as the link of system `owner`. `granted`
/// stands in for the simulation evidence of a running app-owned simulator, so a scene can
/// draw enabled flight actions; commands still have no link to go to.
pub(crate) fn preview(s: &mut State, owner: &str, fresh: bool, granted: bool) {
    if !s.headless {
        return;
    }
    let mut snapshot = fake_snapshot(fresh);
    if let Some(Source::Physical { endpoint }) = s.systems.get_mut(owner).map(|x| &mut x.source) {
        // A physical link always opens on the system's saved endpoint: give the preview one, so
        // the topology, the connection panel and the link all show the same endpoint.
        if endpoint.trim().is_empty() {
            *endpoint = "serial:COM4:57600".into();
        }
        snapshot.link.endpoint = endpoint.trim().into();
        snapshot.link.peers.clear();
        s.hardware.endpoint = TextEdit::new(&snapshot.link.endpoint);
    } else if s.systems.get(owner).is_some_and(systems::routed) {
        // The link of a PX4 system arrives through the router; show it forwarding.
        s.vehicle.router.started = true;
        s.agent.router = Some(RouterStatus {
            state: "forwarding".into(),
            clients: 1,
            packets: 9_204,
            to_vehicle: 61,
            udp_listen: Some(ROUTER_UDP_PORT),
            tcp_port: Some(SIM_TCP_PORT),
            ..Default::default()
        });
    }
    if let Some(v) = &snapshot.vehicle {
        s.vehicle.status = v.status.iter().cloned().collect();
    }
    s.vehicle.endpoint = snapshot.link.endpoint.clone();
    s.vehicle.snapshot = Some(snapshot);
    s.vehicle.owner = Some(owner.to_string());
    s.vehicle.preview = Some(SimulationEvidence {
        app_router_link: granted,
        simulator_source: granted,
        stack_owned_running: granted,
    });
}

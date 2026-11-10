//! Hand-built EVENT frames in PX4's layout (see the sources in `events.rs`).
use super::*;
use crate::mavlink::fake_vehicle::{FakeVehicle, is_home_request, wait_for};
use crate::mavlink::frame::{Encoder, Framer, Msg};
use crate::mavlink::{Config, Endpoint, LinkOrigin, Service, VehicleState};
use std::time::{Duration, Instant};

const GPS: u32 = 1 << 3;
const BATTERY: u32 = 1 << 11;
const MAGNETOMETER: u32 = 1 << 27;
const MANUAL: u32 = 1 << 0;
const POSITION: u32 = 1 << 2;
const HOLD: u32 = 1 << 4;

/// Packs a u8 then little-endian u32s, as PX4's `fillEventArguments` does.
fn args(first: u8, words: &[u32]) -> [u8; 40] {
    let mut out = [0u8; 40];
    out[0] = first;
    for (i, w) in words.iter().enumerate() {
        out[1 + i * 4..5 + i * 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

fn event(id: u32, sequence: u16, arguments: [u8; 40]) -> Msg {
    Msg::EVENT(dialect::EVENT_DATA {
        id,
        event_time_boot_ms: 12_000,
        sequence,
        destination_component: 0,
        destination_system: 0,
        // Protocol (8) internal and external, as PX4 sends both summaries.
        log_levels: 0x88,
        arguments,
    })
}

/// One report: arming summary at `seq`, one individual check, health summary at `seq + 2`.
fn report(seq: u16, errors: u32, warnings: u32, can_arm: u32, health: [u32; 3]) -> Vec<Msg> {
    let check = {
        let mut a = [0u8; 40];
        a[..4].copy_from_slice(&POSITION.to_le_bytes());
        a[4] = 3;
        Msg::EVENT(dialect::EVENT_DATA {
            id: px4_event_id("check_gps_fix"),
            sequence: seq.wrapping_add(1),
            log_levels: 0x33,
            arguments: a,
            ..Default::default()
        })
    };
    vec![
        event(
            ARMING_SUMMARY_ID,
            seq,
            args(0, &[errors, warnings, can_arm, can_arm]),
        ),
        check,
        event(HEALTH_SUMMARY_ID, seq.wrapping_add(2), args(0, &health)),
    ]
}

fn reset(sequence: u16) -> Msg {
    Msg::CURRENT_EVENT_SEQUENCE(dialect::CURRENT_EVENT_SEQUENCE_DATA {
        sequence,
        flags: dialect::MavEventCurrentSequenceFlags::MAV_EVENT_CURRENT_SEQUENCE_FLAGS_RESET,
    })
}

/// Encodes, frames and decodes each message as the link would.
fn over_the_wire(msgs: &[Msg]) -> Vec<Msg> {
    let mut enc = Encoder::new(1, 1);
    let mut bytes = Vec::new();
    for m in msgs {
        bytes.extend_from_slice(&enc.encode(m).bytes);
    }
    let mut framer = Framer::new();
    let mut frames = Vec::new();
    framer.push(&bytes, &mut frames);
    assert_eq!(framer.bad_frames, 0, "EVENT frames pass the dialect CRC");
    frames.iter().filter_map(|f| f.decode()).collect()
}

#[test]
fn event_ids_are_px4s_fnv1a_hash_in_the_autopilot_component() {
    // Published FNV-1a 32-bit test vectors.
    assert_eq!(fnv1a(""), 0x811c_9dc5);
    assert_eq!(fnv1a("a"), 0xe40c_292c);
    assert_eq!(fnv1a("foobar"), 0xbf9c_f968);
    assert_eq!(ARMING_SUMMARY_ID >> 24, 1);
    assert_eq!(
        ARMING_SUMMARY_ID & 0xff_ffff,
        fnv1a("commander_arming_check_summary") & 0xff_ffff
    );
    assert_ne!(ARMING_SUMMARY_ID, HEALTH_SUMMARY_ID);
}

#[test]
fn a_summary_pair_from_the_wire_becomes_the_report() {
    let now = Instant::now();
    let mut v = VehicleState::new(1, 1, now);
    let msgs = over_the_wire(&report(
        40,
        GPS | MAGNETOMETER,
        BATTERY,
        MANUAL,
        [GPS | BATTERY | MAGNETOMETER, GPS, BATTERY],
    ));
    assert_eq!(msgs.len(), 3);
    assert!(
        !v.apply(&msgs[0], now),
        "half a report changes nothing visible"
    );
    assert!(!v.apply(&msgs[1], now), "individual checks are not decoded");
    assert!(v.apply(&msgs[2], now));
    let r = *v.health_report().expect("report");
    assert_eq!(
        r,
        HealthReport {
            arming_errors: GPS | MAGNETOMETER,
            arming_warnings: BATTERY,
            can_arm: MANUAL,
            can_run: MANUAL,
            present: GPS | BATTERY | MAGNETOMETER,
            health_errors: GPS,
            health_warnings: BATTERY,
        }
    );
    assert!(r.can_arm_in(MANUAL) && !r.can_arm_in(POSITION));
    assert_eq!(
        component_names(r.error_components()),
        ["GPS", "Magnetometer"]
    );
    assert_eq!(component_names(r.arming_warnings), ["Battery"]);
    assert_eq!(mode_group_names(r.can_arm), ["Manual"]);
    // The same report again (re-sent on request) is no visible change.
    for m in &msgs {
        assert!(!v.apply(m, now));
    }
}

#[test]
fn unpaired_or_mismatched_summaries_are_not_a_report() {
    let now = Instant::now();
    let mut v = VehicleState::new(1, 1, now);
    let pair = report(7, GPS, 0, 0, [GPS, GPS, 0]);
    assert!(!v.apply(&pair[2], now), "health without its arming summary");
    // Another chunk index.
    v.apply(&pair[0], now);
    assert!(!v.apply(&event(HEALTH_SUMMARY_ID, 9, args(1, &[GPS, GPS, 0])), now));
    // A health summary far after the arming summary belongs to another report.
    v.apply(&pair[0], now);
    assert!(!v.apply(
        &event(HEALTH_SUMMARY_ID, 7 + 500, args(0, &[GPS, GPS, 0])),
        now
    ));
    assert!(v.health_report().is_none());
    // Sequence numbers wrap.
    for m in report(u16::MAX, 0, 0, POSITION, [GPS, 0, 0]) {
        v.apply(&m, now);
    }
    assert_eq!(v.health_report().map(|r| r.can_arm), Some(POSITION));
}

#[test]
fn a_sequence_reset_drops_the_report_of_the_previous_boot() {
    let now = Instant::now();
    let mut v = VehicleState::new(1, 1, now);
    for m in report(300, 0, 0, POSITION, [GPS, 0, 0]) {
        v.apply(&m, now);
    }
    let plain = Msg::CURRENT_EVENT_SEQUENCE(dialect::CURRENT_EVENT_SEQUENCE_DATA {
        sequence: 302,
        flags: dialect::MavEventCurrentSequenceFlags::empty(),
    });
    assert!(!v.apply(&plain, now));
    assert!(v.health_report().is_some());
    assert!(v.apply(&reset(0), now), "rebooted: the old report is gone");
    assert!(v.health_report().is_none());
    assert!(!v.apply(&reset(0), now));
    // A reset announced just after the new boot's arming summary keeps that summary.
    let fresh = report(0, GPS, 0, 0, [GPS, GPS, 0]);
    v.apply(&fresh[0], now);
    v.apply(&reset(0), now);
    assert!(v.apply(&fresh[2], now));
    assert_eq!(v.health_report().map(|r| r.arming_errors), Some(GPS));
}

fn current(sequence: u16) -> Msg {
    Msg::CURRENT_EVENT_SEQUENCE(dialect::CURRENT_EVENT_SEQUENCE_DATA {
        sequence,
        flags: dialect::MavEventCurrentSequenceFlags::empty(),
    })
}

fn other(sequence: u16) -> Msg {
    event(px4_event_id("check_gps_fix"), sequence, [0; 40])
}

#[test]
fn a_lost_event_after_the_report_marks_it_possibly_outdated() {
    let now = Instant::now();
    let mut v = VehicleState::new(1, 1, now);
    for m in report(40, 0, 0, POSITION, [GPS, 0, 0]) {
        v.apply(&m, now);
    }
    assert!(!v.health_events.outdated());
    // In order and duplicates: nothing went missing.
    assert!(!v.apply(&other(43), now));
    assert!(!v.apply(&other(43), now));
    assert!(!v.apply(&other(42), now), "a late duplicate");
    assert!(!v.health_events.outdated());
    // 44 never arrives: it may have been the next report's arming summary.
    assert!(
        v.apply(&other(45), now),
        "the view learns the report may be outdated"
    );
    assert!(v.health_events.outdated());
    assert!(!v.apply(&other(47), now), "already marked: no redraw");
    assert!(v.health_report().is_some(), "the report is kept, marked");
    // A new complete report is current again, even with the same contents.
    for m in report(48, 0, 0, POSITION, [GPS, 0, 0]) {
        v.apply(&m, now);
    }
    assert!(!v.health_events.outdated());
}

#[test]
fn current_event_sequence_ahead_of_the_last_event_marks_the_report() {
    let now = Instant::now();
    let mut v = VehicleState::new(1, 1, now);
    // Announced before any event: the starting point, nothing is missing.
    assert!(!v.apply(&current(9), now));
    for m in report(10, 0, 0, POSITION, [GPS, 0, 0]) {
        v.apply(&m, now);
    }
    // The periodic sequence matches the last event received (12).
    assert!(!v.apply(&current(12), now));
    assert!(!v.apply(&current(11), now), "behind: an old announcement");
    assert!(!v.health_events.outdated());
    // The vehicle sent 13 (a link outage, or a lost frame) that never arrived.
    assert!(v.apply(&current(13), now));
    assert!(v.health_events.outdated());
    assert!(!v.apply(&current(14), now));
    // A reboot drops the report and the mark with it.
    assert!(v.apply(&reset(0), now));
    assert!(!v.health_events.outdated());
}

#[test]
fn sequence_gaps_wrap_like_libevents() {
    let now = Instant::now();
    let mut v = VehicleState::new(1, 1, now);
    for m in report(u16::MAX - 2, 0, 0, POSITION, [GPS, 0, 0]) {
        v.apply(&m, now);
    }
    assert!(!v.apply(&other(0), now), "65535 then 0 is in order");
    assert!(!v.apply(&current(0), now));
    assert!(!v.health_events.outdated());
    assert!(v.apply(&current(1), now));
    assert!(v.health_events.outdated());
    // A sequence far behind (a reboot whose reset was not seen) restarts the count.
    let mut v = VehicleState::new(1, 1, now);
    for m in report(30_000, 0, 0, POSITION, [GPS, 0, 0]) {
        v.apply(&m, now);
    }
    assert!(!v.apply(&other(3), now));
    assert!(!v.apply(&other(4), now));
    assert!(!v.health_events.outdated());
}

#[test]
fn heartbeat_modes_map_to_px4_mode_groups() {
    let px4 = Autopilot::Px4;
    let mode = |main: u32, sub: u32| (main << 16) | (sub << 24);
    assert_eq!(mode_group(px4, 1, mode(4, 3)), Some((HOLD, "Hold")));
    assert_eq!(mode_group(px4, 1, mode(3, 0)), Some((POSITION, "Position")));
    assert_eq!(
        mode_group(px4, 1, mode(3, 2)),
        Some((1 << 6, "Position Slow"))
    );
    assert_eq!(mode_group(px4, 1, mode(4, 4)), Some((1 << 3, "Mission")));
    assert_eq!(mode_group(px4, 1, mode(4, 2)), Some((1 << 17, "Takeoff")));
    assert_eq!(
        mode_group(px4, 1, mode(7, 0)),
        Some((1 << 15, "Stabilized"))
    );
    assert_eq!(
        mode_group(px4, 1, mode(4, 11)),
        Some((1 << 23, "External 1"))
    );
    assert_eq!(mode_group(px4, 1, mode(1, 0)), Some((MANUAL, "Manual")));
    assert_eq!(mode_group(px4, 1, mode(4, 1)), None, "Ready has no group");
    assert_eq!(
        mode_group(px4, 1, mode(10, 0)),
        None,
        "Termination has no group"
    );
    assert_eq!(
        mode_group(px4, 1, mode(4, 20)),
        None,
        "Descend has no group"
    );
    assert_eq!(
        mode_group(px4, 0, mode(4, 3)),
        None,
        "custom mode not enabled"
    );
    assert_eq!(mode_group(Autopilot::ArduPilot, 1, 5), None);
}

#[test]
fn component_and_mode_names_follow_px4s_enums() {
    assert_eq!(component_names(1), ["General"]);
    assert_eq!(component_names(1 << 31), ["Traffic avoidance"]);
    assert_eq!(component_names(0), Vec::<&str>::new());
    // Bits 11..=13 and 16 are no group; nav_state 31 does not exist.
    assert_eq!(mode_group_names(0xffff_ffff).len(), 27);
}

/// On a physical link the report arrives passively: Drone Deck sends no REQUEST_EVENT
/// (nor anything else but its one HOME_POSITION request).
#[test]
fn a_physical_link_reads_the_report_without_requesting_anything() {
    let fake = FakeVehicle::tcp();
    fake.with(|s| s.extra = report(5, MAGNETOMETER, 0, MANUAL, [MAGNETOMETER, MAGNETOMETER, 0]));
    let endpoint: Endpoint = format!("tcp:127.0.0.1:{}", fake.addr.port())
        .parse()
        .expect("endpoint");
    let mut service = Service::start(Config::new(endpoint, LinkOrigin::Physical)).expect("start");
    let limit = Duration::from_secs(10);
    let snap = wait_for(limit, || {
        service.poll().snapshot.filter(|s| {
            s.vehicle
                .as_ref()
                .is_some_and(|v| v.health_report().is_some())
        })
    })
    .expect("a report");
    let v = snap.vehicle.expect("vehicle");
    let r = v.health_report().expect("report");
    assert_eq!(component_names(r.arming_errors), ["Magnetometer"]);
    let (group, name) = mode_group(v.autopilot, v.base_mode, v.custom_mode).expect("group");
    assert_eq!(name, "Position");
    assert!(!r.can_arm_in(group));
    std::thread::sleep(Duration::from_millis(300));
    let sent = fake.received(|m| !matches!(m, Msg::HEARTBEAT(_)) && !is_home_request(m));
    assert!(sent.is_empty(), "nothing but the home request: {sent:?}");
}

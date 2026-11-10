use super::*;
use mavlink::{MAVLinkV1MessageRaw, MavHeader};

fn heartbeat() -> Msg {
    Msg::HEARTBEAT(dialect::HEARTBEAT_DATA {
        custom_mode: 3 << 16,
        mavtype: dialect::MavType::MAV_TYPE_QUADROTOR,
        autopilot: dialect::MavAutopilot::MAV_AUTOPILOT_PX4,
        base_mode: dialect::MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED,
        system_status: dialect::MavState::MAV_STATE_ACTIVE,
        mavlink_version: 3,
    })
}

#[test]
fn encodes_and_decodes_v2() {
    let mut enc = Encoder::new(7, 42);
    let a = enc.encode(&heartbeat());
    let b = enc.encode(&heartbeat());
    assert_eq!(a.version(), MavlinkVersion::V2);
    assert_eq!(
        (a.system_id(), a.component_id(), a.message_id()),
        (7, 42, 0)
    );
    assert_eq!((a.sequence(), b.sequence()), (0, 1));
    assert_eq!(a.decode(), Some(heartbeat()));
}

#[test]
fn splits_a_stream_across_reads_and_skips_garbage() {
    let mut enc = Encoder::new(1, 1);
    let mut stream = vec![0x00, 0x13, 0x37];
    let frames: Vec<Frame> = (0..3).map(|_| enc.encode(&heartbeat())).collect();
    for f in &frames {
        stream.extend_from_slice(&f.bytes);
        stream.push(0x55);
    }
    let mut framer = Framer::new();
    let mut out = Vec::new();
    for chunk in stream.chunks(5) {
        framer.push(chunk, &mut out);
    }
    assert_eq!(out, frames);
    assert_eq!(framer.skipped_bytes, 6);
    assert_eq!(framer.bad_frames, 0);
}

#[test]
fn drops_a_corrupted_frame_and_resyncs() {
    let mut enc = Encoder::new(1, 1);
    let mut bad = enc.encode(&heartbeat()).bytes;
    let n = bad.len();
    bad[n - 1] ^= 0xff;
    let good = enc.encode(&heartbeat());
    let mut bytes = bad;
    bytes.extend_from_slice(&good.bytes);
    let mut framer = Framer::new();
    let mut out = Vec::new();
    framer.push(&bytes, &mut out);
    assert_eq!(out, vec![good]);
    assert_eq!(framer.bad_frames, 1);
}

#[test]
fn reads_mavlink_1() {
    let mut raw = MAVLinkV1MessageRaw::new();
    let header = MavHeader {
        system_id: 3,
        component_id: 1,
        sequence: 9,
    };
    raw.serialize_message(header, &heartbeat());
    let mut out = Vec::new();
    Framer::new().push(raw.raw_bytes(), &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].version(), MavlinkVersion::V1);
    assert_eq!((out[0].system_id(), out[0].sequence()), (3, 9));
    assert_eq!(out[0].decode(), Some(heartbeat()));
}

#[test]
fn passes_unknown_messages_through_undecoded() {
    // Message id 0xABCDE is not in the dialect: the frame is kept for forwarding.
    let mut bytes = vec![
        0xFD, 2, 0, 0, 5, 1, 1, 0xDE, 0xBC, 0x0A, 0xAA, 0xBB, 0x12, 0x34,
    ];
    let mut out = Vec::new();
    Framer::new().push(&bytes, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].message_id(), 0xABCDE);
    assert_eq!(out[0].decode(), None);
    bytes[2] = 0x80; // an incompatibility flag this decoder doesn't support
    let mut out = Vec::new();
    let mut framer = Framer::new();
    framer.push(&bytes, &mut out);
    assert!(out.is_empty());
    assert_eq!(framer.bad_frames, 1);
}

#[test]
fn reads_text_fields() {
    let chars: mavlink::types::CharArray<16> = "SYS_AUTOSTART".into();
    assert_eq!(text(&chars), "SYS_AUTOSTART");
}

#[test]
fn id_cache_stays_bounded_under_noise() {
    let mut framer = Framer::new();
    let mut out = Vec::new();
    for id in 0x10_0000u32..0x10_0000 + 5_000 {
        let [a, b, c, _] = id.to_le_bytes();
        framer.push(&[0xFD, 0, 0, 0, 0, 1, 1, a, b, c, 0, 0], &mut out);
    }
    assert_eq!(out.len(), 5_000, "unknown ids still pass through");
    assert!(framer.known.len() <= KNOWN_CACHE);
    // The cache still answers correctly once full.
    let frame = Encoder::new(1, 1).encode(&heartbeat());
    out.clear();
    framer.push(&frame.bytes, &mut out);
    assert_eq!(out[0].decode(), Some(heartbeat()));
}

use super::*;
use crate::mavlink::fake_vehicle::scratch;
use crate::mavlink::frame::{Encoder, Msg, dialect};
use std::time::Duration;

fn heartbeat(armed: bool) -> Msg {
    let mut flags = dialect::MavModeFlag::MAV_MODE_FLAG_CUSTOM_MODE_ENABLED;
    if armed {
        flags |= dialect::MavModeFlag::MAV_MODE_FLAG_SAFETY_ARMED;
    }
    Msg::HEARTBEAT(dialect::HEARTBEAT_DATA {
        custom_mode: 3 << 16,
        mavtype: dialect::MavType::MAV_TYPE_QUADROTOR,
        autopilot: dialect::MavAutopilot::MAV_AUTOPILOT_PX4,
        base_mode: flags,
        system_status: dialect::MavState::MAV_STATE_ACTIVE,
        mavlink_version: 3,
    })
}

#[test]
fn writes_reads_and_summarizes() {
    let dir = scratch("tlog");
    let path = default_path(&dir, UNIX_EPOCH + Duration::from_secs(1_700_000_000));
    assert!(path.ends_with("2023-11-14_22-13-20.tlog"));
    let mut enc = Encoder::new(1, 1);
    let t0 = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    {
        let mut w = TlogWriter::create(&path, 1_000_000).expect("create");
        w.write(t0, &enc.encode(&heartbeat(false))).expect("write");
        w.write(t0 + Duration::from_secs(1), &enc.encode(&heartbeat(true)))
            .expect("write");
        let pos = Msg::GLOBAL_POSITION_INT(dialect::GLOBAL_POSITION_INT_DATA {
            relative_alt: 30_000,
            ..Default::default()
        });
        w.write(t0 + Duration::from_secs(2), &enc.encode(&pos))
            .expect("write");
        let text = Msg::STATUSTEXT(dialect::STATUSTEXT_DATA {
            text: "Takeoff detected".into(),
            ..Default::default()
        });
        w.write(t0 + Duration::from_secs(3), &enc.encode(&text))
            .expect("write");
        w.write(t0 + Duration::from_secs(4), &enc.encode(&heartbeat(false)))
            .expect("write");
    }
    let mut bytes = std::fs::read(&path).expect("read");
    assert_eq!(&bytes[..8], &micros(t0).to_be_bytes());
    // Damage the second record's timestamp: the reader skips that record and recovers.
    let first = 8 + enc.encode(&heartbeat(false)).bytes.len();
    bytes.splice(first + 1..first + 1, [0xAAu8; 5]);
    let damaged = summarize_bytes(&bytes);
    assert_eq!(damaged.frames, 4);
    assert_eq!(damaged.skipped_bytes, first as u64 + 5);
    assert!(damaged.damaged() && !damaged.truncated);
    let s = summarize(&path).expect("summary");
    assert!(!s.damaged());
    assert_eq!(s.frames, 5);
    assert_eq!(s.duration_s(), 4.0);
    assert_eq!(s.vehicle, Some((1, "PX4".into(), "Quadrotor".into())));
    assert_eq!(s.max_relative_alt, Some(30.0));
    assert_eq!(s.armed_seconds, 3.0);
    assert_eq!(s.modes, vec!["Position".to_string()]);
    assert_eq!(s.status, vec![(0, "Takeoff detected".to_string())]);
    assert_eq!(s.message_counts["HEARTBEAT"], 3);
    assert_eq!((s.v1, s.v2, s.heartbeats()), (0, 5, 3));
    assert_eq!(s.components.get(&(1, 1)), Some(&5));
}

#[test]
fn frames_outside_the_dialect_are_counted_but_not_decoded() {
    let t0 = micros(UNIX_EPOCH + Duration::from_secs(1_700_000_000));
    let mut bytes = t0.to_be_bytes().to_vec();
    bytes.extend_from_slice(&Encoder::new(1, 1).encode(&heartbeat(false)).bytes);
    // MAVLink 2, 1-byte payload, message id 0xABCDEF (unknown, so its CRC can't be checked).
    bytes.extend_from_slice(&(t0 + 1).to_be_bytes());
    bytes.extend_from_slice(&[0xFD, 1, 0, 0, 7, 9, 1, 0xEF, 0xCD, 0xAB, 42, 0, 0]);
    let s = summarize_bytes(&bytes);
    assert_eq!((s.frames, s.unknown_frames), (2, 1));
    assert_eq!(s.message_counts.len(), 1);
    assert_eq!(s.components.get(&(9, 1)), Some(&1));
    assert!(!s.damaged());
    // A file that ends inside the 8-byte timestamp is truncated, not damaged mid-way.
    let cut = summarize_bytes(&bytes[..bytes.len() - 13 - 4]);
    assert!(cut.truncated && cut.skipped_bytes == 0);
}

#[test]
fn stops_at_its_size_limit() {
    let dir = scratch("tlog-limit");
    let mut w = TlogWriter::create(&dir.join("x.tlog"), 40).expect("create");
    let frame = Encoder::new(1, 1).encode(&heartbeat(false));
    assert!(w.write(SystemTime::now(), &frame).is_ok());
    assert!(w.write(SystemTime::now(), &frame).is_err());
    assert_eq!(w.written(), 8 + frame.bytes.len() as u64);
}

#[test]
fn never_overwrites_an_existing_log() {
    let dir = scratch("tlog-unique");
    let path = dir.join("2024-01-01_00-00-00.tlog");
    std::fs::write(&path, b"earlier session").expect("seed");
    let mut w = TlogWriter::create(&path, 1_000).expect("create");
    assert_eq!(w.path(), dir.join("2024-01-01_00-00-00-2.tlog"));
    w.write(
        SystemTime::now(),
        &Encoder::new(1, 1).encode(&heartbeat(false)),
    )
    .expect("write");
    drop(w);
    assert_eq!(std::fs::read(&path).expect("kept"), b"earlier session");
    let w = TlogWriter::create(&path, 1_000).expect("create");
    assert_eq!(w.path(), dir.join("2024-01-01_00-00-00-3.tlog"));
}

#[test]
fn a_capture_without_frames_is_removed() {
    let dir = scratch("tlog-empty");
    let path = dir.join("never-up.tlog");
    let w = TlogWriter::create(&path, 1_000).expect("create");
    assert!(path.exists());
    drop(w);
    assert!(!path.exists(), "an empty capture is not left behind");
}

#[test]
fn streams_large_logs_like_the_in_memory_reader() {
    let dir = scratch("tlog-stream");
    let path = dir.join("big.tlog");
    let mut enc = Encoder::new(1, 1);
    let t0 = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    {
        // Well over the 4 MB read chunk, so records straddle chunk boundaries.
        let mut w = TlogWriter::create(&path, u64::MAX).expect("create");
        for i in 0..200_000u64 {
            let at = t0 + Duration::from_millis(i);
            w.write(at, &enc.encode(&heartbeat(i % 1000 < 500)))
                .expect("write");
        }
    }
    assert!(std::fs::metadata(&path).expect("meta").len() > 5 << 20);
    let streamed = summarize(&path).expect("summary");
    let whole = summarize_bytes(&std::fs::read(&path).expect("read"));
    assert_eq!(streamed.frames, 200_000);
    assert_eq!(streamed.frames, whole.frames);
    assert_eq!(streamed.end_us, whole.end_us);
    assert_eq!(streamed.armed_seconds, whole.armed_seconds);
    assert_eq!(streamed.message_counts, whole.message_counts);
}

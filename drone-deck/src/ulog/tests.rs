use super::fixture::{self, Body, Writer};
use super::parse::{Ulog, Value};
use super::*;

fn flight() -> Ulog {
    parse(fixture::flight()).expect("synthetic flight parses")
}
fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn rejects_files_without_the_ulog_header() {
    assert_eq!(parse(Vec::new()).unwrap_err(), Error::NotUlog);
    assert_eq!(parse(b"ULog\x01\x12".to_vec()).unwrap_err(), Error::NotUlog);
    assert_eq!(parse(vec![0; 64]).unwrap_err(), Error::NotUlog);
    let header_only = Writer::new(42).buf;
    let log = parse(header_only).unwrap();
    assert_eq!(log.start_us, 42);
    assert_eq!(log.version, 1);
    assert!(log.topics.is_empty() && !log.issues.truncated);
}

#[test]
fn reads_definitions_info_params_and_messages() {
    let log = flight();
    assert_eq!(log.info_text("sys_name").as_deref(), Some("PX4"));
    assert_eq!(
        log.info.get("ver_sw_release"),
        Some(&Value::Int(0x010F_00FF))
    );
    assert_eq!(log.params.get("MPC_XY_VEL_MAX"), Some(&Value::Float(12.)));
    assert_eq!(log.params.get("SYS_AUTOSTART"), Some(&Value::Int(10040)));
    assert_eq!(
        log.info_multi.get("perf_counter_preflight"),
        Some(&vec![Value::Text(
            "sensors: 12 events, ekf2: 3 events".into()
        )]),
        "continued multi-info values are joined"
    );
    assert_eq!(log.messages.len(), 3);
    assert_eq!(log.messages[1].level, 4);
    assert_eq!(log.messages[2].text, "Failsafe activated: RTL");
    assert_eq!(log.dropouts.len(), 1);
    assert_eq!(log.dropouts[0].ms, 120);
    assert_eq!(log.syncs, 1);
    assert_eq!(log.first_us, Some(fixture::T0));
    assert_eq!(log.last_us, Some(fixture::T0 + 100_000_000));
    assert_eq!(log.issues, Default::default());
}

#[test]
fn series_by_field_name_skip_padding_and_tolerate_missing_trailing_padding() {
    let log = flight();
    let layout = log
        .topic("vehicle_local_position")
        .next()
        .unwrap()
        .layout
        .clone()
        .unwrap();
    assert!(layout.field("_padding0[0]").is_none());
    assert_eq!(layout.field("xy_valid").unwrap().offset, 32);
    let z = log.series("vehicle_local_position", 0, "z").unwrap();
    assert_eq!(
        z.v.len(),
        501,
        "data written without trailing padding still decodes"
    );
    assert!(close(
        z.v.iter().cloned().fold(f64::MAX, f64::min),
        -20.,
        1e-4
    ));
    let q = log.series("vehicle_attitude", 0, "q[0]").unwrap();
    assert!(close(q.v[0], 1., 1e-6));
    assert!(log.series("vehicle_local_position", 0, "nope").is_none());
    assert!(log.series("not_logged", 0, "x").is_none());
}

#[test]
fn multi_instance_topics_stay_separate() {
    let log = flight();
    assert_eq!(log.instances("battery_status"), vec![0, 1]);
    let a = log.series("battery_status", 0, "voltage_v").unwrap();
    let b = log.series("battery_status", 1, "voltage_v").unwrap();
    assert!(close(a.v[0], 16.8, 1e-4));
    assert!(b.v.iter().all(|&v| v == 0.));
    let s = summarize(&log);
    assert!(
        close(s.battery_min_v.unwrap(), 14.9, 1e-3),
        "disconnected instance ignored"
    );
    assert!(close(s.battery_min_pct.unwrap(), 35., 1e-3));
}

#[test]
fn summary_matches_the_synthetic_flight() {
    let s = summarize(&flight());
    assert!(close(s.duration_s, 100., 1e-9));
    assert!(close(s.max_alt_rel_m.unwrap(), 20., 1e-3));
    assert!(close(s.max_alt_amsl_m.unwrap(), 508., 1e-3));
    assert!(close(s.distance_m.unwrap(), 200., 1e-2));
    assert!(close(s.max_speed_ms.unwrap(), 5., 1e-3));
    assert_eq!(s.failsafes, Some(vec![55.]));
    let armed = s.armed.clone().unwrap();
    assert_eq!(armed.len(), 1);
    assert!(close(armed[0].0, 10., 1e-9) && close(armed[0].1, 92., 1e-9));
    assert_eq!((s.warnings, s.errors, s.messages.len()), (1, 1, 2));
    assert_eq!((s.dropouts, s.dropout_ms), (1, 120));
    let e = s.estimator.clone().unwrap();
    assert_eq!(
        (e.samples, e.fault_samples, e.innovation_fail_samples),
        (501, 0, 5)
    );
    assert_eq!((s.topics, s.params), (8, 2));
    assert!(s.utc_start_us.is_none(), "no GPS time in the fixture");
    assert!(
        s.vehicle
            .contains(&("Software", "v1.15.0 (01234567)".to_string()))
    );
    assert!(s.vehicle.contains(&("System", "PX4".to_string())));
}

#[test]
fn missing_topics_are_not_logged_rather_than_zero() {
    let mut w = Writer::new(0);
    w.format("vehicle_attitude:uint64_t timestamp;float[4] q;")
        .add(0, 0, "vehicle_attitude")
        .data(
            0,
            1_000_000,
            &Body::default().f32(1.).f32(0.).f32(0.).f32(0.).0,
        )
        .data(
            0,
            3_000_000,
            &Body::default().f32(1.).f32(0.).f32(0.).f32(0.).0,
        );
    let log = parse(w.buf.clone()).unwrap();
    let s = summarize(&log);
    assert!(close(s.duration_s, 2., 1e-9));
    assert_eq!(s.max_alt_rel_m, None);
    assert_eq!(s.distance_m, None);
    assert_eq!(s.battery_min_v, None);
    assert_eq!(s.failsafes, None);
    assert_eq!(s.armed, None);
    assert_eq!(s.estimator, None);
    let plots = plots::extract(&log, s.first_us);
    assert_eq!(
        plots.iter().map(|p| p.id).collect::<Vec<_>>(),
        vec!["attitude"]
    );
}

#[test]
fn truncation_at_every_byte_never_panics_and_is_reported() {
    let full = fixture::flight();
    let complete = summarize(&parse(full.clone()).unwrap());
    let mut truncated = 0;
    for cut in (16..full.len()).step_by(97) {
        let log = parse(full[..cut].to_vec()).unwrap();
        if log.issues.truncated {
            truncated += 1;
        }
        let s = summarize(&log);
        assert!(s.duration_s <= complete.duration_s);
    }
    assert!(truncated > 0);
    let half = parse(full[..full.len() / 2].to_vec()).unwrap();
    let s = summarize(&half);
    assert!(
        s.duration_s > 10. && s.duration_s < 100.,
        "{}",
        s.duration_s
    );
    // A cut inside a message header (one byte of the size) is also truncation.
    let mut w = Writer::new(0);
    w.format("a:uint64_t timestamp;");
    w.buf.push(5);
    assert!(parse(w.buf).unwrap().issues.truncated);
}

#[test]
fn corruption_skips_to_the_next_sync_marker() {
    let mut w = Writer::new(0);
    w.format("t:uint64_t timestamp;float v;").add(0, 9, "t");
    w.data(9, 1_000_000, &Body::default().f32(1.).0);
    // Not a message type letter: damage, not a newer message type.
    w.buf.extend_from_slice(&[0x10, 0x00, 0xEE, 1, 2, 3, 4]);
    w.sync().data(9, 2_000_000, &Body::default().f32(2.).0);
    let log = parse(w.buf.clone()).unwrap();
    assert_eq!(log.issues.corrupt_regions, 1);
    assert!(log.issues.skipped_bytes > 0);
    assert_eq!(log.series("t", 0, "v").unwrap().v, vec![1., 2.]);
    // A size that runs past the end with a sync marker after it is a damaged header in the
    // middle of the file, not a cut-off tail: the data after the sync is still read.
    let mut w = Writer::new(0);
    w.format("t:uint64_t timestamp;float v;").add(0, 9, "t");
    w.data(9, 1_000_000, &Body::default().f32(1.).0);
    w.buf.extend_from_slice(&[0xff, 0xff, b'D', 9, 0]);
    w.sync().data(9, 2_000_000, &Body::default().f32(2.).0);
    let log = parse(w.buf).unwrap();
    assert_eq!(log.series("t", 0, "v").unwrap().v, vec![1., 2.]);
    assert!(!log.issues.truncated);
    assert_eq!(log.issues.corrupt_regions, 1);
    // Without a later sync marker the rest is skipped and what came before is kept.
    let mut w = Writer::new(0);
    w.format("t:uint64_t timestamp;float v;").add(0, 9, "t");
    w.data(9, 1_000_000, &Body::default().f32(1.).0);
    w.buf.extend_from_slice(&[0xff, 0xff, 0xee, 9, 9, 9]);
    let log = parse(w.buf).unwrap();
    assert_eq!(log.series("t", 0, "v").unwrap().v, vec![1.]);
    assert_eq!(log.issues.corrupt_regions, 1);
}

#[test]
fn flag_bits_refuse_unknown_incompatible_features_and_accept_appended_data() {
    let mut w = Writer::new(0);
    w.flags(0b10, 0);
    assert!(matches!(parse(w.buf).unwrap_err(), Error::Incompatible(_)));
    let mut w = Writer::new(0);
    w.flags(1, 0);
    w.format("t:uint64_t timestamp;float v;").add(0, 3, "t");
    let jump_from = w.buf.len();
    w.buf.extend_from_slice(&[0xAB; 20]);
    let appended = w.buf.len() as u64;
    w.data(3, 7, &Body::default().f32(4.).0);
    // Patch the first appended offset (flag bits payload byte 16).
    w.buf[16 + 3 + 16..16 + 3 + 24].copy_from_slice(&appended.to_le_bytes());
    let log = parse(w.buf).unwrap();
    assert!(log.appended);
    assert!(jump_from < appended as usize);
    assert_eq!(log.series("t", 0, "v").unwrap().v, vec![4.]);
}

#[test]
fn unknown_message_types_are_skipped_by_their_size() {
    let mut w = Writer::new(0);
    w.format("t:uint64_t timestamp;float v;").add(0, 9, "t");
    w.data(9, 1_000_000, &Body::default().f32(1.).0);
    // A message type from a newer format revision, holding sync-like bytes it must not resync on.
    w.msg(b'Y', &[0x2F, 0x73, 0x13, 0x20, 1, 2, 3]);
    w.data(9, 2_000_000, &Body::default().f32(2.).0);
    let log = parse(w.buf).unwrap();
    assert_eq!(log.series("t", 0, "v").unwrap().v, vec![1., 2.]);
    assert_eq!(log.issues.unknown_messages, 1);
    assert_eq!(log.issues.corrupt_regions, 0);
}

#[test]
fn a_message_cut_off_where_appended_data_starts_is_dropped() {
    let mut w = Writer::new(0);
    w.flags(1, 0);
    w.format("t:uint64_t timestamp;float v;").add(0, 3, "t");
    w.data(3, 1_000_000, &Body::default().f32(1.).0);
    // The log stops five bytes into a 14-byte data message (as when the logger crashed)...
    let mut cut = vec![14, 0, b'D', 3, 0];
    cut.extend_from_slice(&2_000_000u64.to_le_bytes()[..3]);
    w.buf.extend_from_slice(&cut);
    // ...and a crash dump's messages are appended right there.
    let appended = w.buf.len() as u64;
    w.data(3, 3_000_000, &Body::default().f32(3.).0)
        .log(2, 3_000_000, "hardfault");
    w.buf[16 + 3 + 16..16 + 3 + 24].copy_from_slice(&appended.to_le_bytes());
    let log = parse(w.buf).unwrap();
    assert_eq!(log.series("t", 0, "v").unwrap().v, vec![1., 3.]);
    assert_eq!(log.messages.len(), 1);
    assert_eq!(
        log.issues,
        Default::default(),
        "the cut is allowed by the format"
    );
}

#[test]
fn oversized_and_explosive_formats_are_refused_quickly() {
    let mut w = Writer::new(0);
    w.format("huge:uint64_t timestamp;float[99999999999999] x;")
        .format("empty:")
        .format("wide:empty[65000] a;")
        .format("deep:uint64_t timestamp;wide[65000] b;")
        .format("big:uint64_t timestamp;uint8_t[60000] a;uint8_t[60000] b;")
        .add(0, 0, "huge")
        .add(0, 1, "deep")
        .add(0, 2, "big");
    w.data(0, 1, &[]).data(1, 1, &[]).data(2, 1, &[]);
    let started = std::time::Instant::now();
    let log = parse(w.buf).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(log.issues.unresolved, vec!["huge", "deep", "big"]);
}

#[test]
fn random_damage_never_panics() {
    let full = fixture::flight();
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..60 {
        let mut b = full.clone();
        for _ in 0..1 + next() % 40 {
            let at = 16 + (next() as usize) % (b.len() - 16);
            b[at] = next() as u8;
        }
        let cut = 16 + (next() as usize) % (b.len() - 16);
        b.truncate(cut.max(b.len() * 3 / 4));
        if let Ok(log) = parse(b) {
            let s = summarize(&log);
            let _ = plots::extract(&log, s.first_us);
        }
    }
}

#[test]
fn nested_types_arrays_and_resubscribed_ids() {
    let mut w = Writer::new(0);
    w.format("vec3:float x;float y;float z;")
        .format("thing:uint64_t timestamp;vec3 a;vec3[2] pair;char[4] tag;int16_t n;")
        .add(0, 1, "thing");
    let body = |base: f32| {
        let mut b = Body::default();
        for k in 0..9 {
            b = b.f32(base + k as f32);
        }
        let mut v = b.0;
        v.extend_from_slice(b"abcd");
        v.extend_from_slice(&(-3i16).to_le_bytes());
        v
    };
    w.data(1, 10, &body(0.)).remove(1).add(1, 1, "other");
    w.format("other:uint64_t timestamp;uint16_t k;");
    w.data(1, 20, &Body::default().u16(7).0);
    let log = parse(w.buf).unwrap();
    assert_eq!(log.series("thing", 0, "a.y").unwrap().v, vec![1.]);
    assert_eq!(log.series("thing", 0, "pair[1].z").unwrap().v, vec![8.]);
    assert_eq!(log.series("thing", 0, "n").unwrap().v, vec![-3.]);
    assert_eq!(log.series("other", 1, "k").unwrap().v, vec![7.]);
    let layout = log.topic("thing").next().unwrap().layout.clone().unwrap();
    assert_eq!(layout.field("tag").unwrap().len, 4);
    assert_eq!(layout.size, 8 + 36 + 4 + 2);
}

#[test]
fn self_referencing_formats_do_not_recurse_forever() {
    let mut w = Writer::new(0);
    w.format("loop:uint64_t timestamp;loop inner;")
        .add(0, 0, "loop");
    w.data(0, 1, &[]);
    let log = parse(w.buf).unwrap();
    assert_eq!(log.issues.unresolved, vec!["loop".to_string()]);
    assert!(log.series("loop", 0, "timestamp").is_none());
}

#[test]
fn plots_are_reduced_and_keep_extremes() {
    let t: Vec<f64> = (0..10_000).map(|k| k as f64).collect();
    let mut v = vec![0.; 10_000];
    v[5_123] = 99.;
    let r = plots::reduce(&t, &v);
    assert!(r.len() <= 480);
    assert!(r.iter().any(|p| p[1] == 99.));
    assert!(r.windows(2).all(|w| w[0][0] <= w[1][0]));
    let report = analyze_bytes(fixture::flight()).unwrap();
    let Report::Ulog { plots, .. } = report else {
        panic!("ulog report")
    };
    let ids: Vec<_> = plots.iter().map(|p| p.id).collect();
    assert_eq!(ids, vec!["altitude", "battery", "attitude", "vibration"]);
    let (lo, hi) = plots[0].range().unwrap();
    assert!(close(lo as f64, 0., 1e-3) && close(hi as f64, 20., 1e-3));
    let roll = plots[2].lines[0]
        .points
        .iter()
        .map(|p| p[1])
        .fold(0f32, f32::max);
    assert!(close(roll as f64, 0.2f64.to_degrees(), 0.01));
}

#[test]
fn tlog_summary_comes_from_the_mavlink_decoder_and_reports_damage() {
    use crate::mavlink::summarize_tlog_bytes as summarize;
    let bytes = fixture::tlog();
    let s = summarize(&bytes);
    assert_eq!((s.frames, s.v1, s.v2, s.heartbeats()), (185, 1, 184, 61));
    assert_eq!(s.message_counts.len(), 4);
    assert!(close(s.duration_s(), 60.6, 1e-9));
    assert_eq!(s.components.get(&(1, 1)), Some(&185));
    assert_eq!(s.vehicle, Some((1, "PX4".into(), "Quadrotor".into())));
    assert!(close(s.armed_seconds, 40., 1e-9));
    assert_eq!(s.max_relative_alt, Some(20.0));
    assert!(close(s.min_battery_voltage.unwrap() as f64, 15.8, 1e-4));
    assert_eq!(s.modes, vec!["Position".to_string()]);
    assert_eq!(
        s.status,
        vec![(6, "Takeoff detected".into()), (4, "Low battery".into())]
    );
    assert!(!s.damaged());

    let cut = summarize(&bytes[..bytes.len() - 3]);
    assert!(cut.truncated);
    assert_eq!((cut.frames, cut.skipped_bytes), (184, 0));

    let mut junk = vec![1, 2, 3];
    junk.extend_from_slice(&bytes);
    let j = summarize(&junk);
    assert_eq!((j.frames, j.skipped_bytes, j.truncated), (185, 3, false));

    // One payload byte of the second record flipped: its CRC fails and only it is lost.
    let first = 8 + 12 + bytes[9] as usize;
    let mut bad = bytes.clone();
    bad[first + 8 + 10] ^= 0x55;
    let b = summarize(&bad);
    assert_eq!((b.frames, b.bad_frames), (184, 1));
    assert!(b.skipped_bytes > 0 && b.damaged());
}

#[test]
fn px4_dirs_come_from_profiles_and_expand_under_each_home() {
    let dirs = sources::px4_dirs(&crate::backend::defaults().profiles);
    assert_eq!(dirs[0], sources::DEFAULT_PX4);
    let mut p = crate::backend::defaults().profiles[0].clone();
    p.program = "wsl".into();
    p.args = vec!["cd '/opt/src/PX4-Autopilot' && make px4_sitl gz_x500".into()];
    p.cwd = "~/work/PX4-Autopilot".into();
    let dirs = sources::px4_dirs(std::slice::from_ref(&p));
    assert!(dirs.contains(&"/opt/src/PX4-Autopilot".to_string()));
    assert!(dirs.contains(&"~/work/PX4-Autopilot".to_string()));
    // A non-ASCII separator and a versioned folder name, followed by a sub-folder.
    p.args = vec!["cd\u{a0}/x/PX4-Autopilot-1.15/build && make".into()];
    p.cwd = String::new();
    assert!(sources::px4_dirs(&[p]).contains(&"/x/PX4-Autopilot-1.15".to_string()));
    assert_eq!(
        sources::linux_to(
            std::path::Path::new(r"\\wsl.localhost\Ubuntu"),
            "/home/pilot/PX4-Autopilot"
        ),
        std::path::PathBuf::from(r"\\wsl.localhost\Ubuntu\home\pilot\PX4-Autopilot")
    );
}

#[test]
fn discovery_finds_sitl_vehicle_and_tlog_files_newest_first() {
    let root = std::env::temp_dir().join(format!("drone-deck-ulog-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let sitl = root.join("wsl/home/pilot/px4/build/px4_sitl_default/rootfs/log/2026-11-09");
    std::fs::create_dir_all(&sitl).unwrap();
    std::fs::write(sitl.join("10_42_17.ulg"), fixture::flight()).unwrap();
    let data = root.join("data");
    std::fs::create_dir_all(vehicle_dir(&data)).unwrap();
    std::fs::write(vehicle_dir(&data).join("log_3.ulg"), fixture::flight()).unwrap();
    std::fs::create_dir_all(data.join("hardware/capture")).unwrap();
    std::fs::write(data.join("hardware/capture/a.tlog"), fixture::tlog()).unwrap();
    std::fs::write(data.join("hardware/capture/notes.txt"), "x").unwrap();
    // Point the "distribution" at a local folder by searching its PX4 dir directly.
    let loc = Locations {
        distro: None,
        home: None,
        px4_dirs: vec![],
        data_root: data.clone(),
    };
    let d = sources::discover(&loc);
    assert_eq!(d.files.len(), 2);
    assert!(!d.wsl_reachable);
    assert!(d.files.iter().any(|f| f.source == Source::Vehicle));
    assert!(
        d.files
            .iter()
            .any(|f| f.source == Source::Telemetry && f.is_tlog())
    );
    let mut found = sources::Discovery::default();
    sources::sitl_logs(&root.join("wsl/home/pilot/px4"), &mut found);
    assert_eq!(found.files.len(), 1);
    assert_eq!(
        found.files[0].source,
        Source::Simulation("px4_sitl_default".into())
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn worker_analyzes_off_the_ui_thread() {
    let root = std::env::temp_dir().join(format!("drone-deck-ulog-w-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("flight.ulg");
    std::fs::write(&file, fixture::flight()).unwrap();
    let bad = root.join("bad.ulg");
    std::fs::write(&bad, b"not a log at all").unwrap();
    let w = worker::Worker::spawn();
    w.send(worker::Job::Analyze {
        path: bad.clone(),
        urgent: false,
    });
    w.send(worker::Job::Analyze {
        path: file.clone(),
        urgent: true,
    });
    let mut got = Vec::new();
    for _ in 0..2 {
        match w
            .rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .unwrap()
        {
            worker::Done::Analyzed(p, r) => got.push((p, r)),
            worker::Done::Scanned(_) => panic!("no scan requested"),
        }
    }
    let ok = got.iter().find(|(p, _)| *p == file).unwrap();
    assert!(close(ok.1.as_ref().unwrap().duration_s(), 100., 1e-9));
    let err = got.iter().find(|(p, _)| *p == bad).unwrap();
    assert!(err.1.as_ref().unwrap_err().contains("not a ULog"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_selected_log_jumps_the_background_queue_without_being_read_twice() {
    use std::path::PathBuf;
    let (a, b, c) = (PathBuf::from("a"), PathBuf::from("b"), PathBuf::from("c"));
    let bg = |p: &PathBuf| worker::Job::Analyze {
        path: p.clone(),
        urgent: false,
    };
    let order = worker::order(vec![
        bg(&a),
        bg(&b),
        bg(&c),
        worker::Job::Promote(c.clone()),
        // Already started or finished: nothing is queued again.
        worker::Job::Promote("gone".into()),
    ]);
    assert_eq!(order, vec![c.clone(), a.clone(), b.clone()]);
    // The UI promotes a log that is pending in the background instead of skipping it.
    let mut logs = FlightLogs::default();
    let f = LogFile {
        path: c.clone(),
        source: Source::Vehicle,
        size: 1,
        modified: None,
    };
    logs.files.push(f.clone());
    logs.insert(c.clone(), (1, None), Analysis::Pending);
    logs.request(&f, true);
    assert!(logs.worker.is_some(), "the promotion reached the worker");
}

#[test]
fn explorer_selects_plain_paths_and_opens_the_folder_for_spaces_or_commas() {
    let plain =
        std::path::Path::new(r"C:\Users\pilot\AppData\Local\DroneDeck\logs\vehicle\log_3.ulg");
    assert_eq!(
        explorer_target(plain),
        format!("/select,{}", plain.display())
    );
    let spaced = std::path::Path::new(r"C:\Users\Ann Lee\DroneDeck\logs\vehicle\log_3.ulg");
    assert_eq!(
        explorer_target(spaced),
        r"C:\Users\Ann Lee\DroneDeck\logs\vehicle"
    );
    let comma = std::path::Path::new(r"D:\flights\a,b.ulg");
    assert_eq!(explorer_target(comma), r"D:\flights");
}

#[test]
fn utc_dates_format_without_a_time_library() {
    assert_eq!(utc(0), "1970-01-01 00:00");
    assert_eq!(utc(951_782_400), "2000-02-29 00:00");
    assert_eq!(utc(1_793_000_000), "2026-10-26 07:33");
}

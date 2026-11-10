use super::*;
use crate::mavlink::fake_vehicle::scratch;

#[test]
fn utc_stamps() {
    assert_eq!(utc_stamp(0), "1970-01-01_00-00-00");
    assert_eq!(utc_stamp(1_700_000_000), "2023-11-14_22-13-20");
    assert_eq!(utc_stamp(951_782_400), "2000-02-29_00-00-00");
}

#[test]
fn names_files_per_stack() {
    let e = LogEntry {
        id: 4,
        size: 10,
        time_utc: 1_700_000_000,
    };
    assert_eq!(
        file_name(&e, Autopilot::Px4),
        "log_4_2023-11-14_22-13-20.ulg"
    );
    let e = LogEntry { time_utc: 0, ..e };
    assert_eq!(file_name(&e, Autopilot::ArduPilot), "log_4_10B.bin");
}

fn entry(id: u16, n: u16) -> dialect::LOG_ENTRY_DATA {
    dialect::LOG_ENTRY_DATA {
        time_utc: 0,
        size: 100,
        id,
        num_logs: n,
        last_log_num: n,
    }
}

#[test]
fn list_completes_or_retries() {
    let t0 = Instant::now();
    let (mut l, _) = LogList::new((1, 1), t0);
    l.on_entry(&entry(1, 2), t0);
    assert_eq!(l.poll(t0), ListStep::Wait);
    l.on_entry(&entry(2, 2), t0);
    assert!(
        matches!(l.poll(t0), ListStep::Done { ref entries, complete: true } if entries.len() == 2)
    );

    let (mut l, _) = LogList::new((1, 1), t0);
    l.on_entry(&entry(0, 0), t0);
    assert!(
        matches!(l.poll(t0), ListStep::Done { ref entries, complete: true } if entries.is_empty())
    );

    let (mut l, _) = LogList::new((1, 1), t0);
    l.on_entry(&entry(1, 3), t0);
    let mut t = t0;
    let mut asks = 0;
    let done = loop {
        t += l.idle;
        match l.poll(t) {
            ListStep::Send(_) => asks += 1,
            ListStep::Done { entries, complete } => break (entries.len(), complete),
            ListStep::Wait => panic!("deadline passed"),
        }
    };
    assert_eq!((asks, done), (2, (1, false)));
}

fn data(log: &[u8], ofs: u32) -> dialect::LOG_DATA_DATA {
    let end = (ofs as usize + CHUNK as usize).min(log.len());
    let mut d = [0u8; 90];
    let n = end.saturating_sub(ofs as usize);
    d[..n].copy_from_slice(&log[ofs as usize..end]);
    dialect::LOG_DATA_DATA {
        ofs,
        id: 9,
        count: n as u8,
        data: d,
    }
}

#[test]
fn download_reorders_resumes_and_finishes() {
    let dir = scratch("logs-unit");
    let log: Vec<u8> = (0..1000u32).map(|i| (i % 256) as u8).collect();
    let e = LogEntry {
        id: 9,
        size: log.len() as u32,
        time_utc: 0,
    };
    let t0 = Instant::now();
    let (mut d, step) =
        LogDownload::start(e.clone(), (1, 1), Autopilot::Px4, &dir, t0).expect("start");
    d.window = CHUNK * 4;
    assert!(matches!(step, DownloadStep::Send(Msg::LOG_REQUEST_DATA(r)) if r.ofs == 0));
    // Re-ask for a smaller window so the test sees window turnover.
    let DownloadStep::Send(_) = d.poll(t0 + d.idle) else {
        panic!("re-ask")
    };
    // Out of order: 180 arrives before 90.
    assert_eq!(d.on_data(&data(&log, 0), t0), None);
    assert_eq!(d.on_data(&data(&log, 180), t0), None);
    assert_eq!(d.received(), 90);
    assert_eq!(d.on_data(&data(&log, 90), t0), None);
    assert_eq!(d.received(), 270);
    let next = d.on_data(&data(&log, 270), t0);
    assert!(matches!(next, Some(DownloadStep::Send(Msg::LOG_REQUEST_DATA(r))) if r.ofs == 360));
    d.cancel();
    assert_eq!(std::fs::metadata(d.part_path()).expect("part").len(), 360);

    // A new download resumes from the .part length.
    let (mut d, step) =
        LogDownload::start(e.clone(), (1, 1), Autopilot::Px4, &dir, t0).expect("resume");
    assert!(matches!(step, DownloadStep::Send(Msg::LOG_REQUEST_DATA(r)) if r.ofs == 360));
    let mut done = None;
    let mut ofs = 360;
    while done.is_none() && ofs < 1000 {
        done = d.on_data(&data(&log, ofs), t0);
        ofs += CHUNK;
    }
    let Some(DownloadStep::Done(path)) = done else {
        panic!("not done: {done:?}")
    };
    assert_eq!(std::fs::read(&path).expect("file"), log);
    assert!(!d.part_path().exists());

    // The finished file short-circuits a third request.
    let (_, step) = LogDownload::start(e, (1, 1), Autopilot::Px4, &dir, t0).expect("again");
    assert_eq!(step, DownloadStep::Done(path));
}

#[test]
fn download_fails_after_repeated_silence_and_accepts_short_logs() {
    let dir = scratch("logs-stall");
    let e = LogEntry {
        id: 9,
        size: 500,
        time_utc: 0,
    };
    let t0 = Instant::now();
    let (mut d, _) =
        LogDownload::start(e.clone(), (1, 1), Autopilot::Px4, &dir, t0).expect("start");
    let mut t = t0;
    let reason = loop {
        t += d.idle;
        match d.poll(t) {
            DownloadStep::Send(_) => {}
            DownloadStep::Failed(reason) => break reason,
            other => panic!("{other:?}"),
        }
    };
    assert!(reason.contains("resume"));

    let log = vec![7u8; 100];
    let (mut d, _) = LogDownload::start(e, (1, 1), Autopilot::Px4, &dir, t0).expect("start");
    assert_eq!(d.on_data(&data(&log, 0), t0), None);
    let end = dialect::LOG_DATA_DATA {
        ofs: 100,
        id: 9,
        count: 0,
        data: [0; 90],
    };
    let mut d2 = d.on_data(&data(&log, 90), t0);
    if d2.is_none() {
        d2 = d.on_data(&end, t0);
    }
    let Some(DownloadStep::Done(path)) = d2 else {
        panic!("{d2:?}")
    };
    assert_eq!(std::fs::read(path).expect("file").len(), 100);
}

#[test]
fn malformed_offsets_are_ignored() {
    let dir = scratch("logs-malformed");
    let e = LogEntry {
        id: 9,
        size: 1000,
        time_utc: 0,
    };
    let t0 = Instant::now();
    let (mut d, _) = LogDownload::start(e, (1, 1), Autopilot::Px4, &dir, t0).expect("start");
    for ofs in [u32::MAX, u32::MAX - 10, u32::MAX - 89] {
        let bad = dialect::LOG_DATA_DATA {
            ofs,
            id: 9,
            count: 90,
            data: [1; 90],
        };
        assert_eq!(d.on_data(&bad, t0), None);
    }
    let over = dialect::LOG_DATA_DATA {
        ofs: 0,
        id: 9,
        count: 255,
        data: [2; 90],
    };
    assert_eq!(d.on_data(&over, t0), None);
    assert_eq!(d.received(), 90, "count is capped at the 90-byte payload");
}

#[test]
fn a_finished_file_of_another_size_is_downloaded_again() {
    let dir = scratch("logs-mismatch");
    let e = LogEntry {
        id: 1,
        size: 180,
        time_utc: 1_700_000_000,
    };
    let path = dir.join(file_name(&e, Autopilot::Px4));
    std::fs::write(&path, [9u8; 50]).expect("stale file");
    let t0 = Instant::now();
    let (mut d, step) = LogDownload::start(e, (1, 1), Autopilot::Px4, &dir, t0).expect("start");
    assert!(matches!(step, DownloadStep::Send(Msg::LOG_REQUEST_DATA(r)) if r.ofs == 0));
    let log: Vec<u8> = (0..180u32).map(|i| i as u8).collect();
    let data_at = |ofs: u32| {
        let mut chunk = [0u8; 90];
        chunk.copy_from_slice(&log[ofs as usize..ofs as usize + 90]);
        dialect::LOG_DATA_DATA {
            ofs,
            id: 1,
            count: 90,
            data: chunk,
        }
    };
    assert_eq!(d.on_data(&data_at(0), t0), None);
    let done = d.on_data(&data_at(90), t0);
    assert_eq!(done, Some(DownloadStep::Done(path.clone())));
    assert_eq!(std::fs::read(&path).expect("replaced"), log);
}

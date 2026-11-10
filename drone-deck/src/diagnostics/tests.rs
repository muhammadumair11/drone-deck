use super::redact::Redactor;
use super::*;
use std::sync::atomic::AtomicUsize;

/// A fresh empty folder under the system temp folder, removed on drop.
struct Scratch(PathBuf);
impl Scratch {
    fn new(tag: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "drone-deck-diag-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn redactor() -> Redactor {
    Redactor::new(
        &[
            (r"C:\Users\alice\AppData\Local".into(), "%LOCALAPPDATA%"),
            (r"C:\Users\alice".into(), "%USERPROFILE%"),
        ],
        &[
            ("alice".into(), "<user>"),
            ("ALICE-PC".into(), "<computer>"),
        ],
    )
}

#[test]
fn redaction_replaces_user_folders_in_every_spelling() {
    let r = redactor();
    assert_eq!(
        r.apply(r"cwd C:\Users\alice\AppData\Local\DroneDeck\tools"),
        r"cwd %LOCALAPPDATA%\DroneDeck\tools"
    );
    assert_eq!(r.apply("c:/users/ALICE/src/x.py"), "%USERPROFILE%/src/x.py");
    assert_eq!(
        r.apply(r#"{"cwd": "C:\\Users\\alice\\px4"}"#),
        r#"{"cwd": "%USERPROFILE%\\px4"}"#
    );
}

#[test]
fn redaction_hides_other_profiles_wsl_homes_names_and_tokens() {
    let r = redactor();
    assert_eq!(
        r.apply(r"D:\Users\bob\file and /mnt/c/Users/carol/x"),
        r"D:\Users\<user>\file and /mnt/c/Users/<user>/x"
    );
    assert_eq!(
        r.apply(r"\\wsl.localhost\Ubuntu-24.04\home\dave\PX4 and /home/erin/.local"),
        r"\\wsl.localhost\Ubuntu-24.04\home\<user>\PX4 and /home/<user>/.local"
    );
    assert_eq!(
        r.apply(r#"{"p":"C:\\Users\\frank\\x"}"#),
        r#"{"p":"C:\\Users\\<user>\\x"}"#
    );
    assert_eq!(
        r.apply(r"C:\Users\Public\Desktop"),
        r"C:\Users\Public\Desktop"
    );
    assert_eq!(
        r.apply("alice on ALICE-PC; malice and alice_x stay"),
        "<user> on <computer>; malice and alice_x stay"
    );
    assert_eq!(
        r.apply("token hf_abcdefghijklmnopqrstuvwxyz12 and hf_short"),
        "token hf_<redacted> and hf_short"
    );
    assert_eq!(r.apply("ünïcode /home/ünï ok"), "ünïcode /home/<user> ok");
}

#[test]
fn short_names_are_not_replaced_as_words() {
    let r = Redactor::new(&[], &[("al".into(), "<user>")]);
    assert_eq!(r.apply("al is here"), "al is here");
}

#[test]
fn crc32_matches_the_reference_value() {
    assert_eq!(zip::crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(zip::crc32(b""), 0);
}

#[test]
fn dos_time_packs_local_stamps() {
    let (date, time) = zip::dos_time("2026-11-09 13:45:30");
    assert_eq!(date, ((2026 - 1980) << 9) | (11 << 5) | 9);
    assert_eq!(time, (13 << 11) | (45 << 5) | 15);
    assert_eq!(zip::dos_time("garbage"), ((1 << 5) | 1, 0));
}

fn u16_at(b: &[u8], i: usize) -> usize {
    u16::from_le_bytes([b[i], b[i + 1]]) as usize
}
fn u32_at(b: &[u8], i: usize) -> usize {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize
}

#[test]
fn zip_archives_read_back_through_the_central_directory() {
    let entries = vec![
        ("about.txt".to_string(), b"hello".to_vec()),
        (
            "logs/app.log".to_string(),
            "ünïcode line\n".repeat(50).into_bytes(),
        ),
        ("empty.txt".to_string(), Vec::new()),
    ];
    let zip = zip::store(&entries, zip::dos_time("2026-11-09 13:45:30"));
    let end = zip.len() - 22;
    assert_eq!(u32_at(&zip, end), 0x0605_4b50);
    assert_eq!(u16_at(&zip, end + 10), entries.len());
    let mut at = u32_at(&zip, end + 16);
    for (name, data) in &entries {
        assert_eq!(u32_at(&zip, at), 0x0201_4b50);
        let crc = u32_at(&zip, at + 16);
        let size = u32_at(&zip, at + 24);
        let name_len = u16_at(&zip, at + 28);
        let local = u32_at(&zip, at + 42);
        assert_eq!(&zip[at + 46..at + 46 + name_len], name.as_bytes());
        assert_eq!(u32_at(&zip, local), 0x0403_4b50);
        let start = local + 30 + u16_at(&zip, local + 26);
        assert_eq!(&zip[start..start + size], data.as_slice());
        assert_eq!(crc as u32, zip::crc32(data));
        at += 46 + name_len;
    }
    assert_eq!(at, end);
}

/// The archive opens with .NET's reader, which is what Explorer-adjacent tooling uses.
#[cfg(windows)]
#[test]
fn zip_archives_open_with_the_windows_zip_reader() {
    let scratch = Scratch::new("zip");
    let path = scratch.0.join("bundle.zip");
    let entries = vec![
        ("about.txt".to_string(), b"Drone Deck".to_vec()),
        ("logs/app.log".to_string(), b"line one\nline two\n".to_vec()),
    ];
    fs::write(
        &path,
        zip::store(&entries, zip::dos_time("2026-11-09 13:45:30")),
    )
    .unwrap();
    let script = format!(
        "Add-Type -AssemblyName System.IO.Compression.FileSystem; \
         $z = [IO.Compression.ZipFile]::OpenRead('{}'); \
         foreach ($e in $z.Entries) {{ $r = New-Object IO.StreamReader($e.Open()); \
         Write-Output (\"$($e.FullName)=\" + $r.ReadToEnd().Length); $r.Close() }}; $z.Dispose()",
        path.display()
    );
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .expect("powershell runs");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("about.txt=10"), "{text}");
    assert!(text.contains("logs/app.log=18"), "{text}");
}

#[test]
fn the_app_log_rolls_over_and_keeps_a_bounded_history() {
    let scratch = Scratch::new("log");
    let log = scratch.0.join("logs").join("app.log");
    for i in 0..40 {
        append(&log, &format!("line {i:02} {}", "x".repeat(40)), 200, 2).unwrap();
    }
    let size = |p: &Path| fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    assert!(size(&log) > 0 && size(&log) < 200 + 60);
    assert!(rolled(&log, 1).is_file());
    assert!(rolled(&log, 2).is_file());
    assert!(!rolled(&log, 3).exists());
    let newest = fs::read_to_string(&log).unwrap();
    assert!(
        newest
            .trim_end()
            .ends_with(&format!("line 39 {}", "x".repeat(40)))
    );
    let older = fs::read_to_string(rolled(&log, 1)).unwrap();
    assert!(!older.contains("line 39"));
    assert_eq!(rolled(&log, 1).file_name().unwrap(), "app.1.log");
}

#[test]
fn rotation_without_kept_files_starts_over() {
    let scratch = Scratch::new("log0");
    let log = scratch.0.join("app.log");
    fs::write(&log, "x".repeat(300)).unwrap();
    assert!(rotate(&log, 200, 0).unwrap());
    assert!(!log.exists());
    assert!(!rotate(&log, 200, 0).unwrap());
}

#[test]
fn log_lines_are_capped() {
    let scratch = Scratch::new("cap");
    let log = scratch.0.join("app.log");
    append(&log, &"y".repeat(10_000), LOG_CAP, LOG_KEEP).unwrap();
    assert_eq!(fs::read_to_string(&log).unwrap().trim_end().len(), 4096);
}

#[test]
fn crash_reports_are_offered_once_newest_first_and_pruned() {
    let scratch = Scratch::new("crash");
    let dir = &scratch.0;
    assert!(take_unseen_crash(dir).is_none());
    let text = crash_text("boom", "src/main.rs:1:1", "main", "frames");
    assert!(text.contains("boom") && text.contains("Thread: main"));
    let first = write_crash(dir, &text).unwrap();
    let second = write_crash(dir, &text).unwrap();
    assert_ne!(first, second);
    assert_eq!(take_unseen_crash(dir), Some(second.clone()));
    assert!(take_unseen_crash(dir).is_none(), "offered only once");
    for _ in 0..12 {
        write_crash(dir, &text).unwrap();
    }
    let files = crash_files(dir);
    assert_eq!(files.len(), CRASH_KEEP);
    assert!(!first.exists(), "oldest reports are pruned");
    assert_eq!(take_unseen_crash(dir).as_ref(), files.last());
}

#[test]
fn logs_sit_beside_the_default_workspace_and_inside_overrides() {
    let local = PathBuf::from(r"C:\Users\a\AppData\Local");
    let default_root = local.join("DroneDeck").join("workspace");
    assert_eq!(
        logs_for(&default_root, Some(local.clone().into_os_string())),
        local.join("DroneDeck").join("logs")
    );
    let custom = PathBuf::from(r"D:\data");
    assert_eq!(
        logs_for(&custom, Some(local.into_os_string())),
        custom.join("logs")
    );
    assert_eq!(logs_for(&custom, None), custom.join("logs"));
}

#[test]
fn tails_start_on_a_whole_line() {
    assert_eq!(tail(b"short", 10), "short");
    let text = tail(b"first line\nsecond line\nthird", 14);
    assert_eq!(text, "[earlier lines omitted]\nthird");
}

#[test]
fn file_tails_read_only_the_end_and_match_whole_file_tails() {
    let dir = Scratch::new("tail");
    let path = dir.0.join("output.log");
    let mut text = String::new();
    for i in 0..2000 {
        text.push_str(&format!(
            "line {i}
"
        ));
    }
    fs::write(&path, &text).unwrap();
    // Cuts inside a line and right after a newline both match the whole-file tail.
    for max in [
        100,
        101,
        102,
        103,
        104,
        text.len() - 1,
        text.len(),
        text.len() + 5,
    ] {
        assert_eq!(
            read_tail_of(&path, max).unwrap(),
            tail(text.as_bytes(), max),
            "max {max}"
        );
    }
    assert!(read_tail_of(&path, 100).unwrap().starts_with(
        "[earlier lines omitted]
line 19"
    ));
    assert_eq!(read_tail_of(&dir.0.join("missing.log"), 100), None);
}

#[test]
fn bundles_carry_state_and_redacted_profiles() {
    let mut s = State::new();
    s.config.profiles[0].cwd = r"C:\Users\zed\PX4".into();
    s.status.insert("sih".into(), "Running".into());
    s.logs.push_back("[sih] ready".into());
    let files = entries(&s);
    let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    for name in [
        "about.txt",
        "state.txt",
        "profiles.json",
        "activity.json",
        "process-output.log",
    ] {
        assert!(names.contains(&name), "{name} missing from {names:?}");
    }
    let text = |name: &str| &files.iter().find(|(n, _)| n == name).unwrap().1;
    assert!(text("state.txt").contains("sih: Running"));
    assert!(text("about.txt").contains(VERSION));
    let redacted = Redactor::from_env().apply(text("profiles.json"));
    assert!(!redacted.contains(r"Users\\zed"), "{redacted}");
    assert!(redacted.contains(r"C:\\Users\\<user>\\PX4"));
}

mod page {
    use super::*;
    use crate::ui::{Page, view};
    use rune::{Env, Runtime};

    #[test]
    fn activity_offers_the_crash_report_and_diagnostics_export() {
        set_pending_crash(Some(PathBuf::from(r"C:\logs\crash-20261109-101010.txt")));
        let mut s = State::new();
        s.navigate(Page::Activity);
        let mut rt = Runtime::new(
            s,
            view,
            (1280., 800.),
            Env {
                system_dark: true,
                scale: 1.,
            },
        );
        for _ in 0..5 {
            rt.frame(0.1);
        }
        assert!(rt.find("export-diagnostics").is_some());
        assert!(rt.find("crash-notice").is_some());
        let (x, y) = rt.find("crash-dismiss").expect("dismiss action");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        for _ in 0..5 {
            rt.frame(0.1);
        }
        assert!(pending_crash().is_none());
        assert!(rt.find("crash-notice").is_none());
    }
}

#[test]
fn a_log_that_cannot_roll_keeps_writing_up_to_twice_its_cap() {
    let scratch = Scratch::new("stuck");
    let log = scratch.0.join("app.log");
    // A folder where the rolled file should go makes every rename fail.
    fs::create_dir_all(rolled(&log, 1)).unwrap();
    let mut refused = 0;
    for i in 0..60 {
        if append(&log, &format!("line {i:02} {}", "z".repeat(20)), 100, 1).is_err() {
            refused += 1;
        }
    }
    let size = fs::metadata(&log).unwrap().len();
    assert!((200..200 + 40).contains(&size), "size {size}");
    assert!(refused > 0, "lines past twice the cap are dropped");
}

#[test]
fn redaction_cuts_secret_values_by_key() {
    let r = Redactor::new(&[], &[]);
    assert_eq!(
        r.apply("HF_TOKEN=abc123 make px4"),
        "HF_TOKEN=<redacted> make px4"
    );
    assert_eq!(
        r.apply(r#"{"password": "p\"w d", "user": "x"}"#),
        r#"{"password": "<redacted>", "user": "x"}"#
    );
    assert_eq!(
        r.apply("--api-key=k1 --api_secret: s2; api=public"),
        "--api-key=<redacted> --api_secret: <redacted>; api=public"
    );
    assert_eq!(r.apply("token"), "token");
    assert_eq!(r.apply("secret:"), "secret:");
    assert_eq!(
        r.apply("tokens counted, no value"),
        "tokens counted, no value"
    );
    assert_eq!(r.apply("passwd='ü x' tail"), "passwd='<redacted>' tail");
}

/// PERF-01: the state is copied on the UI thread; the bundle is read, zipped and written on
/// another thread.
#[test]
fn a_snapshot_writes_its_bundle_from_another_thread() {
    let mut s = State::new();
    s.logs.push_back("[sih] snapshot line".into());
    let snap = snapshot(&s);
    let path = std::thread::spawn(move || write_bundle(snap))
        .join()
        .unwrap()
        .expect("bundle written");
    let bytes = fs::read(&path).unwrap();
    fs::remove_file(&path).ok();
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert!(
        bytes
            .windows(b"process-output.log".len())
            .any(|w| w == b"process-output.log")
    );
}

/// PERF-01: an export result lands through `poll`, which reports a change only then, and
/// failures say what went wrong.
#[test]
fn export_results_land_through_poll_only_when_they_arrive() {
    let mut s = State::new();
    start_export(&mut s);
    assert!(s.notice.contains("Headless"), "{}", s.notice);
    assert!(s.activity.exporting.is_none());
    assert!(!poll(&mut s), "idle");

    let (tx, rx) = mpsc::channel();
    s.activity.exporting = Some(exporting(rx));
    assert!(!poll(&mut s), "nothing landed yet");
    tx.send(Err("disk full".into())).unwrap();
    assert!(poll(&mut s));
    assert!(s.activity.exporting.is_none());
    assert_eq!(s.notice, "Could not export diagnostics: disk full");

    let (tx, rx) = mpsc::channel();
    s.activity.exporting = Some(exporting(rx));
    tx.send(Ok(PathBuf::from("exports/drone-deck-diagnostics-1.zip")))
        .unwrap();
    assert!(poll(&mut s));
    assert!(s.notice.starts_with("Diagnostics saved to"), "{}", s.notice);
    assert_eq!(s.activity.entries[0].text, "Exported diagnostics");

    let (tx, rx) = mpsc::channel::<Result<PathBuf, String>>();
    s.activity.exporting = Some(exporting(rx));
    drop(tx);
    assert!(poll(&mut s), "a worker that died still ends the export");
    assert!(
        s.notice.contains("stopped before it finished"),
        "{}",
        s.notice
    );
}

#[test]
fn the_export_button_is_disabled_while_an_export_runs() {
    use crate::ui::{Page, view};
    use rune::{Env, Runtime};
    let mut s = State::new();
    s.navigate(Page::Activity);
    let (tx, rx) = mpsc::channel();
    s.activity.exporting = Some(exporting(rx));
    let mut rt = Runtime::new(
        s,
        view,
        (1280., 800.),
        Env {
            system_dark: true,
            scale: 1.,
        },
    );
    for _ in 0..5 {
        rt.frame(0.1);
    }
    let button = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("export-diagnostics"))
        .expect("export button");
    assert_eq!(button.label.as_deref(), Some("Exporting…"));
    rt.state.notice.clear();
    let (x, y) = rt.find("export-diagnostics").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.1);
    assert!(rt.state.notice.is_empty(), "a disabled export does nothing");
    drop(tx);
}

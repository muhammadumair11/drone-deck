use super::super::foxglove::{self, Opened};
use super::*;
use crate::agent::fake::{self, eventually};
use rune::{Env, Runtime};
use std::path::{Path, PathBuf};

fn recording() -> PipeRecording {
    PipeRecording {
        id: "20261110T000000Z_flight_12345678".into(),
        label: "flight".into(),
        path: "/home/pilot/.local/share/drone-deck/recordings/20261110T000000Z_flight_12345678"
            .into(),
        domain: 0,
        topics: vec![
            PipeTopic {
                name: "/camera/image_raw".into(),
                kind: IMAGE.into(),
            },
            PipeTopic {
                name: "/camera/compressed".into(),
                kind: COMPRESSED.into(),
            },
            PipeTopic {
                name: "/status".into(),
                kind: "std_msgs/msg/String".into(),
            },
        ],
        mcap: vec!["/x/bag/bag_0.mcap".into()],
    }
}

/// `pipe.infer` parameters for the `Ubuntu` distro with the default drive mount.
fn infer(p: &Pipes) -> Result<Value, String> {
    infer_params(p, "Ubuntu", None)
}

fn state_with_recording() -> State {
    let mut s = State::new();
    s.studio.pipes.recordings = vec![recording()];
    select(&mut s, "20261110T000000Z_flight_12345678");
    s
}

#[test]
fn selecting_a_recording_fills_its_image_topics() {
    let mut s = state_with_recording();
    let p = &s.studio.pipes;
    assert_eq!(p.topics.text, "/camera/image_raw /camera/compressed");
    assert_eq!(p.image_topic.text, "/camera/image_raw");
    toggle_topic(&mut s, "/status");
    toggle_topic(&mut s, "/camera/compressed");
    assert_eq!(s.studio.pipes.topics.text, "/camera/image_raw /status");
    assert_eq!(topic_list("/a, /b  /a"), ["/a", "/b"]);
}

#[test]
fn extract_parameters_are_checked_before_anything_is_sent() {
    let mut s = State::new();
    assert_eq!(
        extract_params(&s.studio.pipes).unwrap_err(),
        "Select a recording first."
    );
    s = state_with_recording();
    let value = extract_params(&s.studio.pipes).unwrap();
    assert_eq!(
        value,
        json!({"recording":"20261110T000000Z_flight_12345678",
            "topics":["/camera/image_raw","/camera/compressed"],"format":"png",
            "start":null,"end":null,"rate":null})
    );
    let p = &mut s.studio.pipes;
    p.format = 1;
    p.start = TextEdit::new("1.5");
    p.end = TextEdit::new("4");
    p.rate = TextEdit::new("2");
    p.name = TextEdit::new("frames-2hz");
    let value = extract_params(p).unwrap();
    assert_eq!(
        (
            &value["format"],
            &value["start"],
            &value["rate"],
            &value["name"]
        ),
        (
            &json!("npz"),
            &json!(1.5),
            &json!(2.0),
            &json!("frames-2hz")
        )
    );
    for (edit, expected) in [
        ("end", "End must be after start."),
        (
            "topics",
            "Use absolute topic names: camera should start with /.",
        ),
        (
            "name",
            "Name: use 1–64 letters, numbers, hyphens or underscores.",
        ),
        ("rate", "Rate must be a number of zero or more."),
    ] {
        let mut s = state_with_recording();
        let p = &mut s.studio.pipes;
        match edit {
            "end" => {
                p.start = TextEdit::new("5");
                p.end = TextEdit::new("2");
            }
            "topics" => p.topics = TextEdit::new("camera"),
            "name" => p.name = TextEdit::new("../up"),
            _ => p.rate = TextEdit::new("fast"),
        }
        assert_eq!(extract_params(p).unwrap_err(), expected);
    }
    let mut s = state_with_recording();
    s.studio.pipes.topics = TextEdit::new("  ");
    assert!(extract_params(&s.studio.pipes).is_err());
}

#[test]
fn infer_parameters_refuse_the_capture_domain_and_non_image_topics() {
    let mut s = state_with_recording();
    let p = &mut s.studio.pipes;
    assert_eq!(
        infer(p).unwrap_err(),
        "Enter the path of an .onnx model file."
    );
    p.model = TextEdit::new("C:\\Models\\detector.ONNX");
    let value = infer(p).unwrap();
    assert_eq!(
        value,
        json!({"recording":"20261110T000000Z_flight_12345678","model":"/mnt/c/Models/detector.ONNX",
            "topic":"/camera/image_raw","domain":78,"rate":1.0})
    );
    p.project = TextEdit::new("vision");
    assert_eq!(infer(p).unwrap()["project"], "vision");
    p.domain = TextEdit::new("0");
    assert!(infer(p).unwrap_err().contains("captured on"));
    p.domain = TextEdit::new("300");
    assert_eq!(infer(p).unwrap_err(), "Replay domain must be 0–232.");
    p.domain = TextEdit::new("78");
    p.speed = TextEdit::new("8");
    assert_eq!(infer(p).unwrap_err(), "Replay speed must be 0.1–4.0.");
    p.speed = TextEdit::new("1");
    p.image_topic = TextEdit::new("/status");
    assert!(infer(p).unwrap_err().contains("needs an Image"));
    p.image_topic = TextEdit::new("/missing");
    assert_eq!(
        infer(p).unwrap_err(),
        "Choose an image topic of this recording."
    );
}

#[test]
fn headless_start_sends_nothing_and_says_why() {
    let mut s = state_with_recording();
    s.headless = true;
    s.studio.pipes.model = TextEdit::new("/m.onnx");
    start(&mut s, JobKind::Infer);
    assert!(s.studio.pipes.job.is_none());
    assert!(s.studio.pipes.message.contains("Headless"));
    refresh(&mut s);
    assert!(!s.studio.pipes.listing);
}

#[test]
fn a_job_reports_progress_then_its_result_and_refreshes_the_list() {
    let (connector, seen) =
        fake::connector(Arc::new(|request, out| match request["method"].as_str() {
            Some("pipe.extract") => {
                out.send(
                    json!({"event":"progress","id":request["id"],"message":"Extracted 4 of 8"}),
                );
                out.reply(
                    request,
                    json!({"status":"Completed","count":4,"messages_read":8,"path":"/r/extract/a"}),
                );
            }
            Some("pipe.list") => out.reply(
                request,
                json!({"recordings":[{"id":"r1","label":"one","path":"/r1","domain":0,
                    "topics":[{"name":"/cam","type":IMAGE}],"mcap":["/r1/bag/a.mcap"]}],
                    "runs":[{"kind":"extract","recording":"r1","name":"a","path":"/r1/extract/a",
                    "status":"Completed","count":4,"format":"png","topics":["/cam"]}]}),
            ),
            _ => out.error(request, "unexpected"),
        }));
    let distro = "Fake-Pipelines";
    crate::agent::install(distro, connector);
    let (tx, rx) = mpsc::channel();
    let params = json!({"recording":"r1","topics":["/cam"],"format":"png"});
    run_job(
        JobKind::Extract,
        params.clone(),
        distro.into(),
        Arc::new(AtomicBool::new(false)),
        tx,
    );
    let messages: Vec<Msg> = rx.try_iter().collect();
    assert!(matches!(&messages[0], Msg::Progress(t) if t == "Extracted 4 of 8"));
    let Msg::Done(JobKind::Extract, Ok(result)) = &messages[1] else {
        panic!("expected a result");
    };
    assert_eq!(
        summary(JobKind::Extract, result),
        "Extraction completed: 4 file(s) from 8 message(s) read into /r/extract/a."
    );
    let sent = seen
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|r| r["method"] == "pipe.extract")
        .cloned()
        .unwrap();
    assert_eq!(sent["params"], params);

    // The reply lands through poll and triggers a list refresh, which needs a ready WSL.
    let mut s = state_with_recording();
    s.config.distro = distro.into();
    s.studio
        .pipes
        .tx
        .send(Msg::Listed(Ok(json!({"recordings":[],"runs":[]}))))
        .unwrap();
    assert!(poll(&mut s));
    assert!(s.studio.pipes.listed);
    assert!(
        s.studio.pipes.selected.is_none(),
        "a vanished recording is unselected"
    );
    let reply = crate::agent::shared(distro)
        .unwrap()
        .call("pipe.list", json!({}), Duration::from_secs(5))
        .unwrap();
    s.studio.pipes.tx.send(Msg::Listed(Ok(reply))).unwrap();
    poll(&mut s);
    assert_eq!(s.studio.pipes.recordings[0].topics[0].name, "/cam");
    assert_eq!(s.studio.pipes.runs[0].count, Some(4));
}

#[test]
fn cancel_tells_the_agent_and_ends_once_its_processes_stopped() {
    // The agent is still stopping the run on the first status call, done on the second.
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = asked.clone();
    let (connector, seen) = fake::connector(Arc::new(move |request, out| {
        if request["method"] == "pipe.status" {
            let active = if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                json!(["/r/inference/x"])
            } else {
                json!([])
            };
            out.reply(request, json!({ "active": active }));
        }
    }));
    let distro = "Fake-Pipelines-Cancel";
    crate::agent::install(distro, connector);
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let worker =
        std::thread::spawn(move || run_job(JobKind::Infer, json!({}), distro.into(), flag, tx));
    eventually(|| {
        seen.methods()
            .contains(&"pipe.infer".to_string())
            .then_some(())
    });
    cancel.store(true, Ordering::SeqCst);
    worker.join().unwrap();
    let messages: Vec<Msg> = rx.try_iter().collect();
    assert!(
        matches!(messages.first(), Some(Msg::Progress(t)) if t.starts_with("Cancelling")),
        "the job says it is stopping before it ends"
    );
    assert!(matches!(messages.last(), Some(Msg::Done(JobKind::Infer, Err(e))) if e == CANCELLED));
    assert_eq!(asked.load(Ordering::SeqCst), 2, "waited for the agent");
    eventually(|| seen.methods().contains(&"cancel".to_string()).then_some(()));

    let mut s = State::new();
    let flag = Arc::new(AtomicBool::new(false));
    s.studio.pipes.job = Some(Job {
        kind: JobKind::Infer,
        label: "flight".into(),
        progress: String::new(),
        started: Instant::now(),
        cancel: flag.clone(),
        shown_secs: 0,
    });
    super::cancel(&mut s);
    assert!(flag.load(Ordering::SeqCst));
    assert!(s.studio.pipes.job.as_ref().unwrap().cancelling());
    s.studio
        .pipes
        .tx
        .send(Msg::Done(JobKind::Infer, Err(CANCELLED.into())))
        .unwrap();
    poll(&mut s);
    assert!(s.studio.pipes.job.is_none());
    assert!(s.studio.pipes.message.contains("partial output is kept"));
}

#[test]
fn one_malformed_row_does_not_hide_the_other_recordings() {
    let mut s = State::new();
    s.studio
        .pipes
        .tx
        .send(Msg::Listed(Ok(json!({
            "recordings": [
                {"id":"good","label":"g","path":"/g","domain":0,"topics":[],"mcap":[]},
                {"id":"bad","domain":"three"}
            ],
            "runs": [
                {"kind":"extract","name":"ok","status":"Completed","count":3},
                {"kind":"extract","name":"bad","count":-1}
            ]
        }))))
        .unwrap();
    assert!(poll(&mut s));
    let ids: Vec<&str> = s
        .studio
        .pipes
        .recordings
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(ids, ["good"]);
    let runs: Vec<&str> = s
        .studio
        .pipes
        .runs
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(runs, ["ok"]);
}

#[test]
fn wsl_paths_map_to_both_shares_and_export_names_follow_the_agent() {
    let linux = "/home/pilot/rec/20261110_flight/bag/bag_0.mcap";
    assert_eq!(
        foxglove::unc_candidates("Ubuntu-24.04", linux),
        [
            PathBuf::from(
                r"\\wsl.localhost\Ubuntu-24.04\home\pilot\rec\20261110_flight\bag\bag_0.mcap"
            ),
            PathBuf::from(r"\\wsl$\Ubuntu-24.04\home\pilot\rec\20261110_flight\bag\bag_0.mcap"),
        ]
    );
    assert_eq!(foxglove::export_name(linux), "20261110_flight-bag_0.mcap");
    assert_eq!(foxglove::export_name("x.mcap"), "x.mcap");
}

#[test]
fn model_paths_from_explorer_map_into_the_configured_distro() {
    let mut s = state_with_recording();
    let p = &mut s.studio.pipes;
    for (typed, linux) in [
        (
            r"\\wsl.localhost\Ubuntu\home\me\models\yolo.onnx",
            "/home/me/models/yolo.onnx",
        ),
        (r"\\WSL$\ubuntu\home\me\yolo.onnx", "/home/me/yolo.onnx"),
        ("//wsl.localhost/Ubuntu/opt/m.onnx", "/opt/m.onnx"),
        ("/home/me/m.onnx", "/home/me/m.onnx"),
    ] {
        p.model = TextEdit::new(typed);
        assert_eq!(infer(p).unwrap()["model"], linux, "{typed}");
    }
    p.model = TextEdit::new(r"\\wsl.localhost\Debian\home\me\yolo.onnx");
    let refused = infer(p).unwrap_err();
    assert!(
        refused.contains("Debian") && refused.contains("Ubuntu"),
        "{refused}"
    );
    // A distro with `[automount] root = /` sees drive C at /c.
    p.model = TextEdit::new(r"C:\Models\m.onnx");
    assert_eq!(
        infer_params(p, "Ubuntu", Some("/")).unwrap()["model"],
        "/c/Models/m.onnx"
    );
}

fn temp(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("drone-deck-foxglove-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn open_in_place_then_copy_then_agent_export() {
    let dir = temp("open");
    let share = dir.join("share.mcap");
    std::fs::write(&share, b"mcap bytes").unwrap();
    let exports = dir.join("exports");
    let linux = "/rec/flight/bag/bag_0.mcap";
    let opened = std::cell::RefCell::new(Vec::<PathBuf>::new());
    let ok = |p: &Path| {
        opened.borrow_mut().push(p.to_path_buf());
        Ok(())
    };
    let no_export = |_: &str, _: &Path| -> Result<PathBuf, String> { panic!("not needed") };
    let missing = dir.join("missing.mcap");
    assert_eq!(
        foxglove::open_or_copy(
            &[missing.clone(), share.clone()],
            linux,
            &exports,
            &ok,
            &no_export
        ),
        Ok(Opened::InPlace(share.clone()))
    );
    // The default app refuses the share path: the copy in exports is opened instead.
    let refuse_share = |p: &Path| {
        if p == share {
            Err("access denied".to_string())
        } else {
            opened.borrow_mut().push(p.to_path_buf());
            Ok(())
        }
    };
    let copy = exports.join("flight-bag_0.mcap");
    assert_eq!(
        foxglove::open_or_copy(&[share.clone()], linux, &exports, &refuse_share, &no_export),
        Ok(Opened::Copy(copy.clone()))
    );
    assert_eq!(std::fs::read(&copy).unwrap(), b"mcap bytes");
    // The share is not reachable at all: the agent copies the file into exports.
    let agent_copy = |path: &str, dest: &Path| {
        assert_eq!(path, linux);
        let target = dest.join("from-agent.mcap");
        std::fs::write(&target, b"agent").map_err(|e| e.to_string())?;
        Ok(target)
    };
    assert_eq!(
        foxglove::open_or_copy(&[missing.clone()], linux, &exports, &ok, &agent_copy),
        Ok(Opened::Copy(exports.join("from-agent.mcap")))
    );
    let never = |_: &Path| Err("no app is associated".to_string());
    let error =
        foxglove::open_or_copy(&[missing], linux, &exports, &never, &agent_copy).unwrap_err();
    assert!(error.contains("Install Foxglove"), "{error}");
    assert!(error.contains("not reachable"), "{error}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn live_foxglove_opens_once_the_bridge_runs_and_gives_up_when_it_stops() {
    let mut s = State::new();
    s.studio.pipes.live_pending = Some(Instant::now() - Duration::from_secs(5));
    s.status.insert(foxglove::PROFILE.into(), "Starting".into());
    assert!(!foxglove::poll(&mut s));
    assert!(s.studio.pipes.live_pending.is_some());
    s.status.insert(foxglove::PROFILE.into(), "Running".into());
    assert!(foxglove::poll(&mut s));
    assert!(s.studio.pipes.live_pending.is_none());
    assert!(s.studio.pipes.message.contains("ws://localhost:8765"));
    s.studio.pipes.live_pending = Some(Instant::now());
    s.status.insert(foxglove::PROFILE.into(), "Exited 1".into());
    assert!(foxglove::poll(&mut s));
    assert!(s.studio.pipes.message.contains("did not start (Exited 1)"));
    s.headless = true;
    foxglove::toggle_live(&mut s);
    assert!(s.studio.pipes.live_pending.is_none());
}

fn runtime() -> Runtime<State> {
    let mut s = State::new();
    preview(&mut s);
    Runtime::new(
        s,
        crate::view,
        (1216., 800.),
        Env {
            scale: 1.,
            system_dark: false,
        },
    )
}

#[test]
fn pipelines_tab_shows_forms_runs_and_foxglove_actions() {
    let mut rt = runtime();
    rt.frame(0.1);
    for key in [
        "data-tab-2",
        "pipelines-tab",
        "pipe-status",
        "pipe-recording-20261110T091500Z_camera_4f1a2b3c",
        "pipe-topics",
        "pipe-topic-0",
        "pipe-format-1",
        "extract-frames",
        "pipe-model",
        "pipe-image-topic-0",
        "pipe-domain",
        "infer-replay",
        "pipe-run-0",
        "open-run-0",
        "foxglove-run-0",
        "foxglove-live",
        "refresh-pipelines",
    ] {
        assert!(rt.find(key).is_some(), "{key} missing");
    }
    // The extraction run has no MCAP output, so it has no Foxglove action.
    assert!(rt.find("foxglove-run-1").is_none());
    assert!(rt.find("evaluation-csv").is_none());
    assert!(rt.find("cancel-pipeline").is_none());
}

#[test]
fn pipelines_tab_fits_compact_and_wide_windows() {
    let mut rt = runtime();
    for (w, h) in [(360., 800.), (640., 360.), (1440., 900.)] {
        rt.set_viewport((w, h), 1.);
        for _ in 0..6 {
            rt.frame(0.1);
        }
        for key in ["app-root", "main-scroll", "pipelines-tab"] {
            let (r, _) = rt.presented(key).unwrap();
            assert!(
                r[0] >= -1. && r[0] + r[2] <= w + 1.,
                "{key} at {w}x{h}: {r:?}"
            );
        }
    }
}

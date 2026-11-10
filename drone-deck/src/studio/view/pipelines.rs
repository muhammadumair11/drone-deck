//! Pipelines: extract frames from a finalized recording, run an ONNX model on its replay, and
//! list the runs, with "Open in Foxglove" for outputs and "Live in Foxglove" for the ROS graph.
use super::super::{
    foxglove,
    pipelines::{self, FORMATS, JobKind, PipeRecording, Run},
};
use super::*;

/// The status line of the tab: the job's progress while one runs.
pub(super) fn status(s: &State) -> El {
    let ps = &s.studio.pipes;
    let (lead, text) = match &ps.job {
        Some(job) => (
            "Working",
            format!(
                "{} / {} / {}s: {}",
                job.kind.name(),
                job.label,
                job.started.elapsed().as_secs(),
                job.progress
            ),
        ),
        None => ("Status", ps.message.clone()),
    };
    row()
        .key("pipe-status")
        .role("status")
        .label(text.clone())
        .w_full()
        .min_w(0.)
        .gap(14.)
        .py(10.)
        .items_start()
        .child(
            meta_sized(lead, 9., if ps.job.is_some() { fg() } else { muted() })
                .w(64.)
                .pt(4.),
        )
        .child(body(text, 11., fg()).grow(1.).wrap_anywhere())
        .child(if let Some(job) = &ps.job {
            ghost(
                "cancel-pipeline",
                if job.cancelling() {
                    "Cancelling…"
                } else {
                    "Cancel"
                },
                !job.cancelling(),
                pipelines::cancel,
            )
        } else {
            el()
        })
}

fn recording_row(s: &State, r: &PipeRecording) -> El {
    let selected = s.studio.pipes.selected.as_deref() == Some(r.id.as_str());
    let images = r.topics.iter().filter(|t| t.image()).count();
    let detail = format!(
        "{} topics / {images} image / {} MCAP file(s)",
        r.topics.len(),
        r.mcap.len()
    );
    let entry = ruled(false)
        .key(format!("pipe-recording-{}", r.id))
        .gap(12.)
        .py(10.)
        .px(4.)
        .items_start()
        .child(
            col()
                .grow(1.)
                .min_w(0.)
                .gap(4.)
                .child(h3(r.label.clone(), 13.))
                .child(code(r.id.clone(), 11., muted()).wrap_anywhere())
                .child(p(detail, 11.)),
        )
        .child(pick_label(selected));
    let id = r.id.clone();
    choose(entry, &r.label, selected, move |s| {
        pipelines::select(s, &id)
    })
}

fn recordings(s: &State) -> El {
    let ps = &s.studio.pipes;
    let mut list = col().key("pipe-recordings").w_full().min_w(0.);
    for r in &ps.recordings {
        list = list.child(recording_row(s, r));
    }
    if ps.recordings.is_empty() {
        let (title, text) = if !s.wsl_ready() {
            (
                "Linux environment not ready",
                "Recordings and pipeline outputs live inside the selected Ubuntu. Select a working WSL 2 distribution in Settings first.",
            )
        } else if ps.listing {
            (
                "Loading recordings",
                "Reading finalized recordings and run manifests in the selected Ubuntu.",
            )
        } else if ps.listed {
            (
                "No finalized recordings",
                "Capture and stop a recording under Recordings; pipelines read finalized MCAP files only.",
            )
        } else {
            (
                "Recordings not loaded",
                "Reload to list finalized recordings in the selected Ubuntu.",
            )
        };
        list = list.child(empty("pipe-recordings-empty", title, text).py(16.));
    }
    list
}

fn live_panel(s: &State) -> El {
    let ps = &s.studio.pipes;
    let configured = s.config.profiles.iter().any(|x| x.id == foxglove::PROFILE);
    let state = s.status.get(foxglove::PROFILE).cloned().unwrap_or_else(|| {
        if configured {
            "Not running".into()
        } else {
            "Not configured".into()
        }
    });
    let tone = match state.as_str() {
        "Running" => success(),
        "Starting" => fg(),
        s if s.starts_with("Exited") || s == "Failed" => danger(),
        _ => muted(),
    };
    let live = foxglove::live(s);
    col()
        .key("foxglove-panel")
        .w_full()
        .min_w(0.)
        .gap(10.)
        .child(meta("Live in Foxglove"))
        .child(
            row()
                .gap(10.)
                .center_y()
                .child(dot(tone))
                .child(meta_sized(format!("foxglove_bridge / {state}"), 10., fg()).wrap()),
        )
        .child(p(
            "Starts the owned foxglove_bridge (ROS 2, port 8765) and opens Foxglove on ws://localhost:8765 once it runs. It shows the live ROS graph of the capture domain, not the replay domain.",
            11.,
        ))
        .child(
            row()
                .gaps(4., 20.)
                .flex_wrap()
                .child(ghost(
                    "foxglove-live",
                    if live {
                        "Stop Foxglove bridge"
                    } else {
                        "Live in Foxglove ↗"
                    },
                    (configured && s.wsl_ready()) || live,
                    foxglove::toggle_live,
                ))
                .child(ghost(
                    "refresh-pipelines",
                    "Reload recordings",
                    s.wsl_ready() && !ps.listing,
                    pipelines::refresh,
                )),
        )
        .child(if configured {
            el()
        } else {
            notice(
                "Not configured yet: install the Foxglove bridge (ros-jazzy-foxglove-bridge) from Tools & config.",
            )
        })
}

fn topic_toggles(s: &State) -> El {
    let ps = &s.studio.pipes;
    let chosen = pipelines::topic_list(&ps.topics.text);
    let mut list = col().key("pipe-topic-list").w_full().min_w(0.);
    if let Some(r) = ps.recording() {
        for (i, t) in r.topics.iter().enumerate() {
            let on = chosen.iter().any(|c| c == &t.name);
            let name = t.name.clone();
            list = list.child(
                row()
                    .w_full()
                    .min_w(0.)
                    .gap(12.)
                    .center_y()
                    .child(
                        checkbox(&format!("pipe-topic-{i}"), &t.name, on, move |s| {
                            pipelines::toggle_topic(s, &name)
                        })
                        .grow(1.)
                        .min_w(0.),
                    )
                    .child(
                        code(t.kind.clone(), 10., muted())
                            .wrap_anywhere()
                            .max_w(260.),
                    ),
            );
        }
    }
    list
}

fn extract_form(s: &State, vp: &Viewport) -> El {
    let ps = &s.studio.pipes;
    let can = s.wsl_ready() && ps.job.is_none() && ps.selected.is_some();
    let numbers = if vp.compact() { col() } else { row() }
        .w_full()
        .min_w(0.)
        .gap(16.)
        .child(
            labeled(
                "Start / s from first message",
                field(&ps.start, "pipe-start", "0")
                    .on_edit(|s: &mut State| &mut s.studio.pipes.start),
            )
            .grow(1.),
        )
        .child(
            labeled(
                "End / s",
                field(&ps.end, "pipe-end", "end of recording")
                    .on_edit(|s: &mut State| &mut s.studio.pipes.end),
            )
            .grow(1.),
        )
        .child(
            labeled(
                "Max rate / frames per s",
                field(&ps.rate, "pipe-rate", "every message")
                    .on_edit(|s: &mut State| &mut s.studio.pipes.rate),
            )
            .grow(1.),
        );
    let left = col()
        .key("pipe-extract")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(labeled(
            "Topics / space separated",
            field(&ps.topics, "pipe-topics", "/camera/image_raw")
                .on_edit(|s: &mut State| &mut s.studio.pipes.topics),
        ))
        .child(topic_toggles(s))
        .child(numbers)
        .child(tabs(
            "pipe-format",
            &FORMATS,
            ps.format.min(1),
            |s: &mut State, i| s.studio.pipes.format = i,
        ))
        .child(labeled(
            "Output name / optional",
            field(&ps.name, "pipe-name", "UTC time stamp")
                .on_edit(|s: &mut State| &mut s.studio.pipes.name),
        ))
        .child(
            row().child(pill("extract-frames", "Extract frames", can, |s| {
                pipelines::start(s, JobKind::Extract)
            })),
        );
    let right = col()
        .w_full()
        .min_w(0.)
        .gap(10.)
        .child(meta("Output"))
        .child(p(
            "<recording>/extract/<name>/ with index.csv (timestamp_ns, topic, file) and manifest.json: source MCAP hashes, topics, counts, a sha256 for every file and the reader's versions.",
            11.,
        ))
        .child(p(
            "Image topics become PNG (rgb8, bgr8, mono8, mono16/16UC1) or NPZ arrays. Compressed images keep their JPEG/PNG bytes. Other messages are written as JSON lines per topic.",
            11.,
        ))
        .child(p(
            "Reads with the mcap + mcap-ros2-support Python packages, or ROS 2 rosbag2_py when those are missing. Cancel keeps the partial output and marks it Cancelled.",
            11.,
        ));
    col()
        .w_full()
        .min_w(0.)
        .pt(16.)
        .child(section_head(
            "Extract frames",
            meta("PNG or NPZ / index + manifest"),
            vp.compact(),
        ))
        .child(asset(left, right, vp.compact()))
}

fn infer_form(s: &State, vp: &Viewport) -> El {
    let ps = &s.studio.pipes;
    let can = s.wsl_ready() && ps.job.is_none() && ps.selected.is_some();
    let images: Vec<String> = ps
        .recording()
        .map(|r| {
            r.topics
                .iter()
                .filter(|t| t.image())
                .map(|t| t.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let names: Vec<&str> = images.iter().map(String::as_str).collect();
    let current = names
        .iter()
        .position(|n| *n == ps.image_topic.text.trim())
        .unwrap_or(usize::MAX);
    let picker = if names.is_empty() {
        notice(if ps.selected.is_some() {
            "This recording has no Image or CompressedImage topic."
        } else {
            "Select a recording to choose its image topic."
        })
    } else {
        let list = images.clone();
        tabs(
            "pipe-image-topic",
            &names,
            current,
            move |s: &mut State, i| s.studio.pipes.image_topic = TextEdit::new(&list[i]),
        )
    };
    let numbers = if vp.compact() { col() } else { row() }
        .w_full()
        .min_w(0.)
        .gap(16.)
        .child(
            labeled(
                "Replay domain / 0–232",
                field(&ps.domain, "pipe-domain", "78")
                    .on_edit(|s: &mut State| &mut s.studio.pipes.domain),
            )
            .grow(1.),
        )
        .child(
            labeled(
                "Replay speed / 0.1–4",
                field(&ps.speed, "pipe-speed", "1.0")
                    .on_edit(|s: &mut State| &mut s.studio.pipes.speed),
            )
            .grow(1.),
        )
        .child(
            labeled(
                "Project / optional",
                field(&ps.project, "pipe-project", "uses its infer_node.py")
                    .on_edit(|s: &mut State| &mut s.studio.pipes.project),
            )
            .grow(1.),
        );
    let left = col()
        .key("pipe-infer")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(labeled(
            "ONNX model / Linux or Windows path",
            field(&ps.model, "pipe-model", "/home/user/models/detector.onnx")
                .on_edit(|s: &mut State| &mut s.studio.pipes.model),
        ))
        .child(
            col()
                .w_full()
                .min_w(0.)
                .gap(0.)
                .child(caps("Image topic", 10., 400., 0.08, muted()))
                .child(picker),
        )
        .child(numbers)
        .child(
            row().child(pill("infer-replay", "Infer on replay", can, |s| {
                pipelines::start(s, JobKind::Infer)
            })),
        );
    let right = col()
        .w_full()
        .min_w(0.)
        .gap(10.)
        .child(meta("What runs"))
        .child(p(
            "Three owned processes on a localhost-only ROS domain: a recorder for /drone_deck/detections, the inference node (onnxruntime, CPU) and ros2 bag play of the image topic only. They stop together when the replay ends or you cancel.",
            11.,
        ))
        .child(p(
            "Each image is resized to the model input and scaled to 0–1; each output is published as JSON (small tensors in full, otherwise the top-5 scores). Decoding boxes or classes is model-specific: copy scripts/agent/infer_node.py into a project as infer_node.py and name that project here.",
            11.,
        ))
        .child(p(
            "Output: <recording>/inference/<run>/ with the detections MCAP and run.json (model sha256, recording, topics, timings, exit states). Needs ROS 2 Jazzy and the ONNX environment from Setup.",
            11.,
        ));
    col()
        .w_full()
        .min_w(0.)
        .pt(16.)
        .border_sides([1., 0., 0., 0.], rule())
        .child(section_head(
            "Infer on replay",
            meta("Isolated domain / owned processes"),
            vp.compact(),
        ))
        .child(asset(left, right, vp.compact()))
}

fn run_detail(run: &Run) -> String {
    if run.kind == "inference" {
        let model = run
            .model
            .as_deref()
            .map(|m| m.rsplit('/').next().unwrap_or(m))
            .unwrap_or("model not recorded");
        let detections = run.detections.map_or_else(
            || "detections not counted".into(),
            |n| format!("{n} detection messages"),
        );
        format!(
            "{model} on {} / {detections}",
            run.topic.as_deref().unwrap_or("topic not recorded")
        )
    } else {
        format!(
            "{} file(s) / {} / {}",
            run.count.unwrap_or(0),
            run.format
                .as_deref()
                .unwrap_or("format not recorded")
                .to_uppercase(),
            if run.topics.is_empty() {
                "topics not recorded".into()
            } else {
                run.topics.join(" ")
            }
        )
    }
}

fn run_row(s: &State, i: usize, run: &Run, compact: bool) -> El {
    let tone = match run.status.as_str() {
        "Completed" => success(),
        "Running" => fg(),
        "Cancelled" | "Interrupted" => warn(),
        "Failed" => danger(),
        _ => muted(),
    };
    let kind = if run.kind == "inference" {
        "Inference"
    } else {
        "Extract"
    };
    let lead_col = col()
        .w_full()
        .min_w(0.)
        .gap(6.)
        .child(lead(kind, muted(), compact))
        .child(
            row()
                .gap(6.)
                .center_y()
                .child(dot(tone))
                .child(meta_sized(run.status.clone(), 9., fg()).wrap()),
        );
    let main = col()
        .w_full()
        .min_w(0.)
        .gap(4.)
        .child(h3(run.name.clone(), 15.).wrap_anywhere())
        .child(p(run_detail(run), 11.))
        .child(code(run.recording.clone(), 11., fg()).wrap_anywhere())
        .child(code(run.path.clone(), 11., muted()).wrap_anywhere());
    let ready = s.wsl_ready();
    let path = run.path.clone();
    let mut pull = col().gap(2.).items_end().child(ghost(
        format!("open-run-{i}"),
        "Show folder ↗",
        ready,
        move |s| {
            let unc = format!(
                "\\\\wsl.localhost\\{}{}",
                s.config.distro,
                path.replace('/', "\\")
            );
            s.open(unc)
        },
    ));
    if let Some(mcap) = run.mcap.first().cloned() {
        pull = pull.child(ghost(
            format!("foxglove-run-{i}"),
            "Open in Foxglove ↗",
            ready,
            move |s| foxglove::open(s, mcap.clone()),
        ));
    }
    log_row(format!("pipe-run-{i}"), lead_col, main, pull, compact)
}

fn runs(s: &State, vp: &Viewport) -> El {
    let ps = &s.studio.pipes;
    let mut list = col().key("pipe-runs").w_full().min_w(0.);
    for (i, run) in ps.runs.iter().enumerate() {
        list = list.child(run_row(s, i, run, vp.compact()));
    }
    if ps.runs.is_empty() {
        list = list.child(empty(
            "pipe-runs-empty",
            "No pipeline runs yet",
            "Extractions and inference runs appear here with their status and outputs. Each keeps a manifest next to its recording.",
        ));
    }
    col()
        .w_full()
        .min_w(0.)
        .pt(16.)
        .border_sides([1., 0., 0., 0.], rule())
        .child(section_head(
            "Runs",
            meta(format!("{} runs / newest first", ps.runs.len())),
            vp.compact(),
        ))
        .child(list)
}

pub(super) fn tab(s: &State, vp: &Viewport) -> El {
    let left = col()
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(h2("From recordings to frames and detections.", 22.))
        .child(p(
            "Pipelines read finalized MCAP recordings in the selected Ubuntu. Outputs are written next to the recording with hashes and tool versions; the recording itself is never changed.",
            12.,
        ))
        .child(meta("Recording"))
        .child(recordings(s));
    col()
        .key("pipelines-tab")
        .w_full()
        .min_w(0.)
        .child(asset(left, live_panel(s), vp.compact()))
        .child(extract_form(s, vp))
        .child(infer_form(s, vp))
        .child(runs(s, vp))
}

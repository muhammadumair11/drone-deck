//! Data & models → Recordings: rosbag2 / MCAP capture, the finalized library and isolated replay.
use super::*;
use crate::ui::{kit::*, theme::*};

/// Sends a recorder request and records it in Activity once the worker accepted it.
fn act(s: &mut State, action: &str, record: &str) {
    send(s, action);
    if s.recorder.pending {
        activity::record(s, record);
    }
}

fn choose(row: El, name: &str, selected: bool, on: impl Fn(&mut State) + 'static) -> El {
    row.role("button")
        .label(name)
        .focus_selected(selected)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .fill(if selected {
            soft()
        } else {
            rune::Color::TRANSPARENT
        })
        .hover(soft())
        .focus_ring(2., 0., fg())
        .on_press(on)
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / 1048576.)
}

fn input(
    edit: &TextEdit,
    key: &str,
    name: &str,
    placeholder: &str,
    active: bool,
    on: impl Fn(&mut State) -> &mut TextEdit + 'static,
) -> El {
    let f = field(edit, key, placeholder);
    labeled(
        name,
        if active {
            f.disabled(true).dim(0.5)
        } else {
            f.on_edit(on)
        },
    )
}

fn topics(s: &State, active: bool, idle: bool) -> El {
    let r = &s.recorder;
    let chosen = topic_names(&r.topics.text);
    let mut list = col().key("capture-topics-list").w_full().min_w(0.);
    for (i, t) in r.discovered.iter().take(80).enumerate() {
        let name = t.name.clone();
        let on = chosen.contains(&name);
        let entry = ruled(false)
            .key(format!("capture-topic-{i}"))
            .gap(12.)
            .py(10.)
            .px(4.)
            .items_start()
            .child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .gap(3.)
                    .child(code(name.clone(), 12., fg()).wrap_anywhere())
                    .child(meta(t.message_type.clone()).wrap()),
            )
            .child(meta_sized(
                if on { "✓ Selected" } else { "+ Add" },
                10.,
                if on { fg() } else { muted() },
            ));
        list = list.child(if active {
            entry.disabled(true).dim(0.5)
        } else {
            choose(entry, &name.clone(), on, move |s| toggle_topic(s, &name))
        });
    }
    if r.discovered.is_empty() {
        list = list.child(p(
            if r.ready {
                "No topics discovered yet. Discover on the capture domain, or type absolute topic names below."
            } else {
                "Check dependencies first, then discover live topics on the capture domain."
            },
            11.,
        ));
    }
    col()
        .w_full()
        .min_w(0.)
        .gap(8.)
        .child(
            row()
                .w_full()
                .gaps(4., 20.)
                .flex_wrap()
                .center_y()
                .child(meta(format!(
                    "Live topics / {} discovered",
                    r.discovered.len()
                )))
                .child(spacer())
                .child(ghost(
                    "discover-topics",
                    "Discover topics",
                    r.ready && idle,
                    |s| send(s, "topics"),
                )),
        )
        .child(
            col()
                .w_full()
                .min_w(0.)
                .max_h(280.)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(list),
        )
}

fn capture(s: &State, active: bool, idle: bool) -> El {
    let r = &s.recorder;
    let clock = if r.sim_time {
        "Clock: ROS /clock"
    } else {
        "Clock: system time"
    };
    col()
        .key("capture-panel")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(h2("Sensor capture & replay", 22.))
        .child(p(
            "rosbag2 records selected live ROS topics as MCAP. Stop gracefully to finalize metadata. Replay uses a separate ROS domain, normally 77.",
            12.,
        ))
        .child(input(
            &r.name,
            "recording-name",
            "Session name / letters, numbers, - and _",
            "flight",
            active,
            |s| &mut s.recorder.name,
        ))
        .child(input(
            &r.source,
            "recording-source",
            "Vehicle or source note",
            "Simulation",
            active,
            |s| &mut s.recorder.source,
        ))
        .child(input(
            &r.capture_domain,
            "capture-domain",
            "Capture ROS_DOMAIN_ID / 0–232",
            "0",
            active,
            |s| &mut s.recorder.capture_domain,
        ))
        .child(topics(s, active, idle))
        .child(input(
            &r.topics,
            "capture-topics",
            "Topics to record / spaces or commas, absolute names",
            "/fmu/out/sensor_combined",
            active,
            |s| &mut s.recorder.topics,
        ))
        .child(
            row()
                .gaps(4., 20.)
                .flex_wrap()
                .center_y()
                .child(ghost("clock-mode", clock, !active, |s| {
                    s.recorder.sim_time = !s.recorder.sim_time
                })),
        )
        .child(notice(
            "Use ROS /clock only when your simulator publishes it; capture waits for the first clock message.",
        ))
        .child(
            row()
                .gaps(8., 20.)
                .flex_wrap()
                .center_y()
                .child(pill(
                    "start-recording",
                    "Start MCAP recording",
                    r.ready && idle,
                    |s| {
                        let name = s.recorder.name.text.trim().to_string();
                        act(s, "record", &format!("Started recording {name}"))
                    },
                ))
                .child(ghost(
                    "stop-recording",
                    "Stop & finalize",
                    r.recording && !r.pending,
                    |s| act(s, "stop", "Requested stop and finalize of a recording"),
                )),
        )
        .child(notice(
            "Files split at 1 GiB. Stop & finalize flushes MCAP and rosbag metadata. A running process does not prove that the selected topics are publishing.",
        ))
}

fn readiness(s: &State, idle: bool) -> El {
    let r = &s.recorder;
    let (label, tone) = if r.recording {
        ("Recording process active", fg())
    } else if r.playing {
        ("Replay process active", fg())
    } else if r.ready {
        ("Dependencies ready", success())
    } else {
        ("Dependencies not checked", muted())
    };
    let mut col = col()
        .key("recording-readiness")
        .w_full()
        .min_w(0.)
        .gap(12.)
        .child(meta("Readiness"))
        .child(
            row()
                .gap(10.)
                .center_y()
                .child(dot(tone))
                .child(meta_sized(label, 10., fg())),
        )
        .child(
            body(r.message.clone(), 11., if r.ready { fg() } else { muted() })
                .key("recording-message")
                .role("status"),
        )
        .child(
            row()
                .gaps(8., 20.)
                .flex_wrap()
                .center_y()
                .child(pill(
                    "check-recording-tools",
                    "Check dependencies",
                    idle,
                    |s| {
                        send(s, "check");
                        send(s, "list");
                    },
                ))
                .child(ghost("recording-guide", "Recording guide ↗", true, |s| {
                    let root = backend::root();
                    let guide = root.parent().unwrap_or(&root).join("drone-deck/RECORDINGS.md");
                    s.open(guide.to_string_lossy().into_owned())
                })),
        )
        .child(p("PX4 SIH alone does not publish ROS sensor topics.", 11.))
        .child(p(
            "PX4 sensor capture needs a running PX4 → ROS 2 bridge (DDS Agent) and matching message packages.",
            11.,
        ));
    if s.systems.active != systems::ROS {
        col = col.child(
            body(
                format!(
                    "The active system is {}. Live capture is planned for the ROS & sensor capture system; prepare its dependencies first.",
                    systems::KINDS[s.systems.active]
                ),
                11.,
                warn(),
            )
            .key("recording-system-note"),
        );
    }
    col
}

fn library(s: &State, vp: &Viewport) -> El {
    let r = &s.recorder;
    let mut list = col().w_full().min_w(0.);
    for (i, session) in r.sessions.iter().enumerate() {
        let id = session.id.clone();
        let selected = r.selected.as_ref() == Some(&id);
        let tone = if session.status.eq_ignore_ascii_case("finalized") {
            fg()
        } else {
            warn()
        };
        let main = col()
            .w_full()
            .min_w(0.)
            .gap(4.)
            .child(h3(session.label.clone(), 15.))
            .child(p(
                format!(
                    "{} messages / {:.1}s / {}",
                    session.messages,
                    session.duration,
                    mib(session.bytes)
                ),
                11.,
            ));
        let entry = log_row(
            format!("recording-session-{i}"),
            meta_sized(
                session.status.clone(),
                if vp.compact() { 9. } else { 10. },
                tone,
            )
            .wrap(),
            main,
            meta_sized(
                if selected { "Selected" } else { "Select" },
                10.,
                if selected { fg() } else { muted() },
            ),
            vp.compact(),
        );
        let label = session.label.clone();
        list = list.child(choose(entry, &label, selected, move |s| {
            s.recorder.selected = Some(id.clone())
        }));
        if selected {
            list = list.child(detail(session, vp));
        }
    }
    if r.sessions.is_empty() {
        list = list.child(empty(
            "recordings-empty",
            "No finalized recordings",
            if r.ready {
                "Completed recordings appear here. Dataset versions reference the originals; they do not copy the files."
            } else {
                "Completed recordings appear here once dependencies are checked. Dataset versions reference the originals; they do not copy the files."
            },
        ));
    }
    col()
        .key("recording-library")
        .w_full()
        .min_w(0.)
        .pt(16.)
        .child(section_head(
            "Recording library",
            ghost("refresh-recordings", "Refresh library", !r.pending, |s| {
                send(s, "list")
            }),
            vp.compact(),
        ))
        .child(code(r.root.clone(), 11., muted()).wrap_anywhere().mb(12.))
        .child(list)
}

fn detail(session: &Session, vp: &Viewport) -> El {
    let mut topics = col().w_full().min_w(0.).gap(4.);
    for t in &session.topics {
        topics = topics.child(
            code(
                format!("{} · {} · {} messages", t.name, t.message_type, t.count),
                11.,
                muted(),
            )
            .wrap_anywhere(),
        );
    }
    if session.topics.is_empty() {
        topics = topics.child(p("No topic metadata recorded for this session.", 11.));
    }
    col()
        .key("recording-detail")
        .w_full()
        .min_w(0.)
        .gap(10.)
        .pb(18.)
        .pl(if vp.compact() { 70. } else { 124. })
        .child(
            meta(format!(
                "Source {} / clock {}",
                if session.source.is_empty() {
                    "not recorded"
                } else {
                    &session.source
                },
                if session.clock.is_empty() {
                    "not recorded"
                } else {
                    &session.clock
                }
            ))
            .wrap(),
        )
        .child(topics)
        .child(row().child(ghost(
            "open-recording-folder",
            "Open folder ↗",
            true,
            open_selected,
        )))
}

fn replay(s: &State, active: bool, idle: bool, vp: &Viewport) -> El {
    let r = &s.recorder;
    let left = col()
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(h2("Offline replay", 19.))
        .child(p(
            "Replay is local-only on a different ROS domain; known flight-input topics are excluded. Analysis nodes must use the replay domain. Replay does not imply a live vehicle connection.",
            12.,
        ))
        .child(
            body(
                match r.sessions.iter().find(|x| Some(&x.id) == r.selected.as_ref()) {
                    Some(x) => format!("Selected: {}", x.label),
                    None => "Select a finalized recording in the library above.".into(),
                },
                11.,
                fg(),
            ),
        );
    let right = col()
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(input(
            &r.replay_domain,
            "replay-domain",
            "Replay ROS_DOMAIN_ID / default 77",
            "77",
            active,
            |s| &mut s.recorder.replay_domain,
        ))
        .child(input(
            &r.rate,
            "replay-rate",
            "Playback speed / 0.1–4.0",
            "1.0",
            active,
            |s| &mut s.recorder.rate,
        ))
        .child(
            row()
                .gaps(8., 20.)
                .flex_wrap()
                .center_y()
                .child(pill(
                    "start-replay",
                    "Replay selected recording",
                    r.ready && idle && r.selected.is_some(),
                    |s| act(s, "replay", "Started replay of a recording"),
                ))
                .child(ghost(
                    "stop-replay",
                    "Stop replay",
                    r.playing && !r.pending,
                    |s| act(s, "stop", "Requested stop of replay"),
                )),
        );
    col()
        .key("replay-panel")
        .w_full()
        .min_w(0.)
        .mt(16.)
        .border_sides([1., 0., 0., 0.], rule())
        .child(asset(left, right, vp.compact()))
}

/// The Recordings tab of Data & models.
pub(crate) fn page(s: &State, vp: &Viewport) -> El {
    let r = &s.recorder;
    let active = r.recording || r.playing;
    let idle = !active && !r.pending;
    let stacked = vp.compact() || vp.content() < 1000.;
    let top = if stacked {
        asset(readiness(s, idle), capture(s, active, idle), true)
    } else {
        asset(capture(s, active, idle), readiness(s, idle), false)
    };
    col()
        .key("recording-columns")
        .w_full()
        .min_w(0.)
        .child(top)
        .child(library(s, vp))
        .child(replay(s, active, idle, vp))
}

//! The listener panels of the System page, each shown only when the system has that listener
//! (`panels::panels`): QGroundControl, MAVSDK scripts, ROS 2, the recorder, Foxglove and
//! inference. They reuse the state and actions of the feature they show.
use super::*;

fn panel(key: &'static str, title: &str, right: El, vp: &Viewport) -> El {
    col()
        .key(key)
        .w_full()
        .min_w(0.)
        .gap(12.)
        .pt(20.)
        .child(section_head(title, right, vp.compact()))
}

fn state_line(s: &State, id: &str) -> El {
    let (label, tone) = systems::profile_state(s, id);
    row()
        .gap(8.)
        .center_y()
        .child(dot(systems::tone_color(tone)))
        .child(meta_sized(label, 10., fg()))
}

fn code_line(key: &'static str, value: String) -> El {
    code(value, 12., fg())
        .key(key)
        .wrap_anywhere()
        .selectable(true)
}

pub(super) fn ground_station(s: &State, vp: &Viewport) -> El {
    let on = systems::running(s, "qgc");
    panel("panel-qgc", "QGroundControl", state_line(s, "qgc"), vp)
        .child(p(
            "Flight control stays in QGroundControl. It starts with the session and connects to the vehicle link on UDP 14550.",
            12.,
        ))
        .child(ghost(
            "panel-qgc-toggle",
            if on {
                "Stop QGroundControl"
            } else {
                "Open QGroundControl ↗"
            },
            true,
            |s| super::runtime::toggle_profile(s, "qgc"),
        ))
}

pub(super) fn scripts(s: &State, vp: &Viewport) -> El {
    let sys = s.systems.current();
    let addr = sys.consumers.scripts_udp.clone().unwrap_or_default();
    let sent = s
        .vehicle
        .snapshot
        .as_ref()
        .map_or(0, |snap| snap.link.peer_to_vehicle_frames);
    let mut c = panel(
        "panel-scripts",
        "MAVSDK scripts",
        meta(format!("UDP {addr}")),
        vp,
    )
    .child(code_line("panel-scripts-mavsdk", format!("udpin://{addr}")))
    .child(code_line(
        "panel-scripts-pymavlink",
        format!("udpin:{addr}"),
    ));
    c = c.child(p(
        &format!(
            "What your scripts send reaches the vehicle unchanged (two-way MAVLink). Frames sent by your scripts and QGroundControl so far: {sent}."
        ),
        12.,
    ));
    c
}

pub(super) fn ros(s: &State, vp: &Viewport) -> El {
    let sys = s.systems.current();
    let domain = sys.consumers.ros.domain;
    let bridge = sys
        .adapter()
        .and_then(|a| a.ros.as_ref())
        .filter(|_| sys.consumers.ros.agent)
        .map(|r| r.agent);
    let right = match bridge {
        Some(id) => state_line(s, id),
        None => meta(format!("Domain {domain}")),
    };
    let mut topics = row().gaps(6., 8.).flex_wrap().key("panel-ros-topics");
    let found = &s.recorder.discovered;
    for t in found.iter().take(14) {
        topics = topics.child(code(t.name.clone(), 11., fg()));
    }
    if found.len() > 14 {
        topics = topics.child(meta(format!("+ {} more", found.len() - 14)));
    }
    let mut c = panel("panel-ros", "ROS 2", right, vp).child(meta(format!("Domain {domain}")));
    c = if found.is_empty() {
        c.child(p(
            "No topics listed yet. Topics appear once the bridge and your nodes publish on this domain.",
            12.,
        ))
    } else {
        c.child(topics)
    };
    c.child(code_line(
        "panel-ros-export",
        format!("export ROS_DOMAIN_ID={domain}"),
    ))
    .child(ghost(
        "panel-ros-refresh",
        "Refresh topics",
        !s.recorder.pending,
        |s| {
            let action = if s.recorder.ready { "topics" } else { "check" };
            crate::recordings::send(s, action)
        },
    ))
}

pub(super) fn recorder(s: &State, vp: &Viewport) -> El {
    let r = &s.recorder;
    let sys = s.systems.current();
    let id = sys.id.clone();
    let right = meta(if r.recording {
        "Recording".to_string()
    } else {
        r.message.clone()
    });
    let mut c = panel("panel-recorder", "Recorder", right, vp)
        .child(
            row()
                .gaps(8., 16.)
                .flex_wrap()
                .center_y()
                .child(if r.recording {
                    pill("panel-recorder-stop", "Stop recording", !r.pending, |s| {
                        crate::recordings::send(s, "stop")
                    })
                } else {
                    pill(
                        "panel-recorder-start",
                        "Start recording",
                        !r.pending && !r.playing,
                        |s| {
                            if s.recorder.ready {
                                crate::recordings::send(s, "record")
                            } else {
                                crate::recordings::send(s, "check")
                            }
                        },
                    )
                })
                .child(body(
                    format!(
                        "Records this system's topics on ROS domain {} into the recording library.",
                        sys.consumers.ros.domain
                    ),
                    11.,
                    muted(),
                )),
        )
        .child(meta("Topics"))
        .child(code_line(
            "panel-recorder-topics",
            if r.topics.text.trim().is_empty() {
                "None chosen yet: choose them in Data & models → Recordings".into()
            } else {
                r.topics.text.clone()
            },
        ));
    if sys.simulated() {
        c = c.child(checkbox(
            "panel-recorder-auto",
            "Record when the session starts",
            sys.consumers.ros.record_on_start,
            |s| {
                let id = s.systems.active_id().to_string();
                if let Some(sys) = s.systems.get_mut(&id) {
                    sys.consumers.ros.record_on_start = !sys.consumers.ros.record_on_start;
                }
                if let Err(e) = s.systems.save(s.headless) {
                    s.notice = format!("The system could not be saved: {e}");
                }
            },
        ));
    }
    let mine: Vec<_> = r
        .sessions
        .iter()
        .filter(|x| x.system == id)
        .take(5)
        .collect();
    c = c.child(meta("Recent recordings from this system"));
    if mine.is_empty() {
        c = c.child(ghost(
            "panel-recorder-list",
            "Show recordings",
            !r.pending,
            |s| crate::recordings::send(s, "list"),
        ));
    }
    for (i, rec) in mine.into_iter().enumerate() {
        let mut line = row()
            .key(format!("panel-recorder-row-{i}"))
            .w_full()
            .min_w(0.)
            .gaps(4., 16.)
            .flex_wrap()
            .justify_between()
            .child(body(
                format!("{} / {:.0} s / {}", rec.label, rec.duration, rec.status),
                12.,
                fg(),
            ));
        if rec.recipe.is_some() {
            let rid = rec.id.clone();
            line = line.child(ghost(
                format!("panel-recorder-resim-{i}"),
                "Re-simulate ↗",
                true,
                move |s| crate::resim::open(s, &rid),
            ));
        }
        c = c.child(line);
    }
    c
}

pub(super) fn foxglove(s: &State, vp: &Viewport) -> El {
    let sys = s.systems.current();
    let mut c = panel(
        "panel-foxglove",
        "Foxglove",
        state_line(s, crate::studio::foxglove::PROFILE),
        vp,
    );
    if !sys.is_replay() {
        c = c
            .child(p(
                "The Foxglove bridge starts with the session on ws://localhost:8765.",
                12.,
            ))
            .child(ghost(
                "panel-foxglove-live",
                "Open live ↗",
                true,
                crate::studio::foxglove::toggle_live,
            ));
    }
    let id = sys.id.clone();
    let last = s
        .recorder
        .sessions
        .iter()
        .find(|x| x.system == id || Some(&x.id) == s.recorder.selected.as_ref())
        .map(|x| x.path.clone());
    if let Some(path) = last {
        c = c.child(ghost(
            "panel-foxglove-recording",
            "Open the last recording ↗",
            true,
            move |s| crate::studio::foxglove::open(s, path.clone()),
        ));
    }
    c
}

pub(super) fn inference(s: &State, vp: &Viewport) -> El {
    let p_ = &s.studio.pipes;
    let project = p_.project.text.trim();
    let right = meta(match &p_.job {
        Some(job) => format!("{} · {}", job.kind.name(), job.progress),
        None => "Not running".into(),
    });
    panel("panel-inference", "Inference", right, vp)
        .child(kv_line(
            "Project",
            if project.is_empty() {
                "Any project"
            } else {
                project
            },
        ))
        .child(kv_line(
            "Model",
            if p_.model.text.trim().is_empty() {
                "Choose one in Data & models → Pipelines"
            } else {
                p_.model.text.trim()
            },
        ))
        .child(ghost(
            "panel-inference-open",
            "Run on a recording ↗",
            true,
            |s| {
                s.ui.data_tab = 2;
                s.navigate(Page::Data)
            },
        ))
}

fn kv_line(label: &str, value: &str) -> El {
    row()
        .w_full()
        .min_w(0.)
        .gap(16.)
        .child(meta(label).w(80.))
        .child(code(value.to_string(), 12., fg()).wrap_anywhere())
}

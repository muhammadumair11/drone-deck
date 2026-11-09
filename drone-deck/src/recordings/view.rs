use super::*;
use crate::ui::*;

fn input(
    s: &TextEdit,
    key: &str,
    label_text: &str,
    edit: impl Fn(&mut State) -> &mut TextEdit + 'static,
) -> El {
    col()
        .w_full()
        .min_w(0.)
        .gap(6.)
        .child(label(label_text, 12., muted()).wrap())
        .child(field(s, key).on_edit(edit))
}
fn selection(
    key: String,
    name: String,
    detail: String,
    selected: bool,
    action: impl Fn(&mut State) + 'static,
) -> El {
    col()
        .key(key)
        .role("button")
        .label(name.clone())
        .help(detail.clone())
        .focusable(true)
        .w_full()
        .min_w(0.)
        .min_h(44.)
        .padding(10.)
        .gap(5.)
        .radius(5.)
        .fill(if selected { accent() } else { surface() })
        .border(1., line())
        .focus_ring(2., 2., accent())
        .on_press(action)
        .child(
            label(name, 14., if selected { bg() } else { ink() })
                .w_full()
                .wrap(),
        )
        .child(
            label(detail, 12., if selected { bg() } else { muted() })
                .w_full()
                .wrap(),
        )
}
pub(crate) fn page(s: &State, width: f32) -> El {
    let r = &s.recorder;
    let active = r.recording || r.playing;
    let idle = !active && !r.pending;
    let mut discovery = col().w_full().min_w(0.).gap(8.);
    for (i, t) in r.discovered.iter().take(80).enumerate() {
        let name = t.name.clone();
        let chosen = topic_names(&r.topics.text).contains(&name);
        discovery = discovery.child(
            selection(
                format!("capture-topic-{i}"),
                format!("{} {}", if chosen { "✓" } else { "+" }, name),
                t.message_type.clone(),
                chosen,
                move |s| toggle_topic(s, &name),
            )
            .w_full()
            .disabled(active),
        );
    }
    let capture=card().key("capture-panel")
        .child(heading("Capture sensor streams","ROSBAG2 / MCAP"))
        .child(input(&r.name,"recording-name","Session name (letters, numbers, hyphens, underscores)",|s|&mut s.recorder.name).disabled(active))
        .child(input(&r.source,"recording-source","Vehicle / source note",|s|&mut s.recorder.source).disabled(active))
        .child(input(&r.capture_domain,"capture-domain","Capture ROS_DOMAIN_ID (0–232)",|s|&mut s.recorder.capture_domain).disabled(active))
        .child(row().gap(8.).flex_wrap()
            .child(button("discover-topics","Discover topics",false,|s|send(s,"topics")).disabled(!r.ready||!idle))
            .child(button("clock-mode",if r.sim_time{"Clock: ROS /clock"}else{"Clock: system time"},false,|s|s.recorder.sim_time=!s.recorder.sim_time).disabled(active)))
        .child(label("Use ROS /clock only when your simulator publishes it; capture waits for the first clock message.",12.,muted()).wrap())
        .child(label("PX4 sensor capture needs a running PX4 → ROS 2 bridge and matching message packages. SIH/MAVSDK alone do not publish ROS topics.",12.,muted()).wrap())
        .child(col().w_full().min_w(0.).max_h(240.).scroll(ScrollSpec{stick_bottom:false}).child(discovery))
        .child(input(&r.topics,"capture-topics","Topics to record (spaces or commas; absolute names)",|s|&mut s.recorder.topics).disabled(active))
        .child(row().gap(8.).flex_wrap()
            .child(button("start-recording","Start MCAP recording",true,|s|send(s,"record")).disabled(!r.ready||!idle))
            .child(button("stop-recording","Stop & finalize",false,|s|send(s,"stop")).disabled(!r.recording||r.pending)))
        .child(label("Files split at 1 GiB. Stop & finalize flushes MCAP and rosbag metadata. A running process does not guarantee that selected topics are publishing.",12.,muted()).wrap());
    let mut library = card()
        .key("recording-library")
        .child(heading("Recording library", "LOCAL / WSL"))
        .child(label(&r.root, 12., muted()).wrap())
        .child(
            button("refresh-recordings", "Refresh library", false, |s| {
                send(s, "list")
            })
            .disabled(r.pending),
        );
    if r.sessions.is_empty() {
        library = library.child(
            label(
                "No recordings listed yet. Refresh the library after your first capture.",
                14.,
                muted(),
            )
            .wrap(),
        );
    }
    let mut rows = col().w_full().min_w(0.).gap(8.);
    for (i, row) in r.sessions.iter().enumerate() {
        let id = row.id.clone();
        rows = rows.child(
            selection(
                format!("recording-session-{i}"),
                row.label.clone(),
                format!(
                    "{} · {} messages · {:.1}s · {:.1} MiB",
                    row.status,
                    row.messages,
                    row.duration,
                    row.bytes as f64 / 1048576.
                ),
                r.selected.as_ref() == Some(&id),
                move |s| s.recorder.selected = Some(id.clone()),
            )
            .w_full(),
        );
    }
    library = library.child(
        col()
            .w_full()
            .min_w(0.)
            .max_h(360.)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(rows),
    );
    if let Some(row) = r
        .sessions
        .iter()
        .find(|row| Some(&row.id) == r.selected.as_ref())
    {
        library = library.child(
            label(
                format!("Source: {} · Clock: {}", row.source, row.clock),
                12.,
                muted(),
            )
            .wrap(),
        );
        for topic in &row.topics {
            library = library.child(
                label(
                    format!(
                        "{} · {} · {} messages",
                        topic.name, topic.message_type, topic.count
                    ),
                    12.,
                    muted(),
                )
                .wrap(),
            );
        }
        library = library.child(button(
            "open-recording-folder",
            "Open selected folder",
            false,
            open_selected,
        ));
    }
    let replay=card().key("replay-panel").child(heading("Offline replay","SEPARATE ROS DOMAIN"))
        .child(label("Select a finalized recording above. Replay is local-only on a different ROS domain; known flight-input topics are excluded. Analysis nodes must use the replay domain. Keep this graph separate from live vehicle bridges.",13.,muted()).wrap())
        .child(input(&r.replay_domain,"replay-domain","Replay ROS_DOMAIN_ID (default 77)",|s|&mut s.recorder.replay_domain).disabled(active))
        .child(input(&r.rate,"replay-rate","Playback speed (0.1–4.0)",|s|&mut s.recorder.rate).disabled(active))
        .child(row().gap(8.).flex_wrap()
            .child(button("start-replay","Replay selected recording",true,|s|send(s,"replay")).disabled(!r.ready||!idle||r.selected.is_none()))
            .child(button("stop-replay","Stop replay",false,|s|send(s,"stop")).disabled(!r.playing||r.pending)));
    let body = if width >= 1000. {
        row()
            .key("recording-columns")
            .w_full()
            .min_w(0.)
            .gap(16.)
            .items_start()
            .child(col().grow(1.).min_w(0.).child(capture))
            .child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .gap(16.)
                    .child(library)
                    .child(replay),
            )
    } else {
        col()
            .w_full()
            .min_w(0.)
            .gap(16.)
            .child(capture)
            .child(library)
            .child(replay)
    };
    col()
        .w_full()
        .min_w(0.)
        .gap(16.)
        .child(
            card()
                .child(heading("Recordings", "SENSOR DATA / ROS 2 JAZZY"))
                .child(label(&r.message, 14., if r.ready { ink() } else { muted() }).wrap())
                .child(
                    row()
                        .gap(8.)
                        .flex_wrap()
                        .child(badge(
                            if r.recording {
                                "Recording process active"
                            } else if r.playing {
                                "Replay process active"
                            } else if r.ready {
                                "Dependencies ready"
                            } else {
                                "Not checked"
                            },
                            r.ready,
                        ))
                        .child(
                            button("check-recording-tools", "Check dependencies", true, |s| {
                                send(s, "check");
                                send(s, "list");
                            })
                            .disabled(!idle),
                        )
                        .child(button("recording-guide", "Recording guide", false, |s| {
                            s.open(
                                backend::root()
                                    .parent()
                                    .unwrap()
                                    .join("drone-deck/RECORDINGS.md")
                                    .to_string_lossy()
                                    .into_owned(),
                            )
                        })),
                ),
        )
        .child(body)
}

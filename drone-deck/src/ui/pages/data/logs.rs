//! Data & models → Flight logs: where logs come from, the library, and the selected log's
//! summary, plots and logged messages. Every figure is read from the file; absent topics say
//! "Not logged".
use super::plot::plot;
use super::*;
use crate::mavlink::TlogSummary;
use crate::ulog::{self, Analysis, LogFile, Report, Source, Summary};
use std::path::PathBuf;

fn mib(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        format!("{:.0} KiB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MiB", bytes as f64 / 1_048_576.)
    }
}
pub(super) fn duration(s: f64) -> String {
    if s >= 3600. {
        format!(
            "{} h {:02} min",
            (s / 3600.) as u64,
            ((s % 3600.) / 60.) as u64
        )
    } else if s >= 60. {
        format!("{} min {:02} s", (s / 60.) as u64, (s % 60.) as u64)
    } else {
        format!("{s:.1} s")
    }
}
fn opt(v: Option<f64>, f: impl Fn(f64) -> String) -> String {
    v.map_or_else(|| "Not logged".into(), f)
}

/// The date shown for a file: GPS time from its analysis, PX4's `YYYY-MM-DD/HH_MM_SS.ulg`
/// name, or the file time.
fn date(f: &LogFile, a: Option<&Analysis>) -> String {
    if let Some(Analysis::Ready(r)) = a {
        let utc = match r.as_ref() {
            Report::Ulog { summary, .. } => summary.utc_start_us,
            Report::Tlog(t) => (t.frames > 0).then_some(t.start_us),
        };
        if let Some(us) = utc {
            return ulog::utc(us / 1_000_000);
        }
    }
    let day = f
        .path
        .parent()
        .and_then(|p| p.file_name())
        .map(|d| d.to_string_lossy().into_owned())
        .filter(|d| d.len() == 10 && d.as_bytes()[4] == b'-');
    let stem = f
        .path
        .file_stem()
        .map(|n| n.to_string_lossy().replace('_', ":"))
        .filter(|n| n.len() == 8 && n.as_bytes()[2] == b':');
    match (day, stem) {
        (Some(d), Some(t)) => format!("{d} {}", &t[..5]),
        _ => ulog::modified_utc(f.modified).unwrap_or_else(|| "Unknown date".into()),
    }
}

fn status_line(s: &State) -> String {
    let logs = &s.flight_logs;
    if logs.preview {
        return "Headless preview fixture · synthetic files, not read from disk".into();
    }
    if s.headless {
        return "Headless preview · log discovery is disabled".into();
    }
    if logs.scanning {
        return "Searching log folders…".into();
    }
    if !logs.scanned {
        return "Not searched yet".into();
    }
    format!(
        "{} file{} found",
        logs.files.len(),
        if logs.files.len() == 1 { "" } else { "s" }
    )
}

fn sources(s: &State) -> El {
    let logs = &s.flight_logs;
    let distro = &s.config.distro;
    let wsl = if logs.preview || s.headless {
        "Not searched in headless preview".to_string()
    } else if !s.wsl_ready() {
        format!("{distro} is not a ready WSL 2 distribution yet; SITL logs are not searched")
    } else if logs.scanning || !logs.scanned {
        format!(r"\\wsl.localhost\{distro} · searching")
    } else if !logs.wsl_reachable {
        format!(r"\\wsl.localhost\{distro} and \\wsl$\{distro} did not answer")
    } else if logs.searched.is_empty() {
        "No PX4 build with rootfs\\log found in the PX4 folders the profiles name".into()
    } else {
        format!("{} SITL log folder(s) searched", logs.searched.len())
    };
    let root = crate::backend::root();
    let item = |title: &str, path: String, state: String| {
        ruled(false).py(10.).child(
            col()
                .w_full()
                .min_w(0.)
                .gap(4.)
                .child(meta_sized(title, 9., muted()))
                .child(code(path, 11., fg()).wrap_anywhere())
                .child(meta_sized(state, 9., muted()).wrap()),
        )
    };
    col()
        .key("flight-logs-sources")
        .w_full()
        .min_w(0.)
        .child(item(
            "PX4 SITL (WSL)",
            format!(r"\\wsl.localhost\{distro}\…\PX4-Autopilot\build\*\rootfs\log"),
            wsl,
        ))
        .child(item(
            "Vehicle downloads",
            ulog::vehicle_dir(&root).display().to_string(),
            "ULog files downloaded from a vehicle".into(),
        ))
        .child(item(
            "Telemetry captures",
            crate::vehicle::tlog_dir().display().to_string(),
            ".tlog of every vehicle link session".into(),
        ))
        .child(item(
            "Captures from earlier versions",
            root.join("hardware").display().to_string(),
            ".tlog files of the retired vehicle monitor".into(),
        ))
        .child(row().pt(6.).child(ghost(
            "open-vehicle-logs",
            "Downloads folder ↗",
            !s.headless,
            |s: &mut State| {
                let dir = ulog::vehicle_dir(&crate::backend::root());
                let _ = std::fs::create_dir_all(&dir);
                s.open(dir.display().to_string());
            },
        )))
}

fn intro(s: &State) -> El {
    col()
        .w_full()
        .min_w(0.)
        .gap(12.)
        .child(h2("Flight logs", 19.))
        .child(p(
            "PX4 ULog files from simulation and from vehicles, plus telemetry captures. Drone Deck reads them locally and summarizes what was logged; detailed analysis stays with PX4 Flight Review.",
            12.,
        ))
        .child(meta(status_line(s)).wrap())
}

/// Size over duration (or why there is none), right-aligned.
fn pull(f: &LogFile, a: Option<&Analysis>, selected: bool, compact: bool) -> El {
    let state = match a {
        Some(Analysis::Ready(r)) => duration(r.duration_s()),
        Some(Analysis::Pending) => "Reading…".into(),
        Some(Analysis::Failed(_)) => "Unreadable".into(),
        None => "Not read".into(),
    };
    let tone = if selected { fg() } else { muted() };
    let c = col()
        .gap(4.)
        .child(meta_sized(mib(f.size), 10., tone))
        .child(meta_sized(
            state,
            10.,
            if matches!(a, Some(Analysis::Failed(_))) {
                danger()
            } else {
                tone
            },
        ));
    if compact { c } else { c.items_end() }
}

fn library(s: &State, vp: &Viewport) -> El {
    let logs = &s.flight_logs;
    let mut list = col().w_full().min_w(0.);
    for (i, f) in logs.files.iter().enumerate() {
        let a = logs.analysis(f);
        let selected = logs.selected.as_ref() == Some(&f.path);
        let main = col()
            .w_full()
            .min_w(0.)
            .gap(4.)
            .child(h3(f.name(), 15.).wrap_anywhere())
            .child(p(f.source.label(), 11.))
            .child(code(f.path.display().to_string(), 10., muted()).wrap_anywhere());
        let entry = log_row(
            format!("flight-log-{i}"),
            meta_sized(date(f, a), if vp.compact() { 9. } else { 10. }, fg()).wrap(),
            main,
            pull(f, a, selected, vp.compact()),
            vp.compact(),
        );
        let path = f.path.clone();
        list = list.child(
            entry
                .role("button")
                .label(f.name())
                .focus_selected(selected)
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .fill(if selected { soft() } else { Color::TRANSPARENT })
                .hover(soft())
                .focus_ring(2., 0., fg())
                .on_press(move |s: &mut State| ulog::select(s, &path)),
        );
        if selected {
            list = list.child(detail(s, f, a, vp));
        }
    }
    if logs.files.is_empty() {
        let text = if s.headless {
            "Headless preview: no folders are searched. Run Drone Deck normally to list SITL logs, vehicle downloads and telemetry captures."
        } else if logs.scanning || !logs.scanned {
            "Searching the simulation, download and capture folders listed above."
        } else if !s.wsl_ready() {
            "No vehicle downloads or telemetry captures yet, and SITL logs are not searched until the Linux environment is ready (Settings → Environment). PX4 writes a ULog each time the simulator arms."
        } else {
            "PX4 writes a ULog under build/<target>/rootfs/log each time the simulator arms. Fly a simulated session, download logs from a vehicle, or capture telemetry, then refresh."
        };
        list = list.child(empty("flight-logs-empty", "No flight logs found", text));
    }
    col()
        .key("flight-log-library")
        .w_full()
        .min_w(0.)
        .pt(16.)
        .child(section_head(
            "Log library",
            ghost(
                "refresh-logs",
                "Refresh list",
                !s.headless && !logs.scanning,
                ulog::refresh,
            ),
            vp.compact(),
        ))
        .child(list)
}

fn fact(key: &str, label: &str, value: String, tone: Color) -> El {
    col()
        .key(format!("summary-{key}"))
        .w_full()
        .min_w(0.)
        .gap(6.)
        .py(12.)
        .border_sides([1., 0., 0., 0.], soft())
        .child(meta_sized(label, 9., muted()))
        .child(meta_sized(value, 12., tone).wrap())
}
fn facts(items: Vec<El>, compact: bool) -> El {
    let mut grid = row()
        .key("log-summary")
        .w_full()
        .min_w(0.)
        .gaps(0., 24.)
        .flex_wrap();
    for item in items {
        grid = grid.child(
            col()
                .w(if compact { 148. } else { 200. })
                .min_w(0.)
                .child(item),
        );
    }
    grid
}

fn ulog_facts(m: &Summary, compact: bool) -> El {
    let battery = match (m.battery_min_v, m.battery_min_pct) {
        (None, None) => "Not logged".into(),
        (v, pct) => format!(
            "{} / {}",
            v.map_or("—".into(), |v| format!("{v:.2} V")),
            pct.map_or("—".into(), |p| format!("{p:.0} %"))
        ),
    };
    let armed = match &m.armed {
        None => "Not logged".into(),
        Some(p) if p.is_empty() => "Never armed".into(),
        Some(p) => format!(
            "{} period{} · {}",
            p.len(),
            if p.len() == 1 { "" } else { "s" },
            duration(p.iter().map(|(a, b)| b - a).sum())
        ),
    };
    let (failsafe, failsafe_tone) = match &m.failsafes {
        None => ("Not logged".into(), fg()),
        Some(f) if f.is_empty() => ("None".into(), fg()),
        Some(f) => (format!("{} · first at {:.1} s", f.len(), f[0]), danger()),
    };
    let estimator = match &m.estimator {
        None => ("Not logged".into(), fg()),
        Some(e) if e.fault_samples > 0 => (
            format!(
                "Faults in {} samples (0x{:x})",
                e.fault_samples, e.fault_flags
            ),
            danger(),
        ),
        Some(e) if e.innovation_fail_samples > 0 => (
            format!(
                "Innovation checks failed in {} samples",
                e.innovation_fail_samples
            ),
            warn(),
        ),
        Some(_) => ("No faults logged".into(), fg()),
    };
    facts(
        vec![
            fact("duration", "Duration", duration(m.duration_s), fg()),
            fact(
                "start",
                "Start (UTC)",
                m.utc_start_us
                    .map_or("No GPS time logged".into(), |us| ulog::utc(us / 1_000_000)),
                fg(),
            ),
            fact(
                "altitude",
                "Max altitude",
                match (m.max_alt_rel_m, m.max_alt_amsl_m) {
                    (Some(r), Some(a)) => format!("{r:.1} m / {a:.0} m AMSL"),
                    (Some(r), None) => format!("{r:.1} m above origin"),
                    (None, a) => opt(a, |a| format!("{a:.0} m AMSL")),
                },
                fg(),
            ),
            fact(
                "distance",
                "Distance",
                opt(m.distance_m, |d| format!("{d:.0} m")),
                fg(),
            ),
            fact(
                "speed",
                "Max ground speed",
                opt(m.max_speed_ms, |v| format!("{v:.1} m/s")),
                fg(),
            ),
            fact("battery", "Battery minimum", battery, fg()),
            fact("armed", "Armed", armed, fg()),
            fact("failsafes", "Failsafes", failsafe, failsafe_tone),
            fact(
                "messages",
                "Warnings / errors",
                format!("{} / {}", m.warnings, m.errors),
                if m.errors > 0 {
                    danger()
                } else if m.warnings > 0 {
                    warn()
                } else {
                    fg()
                },
            ),
            fact(
                "dropouts",
                "Dropouts",
                if m.dropouts == 0 {
                    "None".into()
                } else {
                    format!("{} · {} ms", m.dropouts, m.dropout_ms)
                },
                if m.dropouts > 0 { warn() } else { fg() },
            ),
            fact("estimator", "Estimator", estimator.0, estimator.1),
            fact(
                "contents",
                "Contents",
                format!(
                    "ULog v{} · {} topics · {} parameters",
                    m.format_version, m.topics, m.params
                ),
                fg(),
            ),
        ],
        compact,
    )
}

fn issues(m: &Summary) -> Option<El> {
    let i = &m.issues;
    let mut lines = Vec::new();
    if i.truncated {
        lines.push("The file ends mid-message (truncated, e.g. power loss or a copy in progress). Figures cover what was read.".to_string());
    }
    if i.corrupt_regions > 0 {
        lines.push(format!(
            "Skipped {} damaged region(s), {} bytes, to the next sync marker.",
            i.corrupt_regions, i.skipped_bytes
        ));
    }
    if i.unknown_messages > 0 {
        lines.push(format!(
            "Skipped {} message(s) of a type this reader does not know (a newer ULog revision).",
            i.unknown_messages
        ));
    }
    if !i.unresolved.is_empty() {
        lines.push(format!("No format for: {}.", i.unresolved.join(", ")));
    }
    if lines.is_empty() {
        return None;
    }
    let mut c = col().key("log-issues").w_full().min_w(0.).gap(4.);
    for l in lines {
        c = c.child(body(l, 11., warn()));
    }
    Some(c)
}

fn messages(rows: Vec<(String, u8, String)>, total: usize) -> El {
    let mut c = col().key("log-messages").w_full().min_w(0.).gap(4.);
    if rows.is_empty() {
        return c.child(p("No warnings or errors were logged.", 11.));
    }
    let shown = rows.len().min(20);
    for (t, level, text) in rows.into_iter().take(shown) {
        let (name, tone) = match level {
            0..=3 => ("ERROR", danger()),
            4 => ("WARN", warn()),
            _ => ("INFO", muted()),
        };
        c = c.child(code(format!("{t:>9}  {name:<5} {text}"), 11., tone).wrap_anywhere());
    }
    if total > shown {
        c = c.child(meta(format!("{} more in the log", total - shown)));
    }
    c
}

fn ulog_detail(m: &Summary, plots: &[ulog::Plot], vp: &Viewport) -> El {
    let mut c = col().w_full().min_w(0.).gap(16.);
    if !m.vehicle.is_empty() {
        let v: Vec<String> = m.vehicle.iter().map(|(k, v)| format!("{k} {v}")).collect();
        c = c.child(meta(v.join(" / ")).wrap());
    }
    c = c.child(ulog_facts(m, vp.compact()));
    if let Some(i) = issues(m) {
        c = c.child(i);
    }
    let armed = m.armed.clone().unwrap_or_default();
    let marks = m.failsafes.clone().unwrap_or_default();
    if plots.is_empty() {
        c = c.child(p(
            "None of the plotted topics (local position, battery, attitude, vibration) were logged.",
            11.,
        ));
    } else {
        let two = !vp.compact() && vp.content() >= 900.;
        let mut grid = col().key("log-plots").w_full().min_w(0.).gap(28.).pt(8.);
        for pair in plots.chunks(if two { 2 } else { 1 }) {
            let mut r = row().w_full().min_w(0.).gap(32.).items_start();
            for p in pair {
                r = r.child(col().grow(1.).min_w(0.).child(plot(
                    p,
                    m.duration_s as f32,
                    &armed,
                    &marks,
                )));
            }
            if two && pair.len() == 1 {
                r = r.child(col().grow(1.).min_w(0.));
            }
            grid = grid.child(r);
        }
        c = c
            .child(grid)
            .child(meta("Shaded: armed / red line: failsafe on").wrap());
    }
    let first = m.first_us;
    let rows = m
        .messages
        .iter()
        .map(|x| {
            let t = x.timestamp.saturating_sub(first) as f64 / 1e6;
            (format!("{t:.1} s"), x.level, x.text.clone())
        })
        .collect();
    c.child(h3("Logged warnings and errors", 13.))
        .child(messages(rows, m.warnings + m.errors))
}

fn tlog_detail(t: &TlogSummary, compact: bool) -> El {
    /// Components listed before the rest are summed up.
    const COMPONENTS: usize = 8;
    let mut comps: Vec<String> = t
        .components
        .iter()
        .take(COMPONENTS)
        .map(|((s, c), n)| format!("{s}:{c} ×{n}"))
        .collect();
    if t.components.len() > COMPONENTS {
        comps.push(format!("{} more", t.components.len() - COMPONENTS));
    }
    let none = || "Not logged".to_string();
    let mut items = vec![
        fact("duration", "Duration", duration(t.duration_s()), fg()),
        fact(
            "start",
            "Start (UTC)",
            if t.frames > 0 {
                ulog::utc(t.start_us / 1_000_000)
            } else {
                "Unknown".into()
            },
            fg(),
        ),
        fact(
            "vehicle",
            "Vehicle",
            t.vehicle
                .as_ref()
                .map_or("No autopilot heartbeat".into(), |(sys, ap, kind)| {
                    format!("{ap} {kind} / system {sys}")
                }),
            fg(),
        ),
        fact(
            "packets",
            "Packets",
            format!("{} (v1 {} / v2 {})", t.frames, t.v1, t.v2),
            fg(),
        ),
        fact("heartbeats", "Heartbeats", t.heartbeats().to_string(), fg()),
        fact(
            "types",
            "Message types",
            if t.unknown_frames > 0 {
                format!(
                    "{} / {} frames outside the dialect",
                    t.message_counts.len(),
                    t.unknown_frames
                )
            } else {
                t.message_counts.len().to_string()
            },
            fg(),
        ),
        fact(
            "components",
            "System:component",
            if comps.is_empty() {
                "None".into()
            } else {
                comps.join(", ")
            },
            fg(),
        ),
        fact("armed", "Armed", duration(t.armed_seconds), fg()),
        fact(
            "altitude",
            "Highest above home",
            t.max_relative_alt
                .map_or_else(none, |a| format!("{a:.1} m")),
            fg(),
        ),
        fact(
            "battery",
            "Lowest battery",
            t.min_battery_voltage
                .map_or_else(none, |v| format!("{v:.2} V")),
            fg(),
        ),
        fact(
            "modes",
            "Modes",
            match t.modes.len() {
                0 => none(),
                n if n > 12 => format!("{} → … {} more", t.modes[..12].join(" → "), n - 12),
                _ => t.modes.join(" → "),
            },
            fg(),
        ),
    ];
    if t.damaged() {
        let mut parts = Vec::new();
        if t.truncated {
            parts.push("Truncated".to_string());
        }
        if t.bad_frames > 0 {
            parts.push(format!("{} frames failed the CRC check", t.bad_frames));
        }
        if t.skipped_bytes > 0 {
            parts.push(format!("{} bytes skipped", t.skipped_bytes));
        }
        items.push(fact("damage", "Damage", parts.join(" · "), warn()));
    }
    let rows = t
        .status
        .iter()
        .map(|(sev, text)| (String::new(), *sev, text.clone()))
        .collect();
    let problems = t.status.iter().filter(|(s, _)| *s <= 4).count();
    col()
        .w_full()
        .min_w(0.)
        .gap(16.)
        .child(facts(items, compact))
        .child(p(
            "Read with the same MAVLink decoder as the live link; frames that fail their CRC check are not counted. Flight Review and QGroundControl read tlogs in full.",
            11.,
        ))
        .child(h3("Status text", 13.))
        .child(messages(rows, problems.max(t.status.len())))
}

fn detail(s: &State, f: &LogFile, a: Option<&Analysis>, vp: &Viewport) -> El {
    let path: PathBuf = f.path.clone();
    let review = path.clone();
    let mut actions = row().gaps(4., 20.).flex_wrap().center_y();
    if !f.is_tlog() {
        actions = actions.child(ghost(
            "flight-review",
            "Open in PX4 Flight Review ↗",
            !s.headless,
            move |s: &mut State| ulog::flight_review(s, &review),
        ));
    }
    actions = actions.child(ghost(
        "show-in-folder",
        "Show in folder ↗",
        !s.headless,
        move |s: &mut State| ulog::show_in_folder(s, &path),
    ));
    let body = match a {
        Some(Analysis::Ready(r)) => match r.as_ref() {
            Report::Ulog { summary, plots } => ulog_detail(summary, plots, vp),
            Report::Tlog(t) => tlog_detail(t, vp.compact()),
        },
        Some(Analysis::Pending) => notice(format!("Reading {} on a background thread…", f.name())),
        Some(Analysis::Failed(e)) => body(e.clone(), 11., danger()),
        None => notice("Select the log again to read it."),
    };
    let source_note = match &f.source {
        Source::Simulation(_) => "Simulation log from PX4 SITL inside WSL.",
        Source::Vehicle => "Downloaded from a vehicle.",
        Source::Telemetry => "MAVLink telemetry capture.",
    };
    col()
        .key("flight-log-detail")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .pt(8.)
        .pb(28.)
        .pl(if vp.compact() { 0. } else { 124. })
        .child(meta(source_note).wrap())
        .child(actions)
        .child(body)
}

pub(super) fn tab(s: &State, vp: &Viewport) -> El {
    col()
        .key("flight-logs")
        .w_full()
        .min_w(0.)
        .child(asset(intro(s), sources(s), vp.compact()))
        .child(library(s, vp))
}

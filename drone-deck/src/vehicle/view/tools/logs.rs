//! Onboard logs: list, download one at a time (progress, resume, cancel), and open the
//! downloaded file in Data & models → Flight logs. Nothing is erased on the vehicle.
use super::{bar, blocked, mine, percent};
use crate::State;
use crate::hardware::{self, size};
use crate::mavlink::LogEntry;
use crate::ui::{kit::*, theme::*};
use rune::{El, col, el, row};

/// Rows drawn at most; a longer list says how many it leaves out.
const SHOWN: usize = 200;

fn date(e: &LogEntry) -> String {
    if e.time_utc == 0 {
        "No vehicle clock".into()
    } else {
        crate::ulog::utc(e.time_utc as u64)
    }
}

fn entry(e: &LogEntry, progress: Option<(u16, u32, u32)>, ready: bool, compact: bool) -> El {
    let id = e.id;
    let pull = match progress {
        Some((pid, received, total)) if pid == id => col()
            .w_full()
            .min_w(0.)
            .gap(6.)
            .child(meta_sized(
                format!("{}%", percent(received as u64, total as u64)),
                9.,
                fg(),
            ))
            .child(bar(&format!("log-bar-{id}"), received as u64, total as u64)),
        _ => col().child(ghost(
            format!("log-download-{id}"),
            "Download",
            ready && progress.is_none(),
            move |s| hardware::download(s, id),
        )),
    };
    log_row(
        format!("log-entry-{id}"),
        meta(format!("Log {id}")),
        row_text(date(e), size(e.size as u64)),
        pull,
        compact,
    )
}

fn downloading(s: &State) -> El {
    let Some((id, received, total)) = s.vehicle.log_progress.filter(|_| mine(s)) else {
        return el();
    };
    col()
        .key("logs-progress")
        .w_full()
        .min_w(0.)
        .gap(8.)
        .role("status")
        .child(
            row()
                .w_full()
                .min_w(0.)
                .gaps(4., 20.)
                .center_y()
                .flex_wrap()
                .child(
                    meta_sized(
                        format!(
                            "Downloading log {id}: {} / {} ({}%)",
                            size(received as u64),
                            size(total as u64),
                            percent(received as u64, total as u64)
                        ),
                        9.,
                        fg(),
                    )
                    .wrap(),
                )
                .child(ghost(
                    "logs-cancel",
                    "Cancel download",
                    true,
                    hardware::cancel,
                )),
        )
        .child(bar("logs-bar", received as u64, total as u64))
}

fn downloaded(s: &State) -> El {
    let Some(path) = s.vehicle.downloaded.clone().filter(|_| mine(s)) else {
        return el();
    };
    let (open, show) = (path.clone(), path.clone());
    // Flight logs lists ULog files only; an ArduPilot DataFlash `.bin` is reached by its folder.
    let ulog = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ulg"));
    ruled(false).key("logs-downloaded").py(12.).child(
        col()
            .w_full()
            .min_w(0.)
            .gap(6.)
            .child(meta_sized("Last download", 9., muted()))
            .child(code(path.display().to_string(), 10., fg()).wrap_anywhere())
            .child(
                row()
                    .gaps(4., 20.)
                    .center_y()
                    .flex_wrap()
                    .child(if ulog {
                        ghost(
                            "logs-open-flight",
                            "Open in Flight logs →",
                            true,
                            move |s| hardware::open_in_flight_logs(s, &open),
                        )
                    } else {
                        el()
                    })
                    .child(ghost(
                        "logs-show-folder",
                        "Show in folder ↗",
                        true,
                        move |s| crate::ulog::show_in_folder(s, &show),
                    )),
            ),
    )
}

pub(super) fn section(s: &State, vp: &Viewport) -> El {
    let compact = vp.compact();
    let v = &s.vehicle;
    let ready = blocked(s).is_none();
    let logs: &[LogEntry] = if mine(s) { &v.logs } else { &[] };
    let listing = v.listing && mine(s);
    let progress = v.log_progress.filter(|_| mine(s));
    let state = if listing {
        "Listing…".to_string()
    } else if logs.is_empty() {
        match blocked(s) {
            Some(why) => format!("Not listed. {why}"),
            None => "Not listed".into(),
        }
    } else {
        let mut text = if v.logs_complete {
            format!("{} log(s) on the vehicle", logs.len())
        } else {
            format!(
                "{} log(s); the vehicle stopped answering before the list was complete",
                logs.len()
            )
        };
        if let Some(why) = blocked(s) {
            text.push_str(&format!(" / from the last list. {why}"));
        }
        text
    };
    let actions = row()
        .gaps(8., 20.)
        .center_y()
        .flex_wrap()
        .child(pill(
            "logs-list",
            if logs.is_empty() {
                "List logs"
            } else {
                "List again"
            },
            ready && !listing && progress.is_none(),
            hardware::list_logs,
        ))
        .child(
            meta_sized(state, 9., fg())
                .key("logs-state")
                .role("status")
                .wrap(),
        );
    let mut block = col()
        .key("vehicle-logs")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(section_head(
            "Onboard logs",
            meta("Read-only / list and download, nothing erased").wrap(),
            compact,
        ))
        .child(actions)
        .child(p(
            "One download at a time; a partial download resumes from its .part file. Downloaded logs appear in Data & models → Flight logs. Saved to:",
            11.,
        ))
        .child(code(crate::vehicle::log_dir().display().to_string(), 10., muted()).wrap_anywhere())
        .child(downloading(s))
        .child(downloaded(s));
    if logs.is_empty() {
        return block;
    }
    let mut list = col().key("logs-list-rows").w_full().min_w(0.);
    for e in logs.iter().take(SHOWN) {
        list = list.child(entry(e, progress, ready, compact));
    }
    if logs.len() > SHOWN {
        list = list.child(notice(format!(
            "{} more not shown. Download them from QGroundControl or MAVProxy.",
            logs.len() - SHOWN
        )));
    }
    block = block.child(list);
    block
}

use super::*;
use rune::{Env, Runtime};

fn runtime(w: f32, h: f32) -> Runtime<State> {
    Runtime::new(
        State::new(),
        crate::view,
        (w, h),
        Env {
            scale: 1.,
            system_dark: false,
        },
    )
}

#[test]
fn flight_logs_tab_shows_an_honest_empty_state_in_headless_mode() {
    let mut rt = runtime(1216., 800.);
    rt.update(|s, _| {
        s.ui.data_tab = LOGS_TAB;
        s.navigate(crate::ui::Page::Data)
    });
    rt.frame(0.1);
    assert!(rt.find("data-tab-4").is_some());
    assert!(rt.find("flight-logs-sources").is_some());
    assert!(rt.find("flight-logs-empty").is_some());
    assert!(rt.find("flight-log-detail").is_none());
    refresh(&mut rt.state);
    assert!(!rt.state.flight_logs.scanning, "headless never scans");
    assert!(rt.state.flight_logs.files.is_empty());
}

#[test]
fn preview_lists_logs_and_selecting_shows_summary_plots_and_actions() {
    for (w, h) in [(1440., 900.), (390., 844.), (360., 640.)] {
        let mut rt = runtime(w, h);
        rt.update(|s, _| preview(s));
        rt.frame(0.1);
        for key in [
            "flight-log-0",
            "flight-log-1",
            "flight-log-2",
            "flight-log-detail",
            "flight-review",
            "show-in-folder",
            "summary-duration",
            "summary-failsafes",
            "plot-altitude",
            "plot-battery",
            "plot-attitude",
            "plot-vibration",
            "log-messages",
        ] {
            assert!(rt.find(key).is_some(), "{key} at {w}x{h}");
        }
        assert!(
            rt.find("log-issues").is_none(),
            "the complete fixture has no issues"
        );
        tap(&mut rt, "flight-log-1");
        rt.frame(0.1);
        assert!(rt.find("log-issues").is_some(), "truncated copy reports it");
        tap(&mut rt, "flight-log-2");
        rt.frame(0.1);
        assert!(rt.find("summary-packets").is_some());
        assert!(
            rt.find("flight-review").is_none(),
            "tlogs are not sent to Flight Review"
        );
        tap(&mut rt, "show-in-folder");
        assert!(rt.state.notice.contains("Headless"));
    }
}

#[test]
fn selecting_an_unread_file_in_headless_mode_says_it_is_not_read() {
    let mut s = State::new();
    s.flight_logs.files.push(LogFile {
        path: "x/flight.ulg".into(),
        source: Source::Vehicle,
        size: 10,
        modified: None,
    });
    select(&mut s, std::path::Path::new("x/flight.ulg"));
    let f = s.flight_logs.selected_file().unwrap().clone();
    assert!(matches!(
        s.flight_logs.analysis(&f),
        Some(Analysis::Failed(_))
    ));
    assert!(s.flight_logs.worker.is_none(), "no thread in headless mode");
}

#[test]
fn cache_is_bounded_and_keeps_the_selection() {
    let mut logs = FlightLogs::default();
    logs.selected = Some("keep.ulg".into());
    logs.insert("keep.ulg".into(), (1, None), Analysis::Pending);
    for k in 0..CACHE_ENTRIES * 2 {
        logs.insert(format!("{k}.ulg").into(), (1, None), Analysis::Pending);
    }
    assert_eq!(logs.cache.len(), CACHE_ENTRIES);
    assert!(logs.cache.contains_key(std::path::Path::new("keep.ulg")));
    let stale = LogFile {
        path: "keep.ulg".into(),
        source: Source::Vehicle,
        size: 2,
        modified: None,
    };
    assert!(
        logs.analysis(&stale).is_none(),
        "a changed file is read again"
    );
}

fn tap(rt: &mut Runtime<State>, key: &str) {
    let target = key.to_string();
    rt.update(move |_, tx| tx.scroll_into_view(&target));
    for _ in 0..10 {
        rt.frame(0.1);
    }
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("{key} is not on screen"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
}

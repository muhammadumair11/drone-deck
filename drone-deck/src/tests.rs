use super::*;
use rune::{Env, Runtime};

fn runtime(state: State, size: (f32, f32)) -> Runtime<State> {
    let mut rt = Runtime::new(
        state,
        view,
        size,
        Env {
            system_dark: true,
            scale: 1.,
        },
    );
    rt.frame(0.);
    rt
}
fn settle(rt: &mut Runtime<State>) {
    for _ in 0..10 {
        rt.frame(0.1);
    }
}
fn tap(rt: &mut Runtime<State>, key: &str) {
    let target = key.to_string();
    rt.update(move |_, tx| tx.scroll_into_view(&target));
    settle(rt);
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("{key} is not on screen"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
}
fn escape(rt: &mut Runtime<State>) {
    rt.key(rune::KeyEvent {
        key: rune::Key::Escape,
        mods: rune::Mods::default(),
    });
    rt.frame(0.2);
}
fn assert_inside(rt: &Runtime<State>, key: &str, w: f32, h: f32, context: &str) {
    let (r, _) = rt
        .presented(key)
        .unwrap_or_else(|| panic!("Missing {key} ({context})"));
    assert!(
        r[0] >= -1. && r[1] >= -1. && r[0] + r[2] <= w + 1. && r[1] + r[3] <= h + 1.,
        "{key} overflows {w}x{h} ({context}): {r:?}"
    );
}
const PAGES: [Page; 6] = [
    Page::System,
    Page::Tools,
    Page::Data,
    Page::Activity,
    Page::Settings,
    Page::Setup,
];

#[test]
fn layout_reflows_across_reference_sizes_and_live_resizing() {
    let mut rt = runtime(State::new(), (1440., 900.));
    for (w, h) in [
        (1920., 1080.),
        (1440., 900.),
        (1366., 768.),
        (1024., 768.),
        (820., 1180.),
        (600., 960.),
        (430., 932.),
        (390., 844.),
        (360., 800.),
        (640., 360.),
        (1440., 900.),
    ] {
        rt.set_viewport((w, h), 1.);
        for kind in 0..4 {
            rt.update(|s, _| {
                s.select_system(kind);
            });
            for page in PAGES {
                rt.update(|s, _| s.navigate(page));
                settle(&mut rt);
                let context = format!("{page:?}, system {kind}");
                for key in [
                    "app-root",
                    "app-stage",
                    "topbar",
                    "main-scroll",
                    "statusbar",
                ] {
                    assert_inside(&rt, key, w, h, &context);
                }
                if page != Page::Setup {
                    assert_eq!(
                        rt.find("open-menu").is_some(),
                        w / (rt.state.ui.zoom_percent as f32 / 100.) <= 720.,
                        "Navigation breakpoint at {w} ({context})"
                    );
                }
            }
        }
    }
}

#[test]
fn zoom_reflows_without_overflow_and_controls_remain_clickable() {
    let mut rt = runtime(State::new(), (1216., 576.));
    for (w, h) in [(1216., 576.), (390., 844.), (360., 320.)] {
        rt.set_viewport((w, h), 1.);
        for percent in [50, 75, 85, 100, 125, 150] {
            rt.update(|s, _| s.ui.zoom_percent = percent);
            settle(&mut rt);
            for key in [
                "window-frame",
                "app-root",
                "topbar",
                "statusbar",
                "zoom-out",
                "zoom-reset",
                "zoom-in",
            ] {
                assert_inside(&rt, key, w, h, &format!("{percent}%"));
            }
            tap(&mut rt, "zoom-reset");
            assert_eq!(rt.state.ui.zoom_percent, 100);
        }
    }
}

#[test]
fn settings_zoom_buttons_and_keyboard_shortcuts_work() {
    let mut rt = runtime(State::new(), (1216., 700.));
    tap(&mut rt, "open-settings");
    assert_eq!(rt.state.page, Page::Settings);
    assert_eq!(rt.state.ui.zoom_percent, 100, "the redesign opens at 100%");
    tap(&mut rt, "settings-zoom-out");
    assert_eq!(rt.state.ui.zoom_percent, 95);
    let event = |c| rune::KeyEvent {
        key: rune::Key::Command(c),
        mods: rune::Mods {
            ctrl: true,
            cmd: true,
            ..Default::default()
        },
    };
    rt.update(|s, tx| assert!(ui::zoom_shortcuts(s, &event('='), tx)));
    assert_eq!(rt.state.ui.zoom_percent, 100);
    rt.update(|s, tx| {
        for _ in 0..100 {
            ui::zoom_shortcuts(s, &event('+'), tx);
        }
    });
    assert_eq!(rt.state.ui.zoom_percent, 150);
    rt.update(|s, tx| {
        for _ in 0..100 {
            ui::zoom_shortcuts(s, &event('-'), tx);
        }
    });
    assert_eq!(rt.state.ui.zoom_percent, 50);
    rt.update(|s, tx| {
        ui::zoom_shortcuts(s, &event('0'), tx);
    });
    assert_eq!(rt.state.ui.zoom_percent, 100);
    rt.update(|s, tx| {
        s.navigate(Page::System);
        ui::zoom_shortcuts(s, &event(','), tx);
    });
    assert_eq!(rt.state.page, Page::Settings);
}

#[test]
fn compact_navigation_and_tool_search_work() {
    let mut rt = runtime(State::new(), (390., 844.));
    tap(&mut rt, "open-menu");
    assert!(rt.state.ui.menu);
    escape(&mut rt);
    assert!(!rt.state.ui.menu, "Escape closes the navigation drawer");
    tap(&mut rt, "open-menu");
    tap(&mut rt, "rail-tools");
    assert_eq!(rt.state.page, Page::Tools);
    assert!(!rt.state.ui.menu);
    rt.update(|s, _| {
        s.ui.tools_all = true;
        s.ui.search = TextEdit::new("Gazebo");
    });
    settle(&mut rt);
    assert!(rt.find("tool-gazebo").is_some());
    assert!(rt.find("tool-ardupilot").is_none());
}

#[test]
fn startup_window_fits_the_work_area() {
    for work in [(1366., 728.), (1093., 575.), (1920., 1040.)] {
        let (w, h) = window::fit_size(work);
        assert!(w < work.0 && h < work.1);
    }
}

#[test]
fn confirmation_stays_visible_in_short_windows_and_escape_dismisses_it() {
    let mut s = State::new();
    s.confirm = Some("stop".into());
    let mut rt = runtime(s, (360., 320.));
    let (r, _) = rt.presented("confirmation").unwrap();
    assert!(r[0] >= 0. && r[1] >= 0. && r[0] + r[2] <= 360. && r[1] + r[3] <= 320.);
    let (x, y) = rt.find("confirm-action").unwrap();
    assert!((0.0..=360.).contains(&x) && (0.0..=320.).contains(&y));
    escape(&mut rt);
    assert!(rt.state.confirm.is_none());
}

#[cfg(windows)]
#[test]
fn process_manager_captures_real_output_and_exit_status() {
    let b = backend::spawn();
    let mut profile = backend::defaults().profiles.remove(0);
    profile.id = "manager-test".into();
    profile.program = std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into());
    profile.args = vec![
        "/D".into(),
        "/C".into(),
        "echo DRONE_DECK_MANAGER_OK".into(),
    ];
    profile.cwd.clear();
    profile.exclusive = false;
    b.tx.send(Request::Start(profile, "Ubuntu-24.04".into()))
        .unwrap();
    let mut saw_output = false;
    let mut saw_exit = false;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !(saw_output && saw_exit) {
        if let Ok(event) = b.rx.recv_timeout(Duration::from_millis(100)) {
            match event {
                Event::Log(_, line) => saw_output |= line.contains("DRONE_DECK_MANAGER_OK"),
                Event::Status(_, status) => saw_exit |= status == "Exited 0",
                _ => {}
            }
        }
    }
    b.shutdown();
    assert!(
        saw_output && saw_exit,
        "process output and lifecycle must both arrive"
    );
}

#[test]
fn rail_navigation_and_profile_registry_are_interactive() {
    let mut rt = runtime(State::new(), (1440., 960.));
    for (page, slug, _, _) in ui::RAIL {
        tap(&mut rt, &format!("rail-{slug}"));
        assert_eq!(rt.state.page, page);
        assert!(rt.find("page-title").is_some(), "{slug} has a page title");
    }
    tap(&mut rt, "open-settings");
    assert_eq!(rt.state.page, Page::Settings);
    tap(&mut rt, "edit-profiles");
    assert!(rt.state.editing);
    rt.state.editor = TextEdit::new("invalid JSON");
    rt.state.apply();
    assert!(rt.state.notice.starts_with("JSON error"));
    rt.state.editor = TextEdit::new(&serde_json::to_string(&backend::defaults()).unwrap());
    rt.state.apply();
    assert!(!rt.state.editing);
}

#[test]
fn preview_never_starts_processes() {
    let mut s = State::new();
    s.headless = true;
    s.bundle(systems::SESSIONS[systems::SIMULATION]);
    assert!(s.backend.is_none());
    assert!(s.status.is_empty());
}

#[test]
fn integration_form_validates_arguments_and_persists_in_state() {
    let mut state = State::new();
    profile_editor::open(&mut state);
    assert_eq!(state.page, Page::Tools);
    let mut rt = runtime(state, (1440., 960.));
    rt.state.form.as_mut().unwrap().fields[0] = TextEdit::new("");
    tap(&mut rt, "save-form");
    assert!(rt.state.notice.starts_with("Name, program"));
    rt.state.form.as_mut().unwrap().fields[0] = TextEdit::new("Configured tool");
    rt.state.form.as_mut().unwrap().fields[2] = TextEdit::new("echo configured");
    rt.frame(0.1);
    tap(&mut rt, "save-form");
    assert!(rt.state.form.is_none());
    assert_eq!(rt.state.config.profiles[0].args, vec!["echo configured"]);
}

#[test]
fn active_system_selector_switches_context_and_refuses_while_running() {
    let mut rt = runtime(State::new(), (1440., 900.));
    tap(&mut rt, "system-select");
    assert!(rt.state.ui.system_menu);
    tap(&mut rt, "system-option-3");
    assert_eq!(rt.state.systems.active, systems::PHYSICAL);
    assert!(!rt.state.ui.system_menu);
    assert_eq!(
        rt.state.hardware.mode, 1,
        "physical system uses native monitoring"
    );
    tap(&mut rt, "system-select");
    escape(&mut rt);
    assert!(!rt.state.ui.system_menu, "Escape closes the system list");

    rt.update(|s, _| {
        s.select_system(systems::SIMULATION);
        s.status.insert("sih".into(), "Running".into());
        s.select_system(systems::ROS);
    });
    assert_eq!(rt.state.systems.active, systems::SIMULATION);
    assert!(rt.state.notice.starts_with("Stop the active session"));
    rt.update(|s, _| {
        s.status.insert("sih".into(), "Exited 0".into());
        s.recorder.recording = true;
        s.select_system(systems::AI);
    });
    assert_eq!(
        rt.state.systems.active,
        systems::SIMULATION,
        "capture blocks switching"
    );
    rt.update(|s, _| {
        s.recorder.recording = false;
        s.select_system(systems::AI);
    });
    assert_eq!(rt.state.systems.active, systems::AI);
}

#[test]
fn guided_setup_validates_the_name_and_returns_to_the_workspace() {
    let mut rt = runtime(State::new(), (1440., 900.));
    tap(&mut rt, "open-guided-setup");
    assert_eq!(rt.state.page, Page::Setup);
    assert_eq!(rt.state.guided.chapter, 0);
    tap(&mut rt, "intent-3");
    assert_eq!(rt.state.guided.choice, systems::PHYSICAL);
    tap(&mut rt, "next");
    assert_eq!(rt.state.guided.chapter, 1);
    rt.update(|s, _| s.guided.name = TextEdit::new("   "));
    rt.frame(0.1);
    tap(&mut rt, "next");
    assert_eq!(rt.state.guided.chapter, 1, "a blank name does not advance");
    assert!(!rt.state.guided.name_error.is_empty());
    rt.update(|s, _| s.guided.name = TextEdit::new("Bench quad"));
    rt.frame(0.1);
    tap(&mut rt, "next");
    assert_eq!(rt.state.guided.chapter, 2);
    tap(&mut rt, "chapter-nav-0");
    assert_eq!(
        rt.state.guided.chapter, 0,
        "earlier chapters stay reachable"
    );
    tap(&mut rt, "back-to-management");
    assert_eq!(rt.state.page, Page::System);
    assert!(rt.state.setup.jobs.is_empty() && !rt.state.setup.pending);
}

#[test]
fn activity_records_actions_and_bounds_history() {
    let mut s = State::new();
    for i in 0..250 {
        activity::record(&mut s, format!("event {i}"));
    }
    assert_eq!(s.activity.entries.len(), 200);
    assert_eq!(s.activity.entries[0].text, "event 249");
    assert_eq!(s.activity.entries[0].system, "Local simulation");
}

#[test]
fn component_states_never_promote_process_state_to_readiness() {
    let mut s = State::new();
    assert_eq!(systems::profile_state(&s, "sih").0, "Not checked");
    s.detected.insert("sih".into(), true);
    assert_eq!(systems::profile_state(&s, "sih").0, "Path found");
    s.status.insert("sih".into(), "Running".into());
    assert_eq!(systems::profile_state(&s, "sih").0, "Running");
    s.readiness.insert("sih".into(), "Ready".into());
    assert_eq!(systems::profile_state(&s, "sih").0, "Ready");
    s.status.insert("telemetry".into(), "Running".into());
    assert_eq!(
        systems::profile_state(&s, "telemetry").0,
        "Running / no data"
    );
    s.metrics.insert("link".into(), "Stale".into());
    assert_eq!(
        systems::profile_state(&s, "telemetry").1,
        systems::Tone::Warn
    );
    assert_eq!(systems::profile_state(&s, "missing").0, "Not configured");
}

#[test]
fn recipe_only_tools_are_listed_under_all_registered_tools() {
    let mut s = State::new();
    s.navigate(Page::Tools);
    assert!(!s.config.profiles.iter().any(|p| p.id == "onnx"));
    let mut rt = runtime(s, (1440., 900.));
    assert!(
        rt.find("tool-onnx").is_none(),
        "active simulation system has no ONNX runtime"
    );
    rt.update(|s, _| s.ui.tools_all = true);
    settle(&mut rt);
    assert!(rt.find("tool-onnx").is_some());
    assert!(rt.find("tool-ros-workspace").is_some());
    assert!(
        rt.find("start-onnx").is_none(),
        "nothing to start before install/adopt"
    );
    rt.update(|s, _| s.ui.tools_filter = 6);
    settle(&mut rt);
    assert!(rt.find("tool-onnx").is_some());
    assert!(
        rt.find("tool-gazebo").is_none(),
        "AI filter hides simulators"
    );
    tap(&mut rt, "configure-onnx");
    assert_eq!(rt.state.ui.expanded.as_deref(), Some("onnx"));
    tap(&mut rt, "setup-review-onnx");
    assert!(rt.state.setup.review);
    assert_eq!(rt.state.setup.recipes[rt.state.setup.selected].id, "onnx");
}

#[test]
fn existing_installation_can_be_detected_and_adopted_from_a_tool_row() {
    let mut s = State::new();
    s.navigate(Page::Tools);
    s.ui.tools_all = true;
    let mut rt = runtime(s, (1440., 900.));
    tap(&mut rt, "configure-onnx");
    for key in [
        "setup-existing-location",
        "browse-existing-tool",
        "detect-selected-tool",
    ] {
        assert!(rt.find(key).is_some(), "{key} renders for a recipe row");
    }
    assert!(
        rt.find("adopt-tool").is_none(),
        "no result before detection"
    );
    tap(&mut rt, "detect-selected-tool");
    assert!(
        rt.state.notice.starts_with("Preview"),
        "headless refuses detection"
    );
    let onnx = rt.state.setup.recipe_index("onnx").unwrap();
    assert_eq!(rt.state.setup.selected, onnx);
    rt.update(|s, _| {
        let distro = s.setup.distro.text.clone();
        s.setup.candidate = Some(setup::Candidate {
            tool: "onnx".into(),
            found: true,
            compatible: true,
            path: "/home/pilot/.local/share/drone-deck/tools/onnx".into(),
            executable: "/home/pilot/.local/share/drone-deck/tools/onnx/bin/python".into(),
            version: "1.23.2".into(),
            reason: "Isolated CPU runtime verified".into(),
            environment: distro,
            ..Default::default()
        });
    });
    settle(&mut rt);
    assert!(rt.find("tool-candidate").is_some());
    tap(&mut rt, "adopt-tool");
    assert_eq!(rt.state.setup.last_applied.as_deref(), Some("onnx"));
    assert!(rt.state.config.profiles.iter().any(|p| p.id == "onnx"));
    assert!(
        rt.state.activity.entries[0]
            .text
            .starts_with("Applied detected configuration"),
        "adoption is recorded"
    );
    settle(&mut rt);
    assert!(rt.find("tool-onnx").is_some());
    assert!(rt.find("start-onnx").is_some(), "adopted tool can start");
}

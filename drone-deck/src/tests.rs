use super::*;
use rune::{Env, Runtime};

#[test]
fn layout_reflows_across_reference_sizes_and_live_resizing() {
    let mut rt = Runtime::new(
        State::new(),
        view,
        (1440., 900.),
        Env {
            system_dark: false,
            scale: 1.,
        },
    );
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
        for page in 0..9 {
            rt.update(|s, _| s.navigate(page));
            for _ in 0..10 {
                rt.frame(0.1);
            }
            for key in [
                "app-root",
                "app-stage",
                "topbar",
                "main-scroll",
                "statusbar",
            ] {
                let (r, _) = rt.presented(key).unwrap_or_else(|| panic!("Missing {key}"));
                assert!(
                    r[0] >= -1. && r[1] >= -1. && r[0] + r[2] <= w + 1. && r[1] + r[3] <= h + 1.,
                    "{key} overflows {w}x{h}, page {page}: {r:?}"
                );
            }
            assert_eq!(
                rt.find("open-menu").is_some(),
                w / (rt.state.ui.zoom_percent as f32 / 100.) < 900.,
                "Navigation breakpoint at {w}"
            );
        }
    }
}

#[test]
fn zoom_reflows_without_overflow_and_controls_remain_clickable() {
    let mut rt = Runtime::new(
        State::new(),
        view,
        (1216., 576.),
        Env {
            system_dark: false,
            scale: 1.,
        },
    );
    for (w, h) in [(1216., 576.), (390., 844.), (360., 320.)] {
        rt.set_viewport((w, h), 1.);
        for percent in [50, 75, 85, 100, 125, 150] {
            rt.update(|s, _| s.ui.zoom_percent = percent);
            for _ in 0..10 {
                rt.frame(0.1);
            }
            for key in [
                "window-frame",
                "app-root",
                "topbar",
                "statusbar",
                "zoom-out",
                "zoom-reset",
                "zoom-in",
            ] {
                let (r, _) = rt.presented(key).unwrap();
                assert!(
                    r[0] >= -1. && r[1] >= -1. && r[0] + r[2] <= w + 1. && r[1] + r[3] <= h + 1.,
                    "{key} at {percent}% on {w}x{h}: {r:?}"
                );
            }
            let (x, y) = rt.find("zoom-reset").unwrap();
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(0.1);
            assert_eq!(rt.state.ui.zoom_percent, 100);
        }
    }
}

#[test]
fn settings_zoom_buttons_and_keyboard_shortcuts_work() {
    let mut rt = Runtime::new(
        State::new(),
        view,
        (1216., 700.),
        Env {
            system_dark: false,
            scale: 1.,
        },
    );
    rt.frame(0.);
    let (x, y) = rt.find("open-settings").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
    assert_eq!(rt.state.page, 6);
    let (x, y) = rt.find("settings-zoom-out").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
    assert_eq!(rt.state.ui.zoom_percent, 80);
    let event = |c| rune::KeyEvent {
        key: rune::Key::Command(c),
        mods: rune::Mods {
            ctrl: true,
            cmd: true,
            ..Default::default()
        },
    };
    rt.update(|s, tx| {
        assert!(ui::zoom_shortcuts(s, &event('='), tx));
    });
    assert_eq!(rt.state.ui.zoom_percent, 85);
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
        s.navigate(0);
        ui::zoom_shortcuts(s, &event(','), tx);
    });
    assert_eq!(rt.state.page, 6);
}

#[test]
fn compact_navigation_and_tool_search_work() {
    let mut rt = Runtime::new(
        State::new(),
        view,
        (390., 844.),
        Env {
            system_dark: false,
            scale: 1.,
        },
    );
    rt.frame(0.);
    let (x, y) = rt.find("open-menu").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
    assert!(rt.state.ui.menu);
    rt.key(rune::KeyEvent {
        key: rune::Key::Escape,
        mods: rune::Mods::default(),
    });
    rt.frame(0.2);
    assert!(!rt.state.ui.menu, "Escape closes the navigation drawer");
    let (x, y) = rt.find("open-menu").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
    let (x, y) = rt.find("nav-1").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
    assert_eq!(rt.state.page, 1);
    assert!(!rt.state.ui.menu);
    rt.update(|s, _| s.ui.search = TextEdit::new("Gazebo"));
    rt.frame(0.2);
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
    let mut rt = Runtime::new(
        s,
        view,
        (360., 320.),
        Env {
            system_dark: false,
            scale: 1.,
        },
    );
    rt.frame(0.);
    let (r, _) = rt.presented("confirmation").unwrap();
    assert!(r[0] >= 0. && r[1] >= 0. && r[0] + r[2] <= 360. && r[1] + r[3] <= 320.);
    let (x, y) = rt.find("confirm-action").unwrap();
    assert!((0.0..=360.).contains(&x) && (0.0..=320.).contains(&y));
    rt.key(rune::KeyEvent {
        key: rune::Key::Escape,
        mods: rune::Mods::default(),
    });
    rt.frame(0.2);
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
fn navigation_and_configuration_are_interactive() {
    let mut state = State::new();
    state.headless = true;
    let mut rt = Runtime::new(
        state,
        view,
        (1440., 960.),
        Env {
            system_dark: true,
            scale: 1.,
        },
    );
    rt.frame(0.);
    for page in 1..6 {
        let (x, y) = rt.find(&format!("nav-{page}")).unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.2);
        assert_eq!(rt.state.page, page);
    }
    let (x, y) = rt.find("edit-profiles").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
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
    s.bundle();
    assert!(s.backend.is_none());
    assert!(s.status.is_empty());
}

#[test]
fn integration_form_validates_arguments_and_persists_in_state() {
    let mut state = State::new();
    profile_editor::open(&mut state);
    let mut rt = Runtime::new(
        state,
        view,
        (1440., 960.),
        Env {
            system_dark: true,
            scale: 1.,
        },
    );
    rt.frame(0.);
    rt.state.form.as_mut().unwrap().fields[0] = TextEdit::new("");
    let (x, y) = rt.find("save-form").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
    assert!(rt.state.notice.starts_with("Name, program"));
    rt.state.form.as_mut().unwrap().fields[0] = TextEdit::new("Configured tool");
    rt.state.form.as_mut().unwrap().fields[2] = TextEdit::new("echo configured");
    let (x, y) = rt.find("save-form").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
    assert!(rt.state.form.is_none());
    assert_eq!(rt.state.config.profiles[0].args, vec!["echo configured"]);
}

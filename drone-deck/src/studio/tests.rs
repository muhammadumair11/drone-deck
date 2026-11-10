use super::*;
use rune::{Env, Runtime};
#[test]
fn studio_and_credentials_are_reachable_in_headless_ui_without_network() {
    let mut rt = Runtime::new(
        State::new(),
        crate::view,
        (1216., 800.),
        Env {
            scale: 1.,
            system_dark: false,
        },
    );
    rt.update(|s, _| {
        s.ui.data_tab = 1;
        s.navigate(crate::ui::Page::Data)
    });
    rt.frame(0.1);
    assert!(rt.find("model-repo").is_some());
    rt.update(|s, _| s.ui.data_tab = 2);
    rt.frame(0.1);
    assert!(rt.find("extract-frames").is_some());
    assert!(rt.find("infer-replay").is_some());
    assert!(rt.find("evaluation-csv").is_none());
    rt.update(|s, _| s.ui.data_tab = 3);
    rt.frame(0.1);
    assert!(rt.find("create-project").is_some());
    rt.update(|s, _| s.navigate(crate::ui::Page::Settings));
    rt.frame(0.1);
    assert!(rt.find("hf-token").is_some());
    request(&mut rt.state, "start_model");
    assert!(!rt.state.studio.pending);
}
#[test]
fn windows_models_map_to_selected_wsl_mount_without_shell_interpolation() {
    let linux = |p| linux_model_path(p, "Ubuntu", None);
    assert_eq!(
        linux("C:\\Users\\Example\\model cache").as_deref(),
        Ok("/mnt/c/Users/Example/model cache")
    );
    assert_eq!(linux("C:").as_deref(), Ok("C:"));
    assert_eq!(
        linux_model_path("C:\\m", "Ubuntu", Some("/drives/")).as_deref(),
        Ok("/drives/c/m")
    );
    assert_eq!(
        linux("\\\\wsl.localhost\\Ubuntu\\home\\me\\m").as_deref(),
        Ok("/home/me/m")
    );
    assert!(linux("\\\\wsl$\\Debian\\home\\me\\m").is_err());
}
#[test]
fn new_workspace_panels_fit_small_and_wide_windows() {
    let mut rt = Runtime::new(
        State::new(),
        crate::view,
        (1216., 800.),
        Env {
            scale: 1.,
            system_dark: false,
        },
    );
    for (w, h) in [(360., 800.), (640., 360.), (1216., 800.)] {
        rt.set_viewport((w, h), 1.);
        for variant in 0..5 {
            rt.update(|s, _| {
                if variant == 4 {
                    s.select_system(crate::systems::PHYSICAL);
                    s.navigate(crate::ui::Page::System);
                } else {
                    s.ui.data_tab = variant;
                    s.navigate(crate::ui::Page::Data);
                }
            });
            for _ in 0..10 {
                rt.frame(0.1);
            }
            for key in ["app-root", "main-scroll", "topbar", "statusbar"] {
                let (r, _) = rt.presented(key).unwrap();
                assert!(
                    r[0] >= -1. && r[0] + r[2] <= w + 1. && r[1] >= -1. && r[1] + r[3] <= h + 1.,
                    "{key} at {w}x{h}: {r:?}"
                );
            }
        }
    }
}
#[test]
fn mavsdk_projects_run_with_the_adopted_mavsdk_interpreter() {
    let mut s = State::new();
    // Nothing is spawned: without a backend, `send` drops the start request.
    s.headless = false;
    s.setup.host = Some(crate::setup::Host {
        distros: vec![crate::setup::Distro {
            name: s.config.distro.clone(),
            version: 2,
            ..Default::default()
        }],
        ..Default::default()
    });
    s.config.profiles.retain(|p| p.id != "mavsdk");
    let project = json!({
        "name": "monitor",
        "kind": "mavsdk",
        "path": "/home/pilot/.local/share/drone-deck/projects/monitor",
        "distro": s.config.distro,
    });
    projects::run(&mut s, project.clone());
    assert!(
        s.studio.message.contains("Install or adopt mavsdk first"),
        "{}",
        s.studio.message
    );
    // The command the setup adopt path writes for a MAVSDK environment.
    s.config.profiles.push(backend::Profile {
        id: "mavsdk".into(),
        name: "MAVSDK Python".into(),
        category: "Development".into(),
        description: String::new(),
        program: "wsl".into(),
        args: vec![
            "'/home/pilot/.local/share/drone-deck/tools/mavsdk/bin/python' -c 'import mavsdk; print(mavsdk.__file__)'".into(),
        ],
        cwd: String::new(),
        probe: String::new(),
        docs: String::new(),
        download_url: String::new(),
        exclusive: false,
    });
    projects::run(&mut s, project);
    assert_eq!(
        s.status.get("project-monitor").map(String::as_str),
        Some("Starting"),
        "{}",
        s.studio.message
    );
}
#[test]
fn settings_show_only_token_results_not_the_data_status_line() {
    let mut rt = Runtime::new(
        State::new(),
        crate::view,
        (1216., 900.),
        Env {
            scale: 1.,
            system_dark: false,
        },
    );
    rt.update(|s, _| s.navigate(crate::ui::Page::Settings));
    rt.frame(0.1);
    assert!(rt.find("hf-token").is_some());
    assert!(rt.find("studio-status").is_none());
    assert!(rt.find("hf-status-note").is_none());
    // A token action's reply is shown; another action's later message is not.
    rt.update(|s, _| {
        s.studio
            .tx
            .send(serde_json::json!({"ok":true,"action":"validate_token",
                "data":{"message":"Token is valid.","stored":true}}))
            .unwrap();
        poll(s);
        s.studio
            .tx
            .send(serde_json::json!({"ok":true,"action":"models","data":{"message":"3 models"}}))
            .unwrap();
        poll(s);
    });
    rt.frame(0.1);
    assert!(rt.find("hf-status-note").is_some());
    assert_eq!(rt.state.studio.credential_note, "Token is valid.");
    assert_eq!(rt.state.studio.message, "3 models");
}

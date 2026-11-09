use super::*;
use rune::{Env, Runtime};
fn candidate(tool: &str, path: &str, exe: &str) -> Candidate {
    Candidate {
        tool: tool.into(),
        found: true,
        compatible: true,
        path: path.into(),
        executable: exe.into(),
        version: "test".into(),
        environment: "Ubuntu-24.04".into(),
        ..Default::default()
    }
}
#[test]
fn adoption_changes_only_selected_profile_and_requires_no_active_processes() {
    let mut s = State::new();
    let before = s.config.clone();
    choose(&mut s, 3);
    s.setup.candidate = Some(candidate(
        "mavsdk",
        "/home/user/project with spaces",
        "/home/user/project with spaces/bin/python",
    ));
    profiles::adopt(&mut s);
    let p = s
        .config
        .profiles
        .iter()
        .find(|p| p.id == "telemetry")
        .unwrap();
    assert!(p.args[0].contains("'/home/user/project with spaces/bin/python'"));
    for profile in before.profiles.iter().filter(|p| p.id != "telemetry") {
        let after = s
            .config
            .profiles
            .iter()
            .find(|p| p.id == profile.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(profile).unwrap(),
            serde_json::to_value(after).unwrap()
        );
    }
    s.status.insert("sih".into(), "Running".into());
    choose(&mut s, 4);
    s.setup.candidate = Some(candidate("px4", "/new/px4", "/new/px4/bin/px4"));
    profiles::adopt(&mut s);
    assert!(s.setup.message.contains("Stop the active"));
}

#[test]
fn discovered_paths_cannot_be_applied_to_a_different_linux_environment() {
    let mut s = State::new();
    choose(&mut s, 3);
    s.setup.candidate = Some(candidate(
        "mavsdk",
        "/home/user/venv",
        "/home/user/venv/bin/python",
    ));
    s.setup.distro = TextEdit::new("OtherLinux");
    let before = serde_json::to_string(&s.config).unwrap();
    profiles::adopt(&mut s);
    assert_eq!(before, serde_json::to_string(&s.config).unwrap());
    assert!(s.setup.message.contains("another WSL environment"));
}
#[test]
fn unknown_or_incompatible_tools_cannot_be_adopted() {
    let mut s = State::new();
    let before = serde_json::to_string(&s.config).unwrap();
    let mut c = candidate("dds", "/tmp/agent", "/tmp/agent");
    c.compatible = false;
    s.setup.candidate = Some(c);
    profiles::adopt(&mut s);
    assert_eq!(serde_json::to_string(&s.config).unwrap(), before);
    assert!(s.setup.message.contains("manual compatibility"));
}
#[test]
fn wsl_environment_selection_requires_an_installed_v2_distribution() {
    let mut s = State::new();
    preview(&mut s);
    s.setup.distro = TextEdit::new("NotInstalled");
    use_distro(&mut s);
    assert_ne!(s.config.distro, "NotInstalled");
    select_distro(&mut s, "Ubuntu-24.04");
    use_distro(&mut s);
    assert_eq!(s.config.distro, "Ubuntu-24.04");
}
#[test]
fn setup_ui_navigation_and_review_do_not_install_anything_in_preview() {
    let mut rt = Runtime::new(
        State::new(),
        view,
        (1216., 800.),
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
    assert_eq!(rt.state.page, crate::ui::Page::Settings);
    rt.update(|s, _| {
        choose(s, 0);
        review(s);
    });
    rt.frame(0.2);
    assert!(rt.find("installation-review").is_some());
    rt.update(|s, _| request(s, "start"));
    assert!(!rt.state.setup.pending);
    assert!(rt.state.setup.jobs.is_empty());
}
#[test]
fn recipes_are_unique_and_only_maintained_entries_offer_automatic_installation() {
    let setup = Setup::default();
    let mut ids = std::collections::HashSet::new();
    for recipe in &setup.recipes {
        assert!(ids.insert(&recipe.id));
        assert!(recipe.docs.starts_with("https://"));
        if recipe.automatic {
            assert!(!recipe.steps.is_empty());
        }
    }
    assert!(
        setup
            .recipes
            .iter()
            .find(|r| r.id == "ardupilot")
            .unwrap()
            .automatic
    );
}

#[test]
fn default_profiles_do_not_embed_the_original_linux_username() {
    let config = backend::defaults();
    assert!(
        !serde_json::to_string(&config)
            .unwrap()
            .contains("/home/hp/")
    );
    assert!(
        config
            .profiles
            .iter()
            .find(|p| p.id == "telemetry")
            .unwrap()
            .args[0]
            .contains("$HOME")
    );
}

#[test]
fn missing_wsl_opens_setup_without_starting_an_installation() {
    let mut s = State::new();
    apply_event(
        &mut s,
        json!({"ok":true,"action":"scan","data":{
        "build":26100,"windows":"Windows fixture","architecture":"64-bit","eligible":true,
        "virtualization":"Available","ram_gib":16.,"gpu":"Fixture","distros":[],"wsl_ready":false}}),
    );
    assert_eq!(
        s.page,
        crate::ui::Page::Setup,
        "first use opens guided setup"
    );
    assert!(s.setup.jobs.is_empty());
    assert!(!s.setup.pending);
    assert!(!s.wsl_ready());
}
#[test]
fn advanced_editor_cannot_switch_distribution_with_active_processes() {
    let mut s = State::new();
    let original = s.config.distro.clone();
    s.status.insert("sih".into(), "Running".into());
    s.edit();
    let mut next = s.config.clone();
    next.distro = "AnotherDistribution".into();
    s.editor = TextEdit::new(&serde_json::to_string(&next).unwrap());
    s.apply();
    assert_eq!(s.config.distro, original);
    assert!(s.notice.contains("Stop active"));
}
#[test]
fn retry_retains_original_target_location_and_distribution() {
    let mut s = State::new();
    restore_retry(
        &mut s,
        &json!({"tool":"mavsdk","distro":"Ubuntu-24.04","location":"/home/user/project with spaces/.venv"}),
    );
    assert_eq!(
        s.setup.location.text,
        "/home/user/project with spaces/.venv"
    );
    assert_eq!(s.setup.recipes[s.setup.selected].id, "mavsdk");
    assert!(s.setup.review);
}
#[test]
fn wizard_shows_real_results_without_promoting_process_state_to_connection() {
    let mut s = State::new();
    preview(&mut s);
    s.setup
        .checks
        .insert("px4".into(), candidate("px4", "/px4", "/px4/bin/px4"));
    crate::guided::open(&mut s);
    s.guided.chapter = 2;
    let mut rt = Runtime::new(
        s,
        view,
        (1216., 800.),
        Env {
            system_dark: false,
            scale: 1.,
        },
    );
    rt.frame(0.);
    assert!(rt.find("workflow-wizard").is_some());
    assert!(rt.find("workflow-reuse-px4").is_some());
    assert!(rt.find("workflow-reuse-mavsdk").is_none());
}
#[test]
fn manual_windows_arguments_preserve_spaces_without_json_escaping() {
    let mut s = State::new();
    profile_editor::create(&mut s, "AI & data");
    let f = s.form.as_mut().unwrap();
    f.fields[1] = TextEdit::new("C:\\Program Files\\Python\\python.exe");
    f.fields[2] = TextEdit::new("C:\\My Project\\infer.py\n--device\ncpu");
    let mut rt = Runtime::new(
        s,
        view,
        (1440., 960.),
        Env {
            system_dark: false,
            scale: 1.,
        },
    );
    rt.frame(0.);
    let (x, y) = rt.find("save-form").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
    let added = rt.state.config.profiles.last().unwrap();
    assert_eq!(
        added.args,
        vec!["C:\\My Project\\infer.py", "--device", "cpu"]
    );
    assert_eq!(added.category, "AI & data");
}

#[test]
fn check_progress_never_underflows_when_the_queue_outgrows_the_preset() {
    let mut s = State::new();
    s.setup.workflow = 1;
    s.setup.queue = workflow::ids(&s)
        .iter()
        .filter_map(|id| s.setup.recipe_index(id))
        .collect();
    s.setup.workflow = 3;
    assert!(s.setup.queue.len() > workflow::ids(&s).len());
    assert_eq!(workflow::progress(&s), Some((0, workflow::ids(&s).len())));
}

#[test]
fn every_preset_names_catalog_recipes_and_each_system_keeps_its_workflow() {
    let s = State::new();
    for (name, ids) in workflow::PRESETS {
        for id in *ids {
            assert!(
                s.setup.recipe_index(id).is_some(),
                "{name} needs unknown recipe {id}"
            );
        }
    }
    let names: Vec<_> = crate::guided::PRESET
        .iter()
        .map(|&i| workflow::PRESETS[i].0)
        .collect();
    assert_eq!(
        names,
        [
            "Lightweight visual simulation",
            "PX4 with ROS 2 and recordings",
            "Offline CPU inference",
            "Native vehicle monitoring",
        ]
    );
}

#[test]
fn adopting_qgroundcontrol_keeps_gpu_safe_mode_opt_in() {
    let mut s = State::new();
    let qgc = s.setup.recipe_index("qgc").unwrap();
    let i = s
        .config
        .profiles
        .iter()
        .position(|p| p.id == "qgc")
        .unwrap();
    let exe = r"C:\Program Files\QGroundControl\bin\QGroundControl.exe";
    for (before, after) in [(vec![], vec![]), (vec!["--swrast"], vec!["-swrast"])] {
        s.config.profiles[i].args = before.into_iter().map(String::from).collect();
        choose(&mut s, qgc);
        s.setup.candidate = Some(candidate("qgc", r"C:\Program Files\QGroundControl", exe));
        profiles::adopt(&mut s);
        assert_eq!(s.setup.last_applied.as_deref(), Some("qgc"));
        assert_eq!(s.config.profiles[i].args, after);
        assert_eq!(s.config.profiles[i].program, exe);
    }
}

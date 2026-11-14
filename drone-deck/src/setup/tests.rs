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
    let p = s.config.profiles.iter().find(|p| p.id == "mavsdk").unwrap();
    assert!(p.args[0].contains("'/home/user/project with spaces/bin/python'"));
    assert_eq!(before.profiles.len() + 1, s.config.profiles.len());
    for profile in &before.profiles {
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
    assert!(s.setup.message.contains("Stop the session"));
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
            .find(|p| p.id == "gazebo")
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
    assert!(s.notice.contains("Stop the session"));
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
fn an_obsolete_probe_is_discarded_even_before_the_wizard_poll_runs() {
    let mut s = State::new();
    crate::guided::new_system(&mut s);
    s.setup.probe_context = Some(crate::guided::lifecycle::check_context(&s));
    s.guided.draft.world = "jmavsim".into();
    apply_event(
        &mut s,
        json!({"ok":true,"action":"detect","data":{
            "tool":"px4","found":true,"compatible":true,"path":"/px4"
        }}),
    );
    assert!(!s.setup.checks.contains_key("px4"));
    assert!(s.setup.candidate.is_none());
}
#[test]
fn wizard_shows_real_results_without_promoting_process_state_to_connection() {
    let mut s = State::new();
    preview(&mut s);
    s.setup
        .checks
        .insert("px4".into(), candidate("px4", "/px4", "/px4/bin/px4"));
    crate::guided::open(&mut s);
    s.guided.chapter = crate::guided::REVIEW;
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
    s.setup.workflow = crate::guided::template_preset(crate::systems::ROS);
    s.setup.queue = workflow::ids(&s)
        .iter()
        .filter_map(|id| s.setup.recipe_index(id))
        .collect();
    s.setup.workflow = crate::guided::template_preset(crate::systems::AI);
    assert!(s.setup.queue.len() > workflow::ids(&s).len());
    assert_eq!(workflow::progress(&s), Some((0, workflow::ids(&s).len())));
}

#[test]
fn every_preset_names_catalog_recipes_and_each_system_keeps_its_workflow() {
    let s = State::new();
    let mut systems: Vec<crate::systems::System> = (0..4)
        .map(|k| crate::systems::model::starter(k, "t", "T"))
        .collect();
    let mut ap = crate::systems::model::starter(0, "a", "A");
    ap.vehicle = crate::systems::Vehicle::ardupilot("copter");
    systems.push(ap);
    for sys in &systems {
        let preset = super::presets::for_system(sys);
        for id in &preset.recipes {
            assert!(
                s.setup.recipe_index(id).is_some(),
                "{} needs unknown recipe {id}",
                preset.name
            );
        }
    }
    let names: Vec<_> = systems
        .iter()
        .map(|x| super::presets::for_system(x).name)
        .collect();
    assert_eq!(
        names,
        [
            "PX4 SIH simulation",
            "PX4 + Gazebo with ROS 2 and recordings",
            "Offline CPU inference",
            "Physical vehicle",
            "ArduPilot development",
        ]
    );
    // The template recipe lists are the fixed presets of earlier versions, in order.
    assert_eq!(
        super::presets::for_system(&systems[1]).recipes,
        [
            "px4",
            "gazebo",
            "ros2",
            "ros-gz",
            "dds",
            "ros-workspace",
            "foxglove-bridge",
            "qgc"
        ]
    );
}

/// UI-03: the physical preset lost "(read-only link)" (it installs QGroundControl, which is
/// bridged both ways when enabled). A checkpoint saved under the old name still resumes.
#[test]
fn a_checkpoint_saved_under_the_old_physical_preset_name_still_resumes() {
    use super::presets::resolve;
    let physical = resolve("Physical vehicle", None).expect("current name");
    assert_eq!(physical.recipes, ["qgc"]);
    let old = resolve("Physical vehicle (read-only link)", None).expect("old name");
    assert_eq!(old, physical);
    assert_eq!(old.name, "Physical vehicle", "shown under its current name");
    assert_eq!(
        resolve("PX4 SIH simulation", None).unwrap().recipes,
        ["px4", "qgc"]
    );
    assert!(resolve("Lightweight visual", None).is_none());

    let saved = serde_json::json!({
        "app_version": env!("CARGO_PKG_VERSION"),
        "batch": {"remaining": [], "waiting": null, "distro": "Ubuntu-24.04"},
        "workflow": "Physical vehicle (read-only link)",
        "message": "",
    });
    let mut s = State::new();
    batch::resume(&mut s, &saved);
    assert!(s.setup.batch.is_some(), "{}", s.setup.batch_message);
    assert_eq!(s.setup.workflow, physical);
    assert_eq!(s.setup.workflow.name, "Physical vehicle");
    assert!(s.setup.batch_message.starts_with("Resuming"));
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

#[test]
fn an_adopted_copter_build_refuses_another_ardupilot_vehicle() {
    let mut s = State::new();
    let ardupilot = s.setup.recipe_index("ardupilot").unwrap();
    choose(&mut s, ardupilot);
    s.setup.candidate = Some(candidate(
        "ardupilot",
        "/home/user/ardupilot",
        "/home/user/ardupilot/build/sitl/bin/arducopter",
    ));
    profiles::adopt(&mut s);
    assert_eq!(s.setup.last_applied.as_deref(), Some("ardupilot"));
    let p = s
        .config
        .profiles
        .iter()
        .find(|p| p.id == "ardupilot")
        .unwrap();
    let command = &p.args[0];
    assert!(
        command.starts_with("{ [ \"${DRONE_DECK_AP_VEHICLE:-ArduCopter}\" = ArduCopter ] ||"),
        "{command}"
    );
    assert!(command.contains("exit 2; }; } && cd '/home/user/ardupilot' && "));
    // The defaults keep the checkout's Copter file when a re-simulation moves SITL to its own
    // folder, and ROS 2 adds the DDS client's domain.
    assert!(
        command.contains("P=\"$PWD/Tools/autotest/default_params/copter.parm\""),
        "{command}"
    );
    assert!(command.contains(
        "exec '/home/user/ardupilot/build/sitl/bin/arducopter' --model + --defaults \"$P\""
    ));
    assert!(command.contains("DDS_DOMAIN_ID") && command.contains("--home"));
}

#[test]
fn adopting_jmavsim_keeps_its_px4_command_and_xplane_runs_the_chosen_exe() {
    let mut s = State::new();
    let jmavsim_before = s
        .config
        .profiles
        .iter()
        .find(|p| p.id == "jmavsim")
        .cloned()
        .unwrap();
    let jmavsim = s.setup.recipe_index("jmavsim").unwrap();
    choose(&mut s, jmavsim);
    s.setup.candidate = Some(candidate(
        "jmavsim",
        "/home/u/PX4-Autopilot",
        "/home/u/PX4-Autopilot/x.jar",
    ));
    profiles::adopt(&mut s);
    let after = s
        .config
        .profiles
        .iter()
        .find(|p| p.id == "jmavsim")
        .unwrap();
    assert_eq!(
        after.args, jmavsim_before.args,
        "the command comes from the PX4 installation"
    );
    assert_eq!(after.px4, Some(crate::px4::Mode::JMavSim));

    let xplane = s.setup.recipe_index("xplane").unwrap();
    choose(&mut s, xplane);
    let mut c = candidate(
        "xplane",
        r"D:\Games\X-Plane 12",
        r"D:\Games\X-Plane 12\X-Plane.exe",
    );
    c.environment = String::new();
    s.setup.candidate = Some(c);
    profiles::adopt(&mut s);
    let xp = s.config.profiles.iter().find(|p| p.id == "xplane").unwrap();
    assert_eq!(xp.program, r"D:\Games\X-Plane 12\X-Plane.exe");
    assert_eq!(xp.cwd, r"D:\Games\X-Plane 12");
    assert!(xp.args.is_empty());
}

#[test]
fn a_guided_tool_reads_guide_until_checked_and_is_never_queued() {
    let mut s = State::new();
    let xplane = &s.setup.recipes[s.setup.recipe_index("xplane").unwrap()];
    assert!(xplane.guided() && !xplane.automatic && xplane.guide.len() >= 3);
    assert_eq!(xplane.pick, "windows-folder");
    s.setup.workflow = presets::Preset {
        name: "PX4 + X-Plane".into(),
        recipes: vec!["px4".into(), "xplane".into()],
    };
    assert_eq!(workflow::status(&s, "xplane").0, "Guide");
    for id in ["px4", "xplane"] {
        let mut missing = candidate(id, "", "");
        (missing.found, missing.compatible) = (false, false);
        s.setup.checks.insert(id.into(), missing);
    }
    assert_eq!(workflow::status(&s, "xplane").0, "Guide", "not found yet");
    assert_eq!(workflow::status(&s, "px4").0, "Not found");
    s.setup.host = Some(Host {
        distros: vec![Distro {
            name: "Ubuntu-24.04".into(),
            version: 2,
            ..Default::default()
        }],
        ..Default::default()
    });
    s.headless = false;
    batch::begin(&mut s);
    s.headless = true;
    let batch = s.setup.batch.as_ref().expect("queue started");
    assert_eq!(batch.remaining(), [s.setup.recipe_index("px4").unwrap()]);
    assert!(
        s.setup
            .batch_message
            .contains("1 tool(s) need a manual install"),
        "{}",
        s.setup.batch_message
    );
    s.setup.checks.insert(
        "xplane".into(),
        candidate("xplane", "D:/X", "D:/X/X-Plane.exe"),
    );
    assert_eq!(workflow::status(&s, "xplane").0, "Found");
}

#[test]
fn a_chosen_location_is_kept_for_its_tool_and_checked() {
    let mut s = State::new();
    let px4 = s.setup.recipe_index("px4").unwrap();
    choose(&mut s, px4);
    apply_event(
        &mut s,
        json!({"ok": true, "action": "browse", "data": {"tool": "px4", "location": "/home/u/PX4-Autopilot"}}),
    );
    assert_eq!(s.setup.location.text, "/home/u/PX4-Autopilot");
    assert_eq!(
        s.setup.overrides.get("px4").map(String::as_str),
        Some("/home/u/PX4-Autopilot")
    );
    assert!(
        s.notice.contains("Preview"),
        "the check was requested: {}",
        s.notice
    );
    apply_event(
        &mut s,
        json!({"ok": true, "action": "browse", "data": {"tool": "px4", "location": ""}}),
    );
    assert_eq!(s.setup.message, "No location was chosen.");
    assert_eq!(
        s.setup.overrides.len(),
        1,
        "a cancelled picker keeps the earlier choice"
    );
}

#[test]
fn a_chosen_webots_or_realflight_program_becomes_its_launch_profile() {
    let mut s = State::new();
    for id in ["webots", "realflight"] {
        let index = s.setup.recipe_index(id).unwrap();
        assert!(s.setup.recipes[index].guided(), "{id}");
        assert_eq!(s.setup.recipes[index].pick, "windows-file");
        choose(&mut s, index);
        let mut c = candidate(id, r"E:\Sims", r"E:\Sims\sim.exe");
        c.environment = String::new();
        s.setup.candidate = Some(c);
        profiles::adopt(&mut s);
        let p = s.config.profiles.iter().find(|p| p.id == id).unwrap();
        assert_eq!(
            (p.program.as_str(), p.cwd.as_str()),
            (r"E:\Sims\sim.exe", r"E:\Sims")
        );
    }
    let flightgear = &s.setup.recipes[s.setup.recipe_index("flightgear").unwrap()];
    assert!(flightgear.automatic && !flightgear.guided());
}

use super::*;
use crate::resim::{Flight, recipe};

fn recipe_for(adapter: &str, world: &str) -> Recipe {
    Recipe {
        schema: recipe::SCHEMA,
        origin: "simulated".into(),
        system: "Lab".into(),
        adapter: adapter.into(),
        autopilot: AutopilotKind::Px4,
        world: World {
            id: world.into(),
            params: [("world".to_string(), "default".to_string())].into(),
        },
        vehicle: None,
        autopilot_version: String::new(),
        home: None,
        params: Some("params.params".into()),
        command_topics: vec!["/fmu/in/trajectory_setpoint".into()],
        evidence_topics: vec!["/fmu/out/vehicle_status_v1".into()],
        anchor: String::new(),
        drone_deck: String::new(),
        source: String::new(),
        command_types: Default::default(),
    }
}

fn flight(adapter: &str, commands: u64) -> Flight {
    Flight {
        id: "20261114T101500Z-offboard".into(),
        label: "offboard".into(),
        recipe: json!({"adapter": adapter, "commands": commands}),
        ..Default::default()
    }
}

#[test]
fn the_temporary_system_flies_the_recorded_world_on_the_isolated_domain() {
    let sys = system_for(&recipe_for("px4-gazebo", "gazebo"), &Target::default());
    assert!(sys.scratch());
    assert_eq!(sys.adapter().map(|a| a.id), Some("px4-gazebo"));
    assert_eq!(sys.param("world"), Some("default"));
    assert_eq!(
        systems::session_stages(&sys),
        ["gazebo", "router", "link", "dds", "rosgz"],
        "no QGroundControl, ROS 2 for the arbiter"
    );
    assert_eq!(sys.consumers.ros.domain, DOMAIN);
    assert!(
        sys.consumers.scripts_udp.is_none()
            && !sys.consumers.foxglove
            && !sys.consumers.ros.recorder
    );

    let env = crate::backend::launch_env("gazebo", &sys);
    let has = |k: &str, v: &str| env.iter().any(|(a, b)| a == k && b == v);
    assert!(
        has("PX4_UXRCE_DDS_NS", NAMESPACE) && has("ROS_DOMAIN_ID", "79"),
        "{env:?}"
    );
    assert!(has("ROS_LOCALHOST_ONLY", "1"));
    let dds = crate::backend::launch_env("dds", &sys);
    assert!(dds.iter().any(|(k, v)| k == "ROS_DOMAIN_ID" && v == "79"));
    assert!(
        !dds.iter().any(|(k, _)| k == "PX4_UXRCE_DDS_NS"),
        "only the autopilot takes the namespace"
    );

    let normal = template(crate::systems::ROS, "lab", "Lab");
    assert!(
        !crate::backend::launch_env("gazebo", &normal)
            .iter()
            .any(|(k, _)| k == "PX4_UXRCE_DDS_NS" || k == "ROS_LOCALHOST_ONLY"),
        "a saved system is unchanged"
    );
}

#[test]
fn the_temporary_system_is_never_saved_or_listed() {
    let mut s = State::new();
    let before = s.systems.active_id().to_string();
    s.systems.add_scratch(system_for(
        &recipe_for("px4-sih", "internal"),
        &Target::default(),
    ));
    assert!(s.systems.set_active(SCRATCH));
    assert!(s.systems.saved().all(|x| !x.scratch()));
    let path = std::env::temp_dir().join(format!("drone-deck-resim-{}.json", std::process::id()));
    s.systems.save_at(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(!text.contains(SCRATCH), "{text}");
    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        file["active"],
        s.systems.list[0].id.as_str(),
        "the saved active system is a kept one"
    );
    s.systems.drop_scratch(&before);
    assert_eq!(s.systems.active_id(), before);
    assert!(s.systems.get(SCRATCH).is_none());
}

#[test]
fn a_run_is_refused_with_a_reason() {
    let mut s = State::new();
    assert!(refusal(&s).unwrap().contains("Choose a flight"));
    s.resim.flights = vec![flight("ardupilot-sitl", 2)];
    s.resim.selected = Some(s.resim.flights[0].id.clone());
    assert!(refusal(&s).unwrap().contains("ArduPilot"));
    s.resim.flights = vec![flight("px4-gazebo", 0)];
    assert!(refusal(&s).unwrap().contains("no ROS 2 commands"));
    s.resim.flights = vec![flight("px4-gazebo", 2)];
    assert!(
        refusal(&s).unwrap().contains("Headless"),
        "nothing runs in tests"
    );
    s.resim.flights = vec![flight("px4-mars", 2)];
    assert!(refusal(&s).unwrap().contains("no adapter"));
}

fn active(s: &State) -> Active {
    Active {
        flight: "20261114T101500Z-offboard".into(),
        label: "offboard".into(),
        phase: Phase::Preparing,
        progress: String::new(),
        since: Instant::now(),
        previous: s.systems.active_id().to_string(),
        recorded: None,
        skipped: Vec::new(),
        cancel: Arc::new(AtomicBool::new(false)),
        outcome: String::new(),
        code: None,
        settings: (None, 0.0, 0.5),
        target: Target::default(),
        same_world: true,
        home: false,
    }
}

#[test]
fn a_prepared_run_launches_the_temporary_system_and_cleans_up_after_a_failure() {
    let mut s = State::new();
    let before = s.systems.active_id().to_string();
    s.resim.run = Some(active(&s));
    let reply = json!({"recipe": serde_json::to_value(recipe_for("px4-sih", "internal")).unwrap(),
        "params": "1\t1\tMPC_XY_VEL_MAX\t8.0\t9\n"});
    prepared(&mut s, Ok(reply));
    assert_eq!(s.systems.active_id(), SCRATCH);
    let run = s.resim.run.as_ref().unwrap();
    assert_eq!(run.phase, Phase::Launching);
    assert_eq!(run.recorded.as_ref().map(|p| p.params.len()), Some(1));
    assert_eq!(
        s.session.as_ref().map(|l| l.stages().to_vec()),
        Some(vec!["sih", "router", "link", "dds"])
    );

    // The launch stops at the simulator: the run ends and the system goes back.
    s.session = None;
    s.launch_failure = Some(("sih".into(), "Exited 1.".into()));
    assert!(poll(&mut s));
    assert_eq!(s.resim.run.as_ref().unwrap().phase, Phase::Stopping);
    assert!(s.resim.message.contains("stopped at sih"));
    assert!(poll(&mut s));
    assert!(s.resim.run.is_none());
    assert_eq!(s.systems.active_id(), before);
    assert!(s.systems.get(SCRATCH).is_none());
}

#[test]
fn an_unreadable_recipe_starts_nothing() {
    let mut s = State::new();
    let before = s.systems.active_id().to_string();
    s.resim.run = Some(active(&s));
    prepared(&mut s, Ok(json!({"recipe": {"adapter": 3}})));
    assert!(s.resim.run.is_none());
    assert!(s.resim.message.contains("unreadable"));
    assert!(s.session.is_none());
    assert_eq!(s.systems.active_id(), before);
}

#[test]
fn recorded_parameters_are_written_after_the_fresh_ones_arrive() {
    let mut s = State::new();
    s.resim.run = Some(active(&s));
    let reply = json!({"recipe": serde_json::to_value(recipe_for("px4-sih", "internal")).unwrap(),
        "params": "1\t1\tMPC_XY_VEL_MAX\t8.0\t9\n1\t1\tCAL_ACC0_ID\t5\t6\n"});
    prepared(&mut s, Ok(reply));
    let run = s.resim.run.as_mut().unwrap();
    enter(run, Phase::ReadingParams);
    s.vehicle.params =
        Some(crate::mavlink::ParamSet::from_qgc("1\t1\tMPC_XY_VEL_MAX\t12.0\t9\n").0);
    // No link in tests: the write is refused, and the run reports it and stops.
    assert!(poll(&mut s));
    let run = s.resim.run.as_ref().unwrap();
    assert_eq!(
        run.skipped,
        [("CAL_ACC0_ID".to_string(), "hardware calibration or wiring")]
    );
    assert_eq!(run.phase, Phase::Stopping);
    assert!(
        s.resim
            .message
            .contains("Could not write the recorded parameters"),
        "{}",
        s.resim.message
    );
}

#[test]
fn the_tab_shows_flights_and_runs() {
    let mut s = State::new();
    crate::resim::preview(&mut s);
    let mut rt = rune::Runtime::new(
        s,
        crate::view,
        (1216., 900.),
        rune::Env {
            scale: 1.,
            system_dark: false,
        },
    );
    rt.frame(0.1);
    for key in [
        "resim-tab",
        "resim-setup",
        "resim-start",
        "resim-run-20261114T120000Z",
        "resim-flight-20261113T164000Z-takeoff",
        "resim-code",
        "resim-tolerance-percent",
        "resim-floor",
        "resim-hold",
    ] {
        assert!(rt.find(key).is_some(), "{key}");
    }
}

#[test]
fn takeover_settings_are_checked_before_a_run() {
    let mut s = State::new();
    assert_eq!(
        s.resim.settings(),
        Ok((None, 0.0, 0.5)),
        "off by default: a pure replay"
    );
    s.resim.takeover = true;
    assert_eq!(s.resim.settings(), Ok((Some(10.0), 0.2, 0.5)));
    s.resim.tolerance = rune::TextEdit::new("150");
    s.resim.flights = vec![flight("px4-gazebo", 2)];
    s.resim.selected = Some(s.resim.flights[0].id.clone());
    assert!(refusal(&s).unwrap().contains("Tolerance (%)"));
    s.resim.tolerance = rune::TextEdit::new("0");
    s.resim.hold = rune::TextEdit::new("x");
    assert!(refusal(&s).unwrap().contains("hold time"));
    s.resim.hold = rune::TextEdit::new("0.5");
    s.resim.code = Some("avoid-v2".into());
    assert!(
        refusal(&s).unwrap().contains("avoid-v2 is not in"),
        "a project must exist to run in shadow"
    );
    s.studio.library = json!({"projects": [{"name": "avoid-v2", "kind": "ros2", "distro": s.config.distro},
        {"name": "detector", "kind": "onnx", "distro": s.config.distro},
        {"name": "elsewhere", "kind": "ros2", "distro": "Other"}]});
    let names: Vec<_> = crate::resim::projects(&s)
        .iter()
        .map(|p| p["name"].clone())
        .collect();
    assert_eq!(
        names,
        [json!("avoid-v2")],
        "ROS 2 projects of this distribution only"
    );
    assert!(refusal(&s).unwrap().contains("Headless"));
}

#[test]
fn code_that_stops_before_the_replay_ends_the_run() {
    let mut s = State::new();
    let mut run = active(&s);
    run.code = Some(json!({"name": "avoid-v2", "kind": "ros2"}));
    enter(&mut run, Phase::StartingCode);
    s.resim.run = Some(run);
    assert!(!poll(&mut s), "waits while it starts");
    s.status
        .insert("project-avoid-v2".into(), "Exited 1".into());
    assert!(poll(&mut s));
    assert_eq!(s.resim.run.as_ref().unwrap().phase, Phase::Stopping);
    assert!(
        s.resim
            .message
            .contains("Your code stopped before the replay began (Exited 1)")
    );
}

#[test]
fn the_result_says_whether_your_code_took_over() {
    let mut s = State::new();
    s.resim.run = Some(active(&s));
    done(
        &mut s,
        Ok(
            json!({"status": "Completed", "sent": 30, "commands": 40, "tolerance": 10.0,
            "takeover": {"t": 42.3, "topic": "/fmu/in/trajectory_setpoint", "reason": "velocity differs."},
            "folder": "/runs/1"}),
        ),
    );
    assert!(
        s.resim
            .message
            .contains("your code took over at 42.3 s (velocity differs.)"),
        "{}",
        s.resim.message
    );
    s.resim.run = Some(active(&s));
    done(
        &mut s,
        Ok(json!({"status": "Completed", "sent": 40, "commands": 40, "tolerance": 10.0})),
    );
    assert!(s.resim.message.contains("never differed enough"));
}

fn adapter(id: &str) -> &'static Adapter {
    crate::systems::adapter::ALL
        .iter()
        .find(|a| a.id == id)
        .unwrap()
}

#[test]
fn a_gazebo_flight_flies_in_jmavsim_from_the_recorded_home() {
    let mut r = recipe_for("px4-gazebo", "gazebo");
    r.home = Some(recipe::Home {
        lat: 47.397742,
        lon: 8.545594,
        alt: 488.0,
    });
    let target = Target {
        adapter: Some(adapter("px4-jmavsim")),
        conditions: Vec::new(),
    };
    assert!(!target.same_world(&r));
    let sys = system_for(&r, &target);
    assert_eq!(sys.adapter().map(|a| a.id), Some("px4-jmavsim"));
    assert_eq!(
        sys.param("world"),
        None,
        "Gazebo's world name is not carried to jMAVSim"
    );
    let env = crate::backend::launch_env("jmavsim", &sys);
    let get = |k: &str| env.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    assert_eq!(
        (
            get("PX4_HOME_LAT"),
            get("PX4_HOME_LON"),
            get("PX4_HOME_ALT")
        ),
        (Some("47.397742"), Some("8.545594"), Some("488"))
    );
    assert_eq!(get("PX4_UXRCE_DDS_NS"), Some(NAMESPACE));

    let recorded = crate::mavlink::ParamSet::from_qgc(
        "1	1	SIM_GZ_EN	1	6
1	1	MPC_XY_VEL_MAX	8.0	9
",
    )
    .0;
    let plan = transfer::plan(&recorded, None, target.same_world(&r));
    assert_eq!(
        plan.apply
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["MPC_XY_VEL_MAX"]
    );
    assert_eq!(
        plan.skipped,
        [("SIM_GZ_EN".to_string(), transfer::Skip::OtherSimulator)]
    );

    // Without a recorded home, no home variables: the world's default start.
    let plain = system_for(&recipe_for("px4-gazebo", "gazebo"), &target);
    assert!(
        !crate::backend::launch_env("jmavsim", &plain)
            .iter()
            .any(|(k, _)| k.starts_with("PX4_HOME"))
    );
}

#[test]
fn wind_is_offered_in_gazebo_and_greyed_elsewhere() {
    let wind = |id: &str| {
        crate::resim::conditions(adapter(id))
            .into_iter()
            .find(|(k, _, _)| *k == "wind")
            .unwrap()
            .2
    };
    assert!(wind("px4-gazebo").is_ok());
    assert!(
        wind("px4-sih")
            .unwrap_err()
            .contains("PX4 SIH has no wind setting")
    );
    assert!(wind("px4-jmavsim").is_err());
    let r = recipe_for("px4-gazebo", "gazebo");
    let windy = system_for(
        &r,
        &Target {
            adapter: None,
            conditions: vec!["wind".into()],
        },
    );
    assert_eq!(windy.param("world"), Some("windy"));
    // A condition the target does not offer changes nothing.
    let sih = system_for(
        &r,
        &Target {
            adapter: Some(adapter("px4-sih")),
            conditions: vec!["wind".into()],
        },
    );
    assert_eq!(sih.param("world"), None);
}

#[test]
fn a_flight_can_fly_in_any_world_of_its_autopilot() {
    let f = flight("px4-gazebo", 2);
    let targets = crate::resim::targets(&f);
    assert_eq!(targets[0].0.id, "px4-gazebo", "the recorded world first");
    let ids: Vec<&str> = targets.iter().map(|(a, _)| a.id).collect();
    assert!(ids.contains(&"px4-sih") && ids.contains(&"px4-jmavsim"));
    assert!(ids.iter().all(|id| id.starts_with("px4-")), "{ids:?}");
    assert!(
        targets.iter().all(|(_, ok)| ok.is_ok()),
        "PX4 worlds take the same commands"
    );
}

use super::recipe::*;
use crate::State;
use crate::systems::{self, Vehicle, adapter, model::starter};

#[test]
fn topics_split_into_commands_and_evidence_by_prefix() {
    let px4 = adapter::find(adapter::AutopilotKind::Px4, "gazebo").unwrap();
    let topics: Vec<String> = [
        "/fmu/in/trajectory_setpoint",
        "/fmu/in/vehicle_command_v1",
        "/fmu/out/vehicle_status_v1",
        "/camera/image_raw",
    ]
    .iter()
    .map(|t| t.to_string())
    .collect();
    let (commands, evidence) = split(&topics, px4.ros.as_ref().unwrap());
    assert_eq!(
        commands,
        ["/fmu/in/trajectory_setpoint", "/fmu/in/vehicle_command_v1"]
    );
    assert_eq!(evidence, ["/fmu/out/vehicle_status_v1"]);

    let ap = adapter::find(adapter::AutopilotKind::ArduPilot, "internal").unwrap();
    let topics: Vec<String> = ["/ap/cmd_vel", "/ap/joy", "/ap/pose/filtered"]
        .iter()
        .map(|t| t.to_string())
        .collect();
    let (commands, evidence) = split(&topics, ap.ros.as_ref().unwrap());
    assert_eq!(commands, ["/ap/cmd_vel", "/ap/joy"]);
    assert_eq!(evidence, ["/ap/pose/filtered"]);
}

#[test]
fn a_recipe_names_the_adapter_world_and_details_of_the_system() {
    let mut s = State::new();
    assert!(s.select_starter(systems::ROS));
    let r = capture(&s, &[]).expect("a simulated system");
    assert_eq!(
        (r.adapter.as_str(), r.world.id.as_str()),
        ("px4-gazebo", "gazebo")
    );
    assert_eq!(
        r.world.params.get("model").map(String::as_str),
        Some("x500_mono_cam")
    );
    assert_eq!(r.vehicle, None);
    assert_eq!(r.origin, "simulated");

    let id = s.systems.active_id().to_string();
    s.systems.get_mut(&id).unwrap().vehicle = Vehicle::ardupilot("plane");
    let r = capture(&s, &[]).unwrap();
    assert_eq!(r.vehicle.as_deref(), Some("plane"));
    assert_eq!(r.autopilot, adapter::AutopilotKind::ArduPilot);

    for kind in [systems::AI, systems::PHYSICAL] {
        assert!(s.select_starter(kind));
        assert!(capture(&s, &[]).is_none(), "{kind}");
    }
    let json = serde_json::to_value(
        capture(
            &{
                let mut t = State::new();
                t.systems.list[0] = starter(systems::SIMULATION, "local-simulation", "Lab");
                t
            },
            &[],
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(json["schema"], SCHEMA);
    assert_eq!(json["autopilot"], "px4");
    assert!(
        json.get("home").is_none() && json.get("params").is_none(),
        "unknown, not invented"
    );
}

#[test]
fn only_the_autopilots_discovered_topics_are_wanted() {
    let mut s = State::new();
    s.recorder.discovered = [
        "/fmu/out/vehicle_status_v1",
        "/rosout",
        "/fmu/in/vehicle_command",
    ]
    .iter()
    .map(|n| crate::recordings::Topic {
        name: (*n).into(),
        ..Default::default()
    })
    .collect();
    assert_eq!(
        wanted_topics(&s),
        ["/fmu/out/vehicle_status_v1", "/fmu/in/vehicle_command"]
    );
    assert!(params_text(&s).is_none(), "no link");
}

use super::PREVIEW_REPORT as COMPARE;

#[test]
fn the_comparison_compare_py_writes_is_read_and_summarized() {
    let r: super::compare::Report = serde_json::from_str(COMPARE).unwrap();
    assert_eq!(r.verdict, "changed");
    assert_eq!(r.path.split_t, Some(2.6));
    assert_eq!(
        r.verdict_line(),
        format!(
            "Changed at t = 2.6 s: beyond 10% (at least 1.0 m); at most {:.2} m apart at {:.1} s.",
            r.path.max_m, r.path.max_t
        )
    );
    assert_eq!(r.takeover_t, Some(3.0));
    assert_eq!(r.events.missing, 1, "the fresh flight never landed");
    assert_eq!(r.ai[0].agree, 8);
    let plot = r.distance_plot();
    assert_eq!(plot.lines.len(), 2);
    assert!(r.duration() > 3.0);
    let same = super::compare::Report {
        verdict: "same".into(),
        tolerance: 0.1,
        ..Default::default()
    };
    assert!(same.verdict_line().starts_with("Same course: within 10%"));
    assert!(
        super::compare::Report::default()
            .verdict_line()
            .starts_with("No verdict")
    );
}

#[test]
fn ai_topics_are_absolute_names() {
    assert_eq!(super::ai_topics(" /a, /b/c ").unwrap(), ["/a", "/b/c"]);
    assert!(super::ai_topics("").unwrap().is_empty());
    assert!(
        super::ai_topics("detections")
            .unwrap_err()
            .contains("absolute")
    );
}

#[test]
fn a_report_arrives_for_the_run_on_screen_and_is_drawn() {
    let mut s = State::new();
    crate::resim::preview(&mut s);
    s.resim.report = Some((
        "20261114T120000Z".into(),
        Err("Loading the comparison…".into()),
    ));
    let value: serde_json::Value = serde_json::from_str(COMPARE).unwrap();
    s.resim
        .tx
        .send(super::Msg::Report("someone-else".into(), Ok(value.clone())))
        .unwrap();
    s.resim
        .tx
        .send(super::Msg::Report("20261114T120000Z".into(), Ok(value)))
        .unwrap();
    super::poll(&mut s);
    let (name, report) = s.resim.report.clone().unwrap();
    assert_eq!(name, "20261114T120000Z");
    assert_eq!(report.unwrap().verdict, "changed");

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
        "resim-report",
        "resim-verdict",
        "resim-track",
        "plot-resim-distance",
        "resim-events",
        "resim-commands",
        "resim-ai",
        "resim-run-verdict-20261114T120000Z",
    ] {
        assert!(rt.find(key).is_some(), "{key}");
    }
}

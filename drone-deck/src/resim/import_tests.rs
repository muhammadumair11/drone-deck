use super::*;
use crate::ulog::fixture::{Body, Writer};

const STATUS: u16 = 0;
const OFFBOARD_MODE: u16 = 1;
const SETPOINT: u16 = 2;
const COMMAND: u16 = 3;
const HOME: u16 = 4;
const LOCAL: u16 = 5;
const T0: u64 = 1_000_000;

fn pad(mut b: Body, n: usize) -> Body {
    for _ in 0..n {
        b = b.u8(0);
    }
    b
}

/// A 6 s flight: hold until 2 s, offboard 2–5 s, then land. `offboard`: whether offboard
/// control ran; `external`: whether a ground station or DDS sent a command.
fn log(offboard: bool, external: bool) -> Vec<u8> {
    let mut w = Writer::new(T0);
    w.flags(0, 0)
        .format("vehicle_status:uint64_t timestamp;uint8_t arming_state;bool failsafe;uint8_t nav_state;uint8_t[5] _padding0;")
        .format("offboard_control_mode:uint64_t timestamp;bool position;bool velocity;uint8_t[6] _padding0;")
        .format("trajectory_setpoint:uint64_t timestamp;float[3] position;float[3] velocity;float yaw;uint8_t[4] _padding0;")
        .format("vehicle_command:uint64_t timestamp;float param1;uint32_t command;bool from_external;uint8_t[7] _padding0;")
        .format("home_position:uint64_t timestamp;double lat;double lon;float alt;uint8_t[4] _padding0;")
        .format("vehicle_local_position:uint64_t timestamp;float x;float y;float z;uint8_t[4] _padding0;")
        .info("ver_sw", "abc123")
        .param_f32("MPC_XY_VEL_MAX", 8.)
        .param_i32("SYS_AUTOSTART", 10040)
        .add(0, STATUS, "vehicle_status")
        .add(0, OFFBOARD_MODE, "offboard_control_mode")
        .add(0, SETPOINT, "trajectory_setpoint")
        .add(0, COMMAND, "vehicle_command")
        .add(0, HOME, "home_position")
        .add(0, LOCAL, "vehicle_local_position");
    w.data(
        HOME,
        T0,
        &pad(Body::default().f64(47.397742).f64(8.545594).f32(488.), 4).0,
    );
    for k in 0..=60u64 {
        let ts = T0 + k * 100_000;
        let t = k as f64 / 10.;
        let nav = if offboard && (2. ..5.).contains(&t) {
            14
        } else if t >= 5. {
            18
        } else {
            4
        };
        w.data(STATUS, ts, &pad(Body::default().u8(2).u8(0).u8(nav), 5).0);
        w.data(
            LOCAL,
            ts,
            &pad(Body::default().f32(t as f32).f32(0.).f32(-2.), 4).0,
        );
        if offboard && (2. ..5.).contains(&t) {
            w.data(OFFBOARD_MODE, ts, &pad(Body::default().u8(1).u8(0), 6).0);
        }
        // PX4's own flight tasks publish setpoints in every mode; only offboard ones count.
        let setpoint = Body::default()
            .f32(t as f32)
            .f32(0.)
            .f32(-2.)
            .f32(f32::NAN)
            .f32(f32::NAN)
            .f32(f32::NAN)
            .f32(0.5);
        w.data(SETPOINT, ts + 10, &pad(setpoint, 4).0);
    }
    // An arm command from outside, and one PX4 sends itself.
    w.data(
        COMMAND,
        T0 + 1_500_000,
        &pad(Body::default().f32(1.).u32(400).u8(external as u8), 7).0,
    );
    w.data(
        COMMAND,
        T0 + 5_000_000,
        &pad(Body::default().f32(0.).u32(21).u8(0), 7).0,
    );
    w.buf.clone()
}

fn lines(text: &str) -> Vec<Value> {
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn an_offboard_flight_exports_its_outside_commands_in_order() {
    let parsed = crate::ulog::parse(log(true, true)).unwrap();
    let e = export(&parsed, "Field test").unwrap();
    assert_eq!(e.counts["/fmu/in/offboard_control_mode"], 30);
    assert_eq!(
        e.counts["/fmu/in/trajectory_setpoint"], 30,
        "only while in Offboard"
    );
    assert_eq!(
        e.counts["/fmu/in/vehicle_command"], 1,
        "PX4's own command is not replayed"
    );
    let rows = lines(&e.commands);
    let times: Vec<u64> = rows.iter().map(|r| r["t_ns"].as_u64().unwrap()).collect();
    assert!(times.windows(2).all(|w| w[0] <= w[1]), "in time order");
    assert_eq!(rows[0]["topic"], "/fmu/in/vehicle_command");
    assert_eq!(rows[0]["message"]["command"], 400);
    assert_eq!(rows[0]["message"]["from_external"], true);
    assert_eq!(rows[0]["t_ns"], (T0 + 1_500_000) * 1000);
    let setpoint = rows
        .iter()
        .find(|r| r["topic"] == "/fmu/in/trajectory_setpoint")
        .unwrap();
    assert_eq!(
        setpoint["message"]["position"].as_array().unwrap().len(),
        3,
        "arrays back together"
    );
    assert_eq!(
        setpoint["message"]["velocity"][0], "nan",
        "PX4's unused fields stay unused"
    );
    assert!((setpoint["message"]["yaw"].as_f64().unwrap() - 0.5).abs() < 1e-6);
    assert!(setpoint["message"].get("_padding0").is_none());

    let r = &e.recipe;
    assert_eq!(
        (r.origin.as_str(), r.adapter.as_str(), r.source.as_str()),
        ("real", "px4-sih", "ulog")
    );
    assert_eq!(
        r.command_types["/fmu/in/trajectory_setpoint"],
        "px4_msgs/msg/TrajectorySetpoint"
    );
    assert_eq!(r.autopilot_version, "abc123");
    let home = r.home.unwrap();
    assert!((home.lat - 47.397742).abs() < 1e-9 && (home.alt - 488.).abs() < 1e-3);
    assert_eq!(
        r.evidence_topics,
        ["/fmu/out/vehicle_status", "/fmu/out/vehicle_local_position"]
    );
    let status = &lines(&e.evidence)[0];
    assert_eq!(status["message"]["nav_state"], 4);
    assert!(status["message"].get("_padding0").is_none());
    assert!(e.params.contains("MPC_XY_VEL_MAX\t8") && e.params.contains("SYS_AUTOSTART\t10040"));
}

#[test]
fn a_flight_flown_by_hand_has_nothing_to_replay() {
    let parsed = crate::ulog::parse(log(false, false)).unwrap();
    assert_eq!(export(&parsed, "Hand flown").unwrap_err(), MANUAL);
}

#[test]
fn ground_station_commands_alone_are_replayable() {
    let parsed = crate::ulog::parse(log(false, true)).unwrap();
    let e = export(&parsed, "Mission").unwrap();
    assert_eq!(e.recipe.command_topics, ["/fmu/in/vehicle_command"]);
}

#[test]
fn the_synthetic_flight_has_no_commands() {
    let parsed = crate::ulog::parse(crate::ulog::fixture::flight()).unwrap();
    assert_eq!(export(&parsed, "Synthetic").unwrap_err(), MANUAL);
}

#[test]
fn an_imported_real_flight_is_selected_with_the_real_tolerance() {
    let mut s = crate::State::new();
    s.resim
        .tx
        .send(crate::resim::Msg::Imported(Ok(
            json!({"id": "20261114T150000Z-field", "label": "field",
            "counts": {"/fmu/in/trajectory_setpoint": 30, "/fmu/in/vehicle_command": 1}}),
        )))
        .unwrap();
    crate::resim::poll(&mut s);
    assert!(
        s.resim
            .message
            .contains("(30 trajectory_setpoint, 1 vehicle_command)"),
        "{}",
        s.resim.message
    );
    assert_eq!(s.resim.selected.as_deref(), Some("20261114T150000Z-field"));
    let flight = |origin: &str| {
        json!({"id": "20261114T150000Z-field", "label": "field",
            "recipe": {"adapter": "px4-sih", "origin": origin, "commands": 2}, "runs": []})
    };
    s.resim
        .tx
        .send(crate::resim::Msg::Listed(Ok(
            json!({"flights": [flight("real")]}),
        )))
        .unwrap();
    crate::resim::poll(&mut s);
    assert_eq!(s.resim.tolerance.text, crate::resim::REAL_TOLERANCE);
    s.resim
        .tx
        .send(crate::resim::Msg::Listed(Ok(
            json!({"flights": [flight("simulated")]}),
        )))
        .unwrap();
    crate::resim::poll(&mut s);
    assert_eq!(s.resim.tolerance.text, crate::resim::DEFAULT_TOLERANCE);
    s.resim.tolerance = rune::TextEdit::new("15");
    s.resim
        .tx
        .send(crate::resim::Msg::Listed(Ok(
            json!({"flights": [flight("real")]}),
        )))
        .unwrap();
    crate::resim::poll(&mut s);
    assert_eq!(
        s.resim.tolerance.text, "15",
        "a tolerance you typed is kept"
    );
}

#[test]
fn an_import_refuses_other_files() {
    let mut s = crate::State::new();
    start(&mut s, std::path::PathBuf::from("flight.bin"));
    assert!(
        s.resim.message.contains("Select a working WSL 2"),
        "headless refuses first"
    );
    assert!(!s.resim.importing);
}

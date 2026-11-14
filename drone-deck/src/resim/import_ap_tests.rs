use super::*;
use crate::dataflash::fixture::{Builder, F};

const MSG: u8 = 60;
const MODE_ID: u8 = 61;
const ARM_ID: u8 = 62;
const EV: u8 = 63;
const ORGN: u8 = 64;
const POS: u8 = 65;
const GUIP: u8 = 66;
const PARM: u8 = 67;
const S: f64 = 1_000_000.;
const LAT: f64 = -35.3632621;
const LON: f64 = 149.1652374;

fn base(version: &str) -> Builder {
    let mut b = Builder::new();
    b.format(MSG, "MSG", "QZ", "TimeUS,Message")
        .format(MODE_ID, "MODE", "QMBB", "TimeUS,Mode,ModeNum,Rsn")
        .format(
            ARM_ID,
            "ARM",
            "QBIBB",
            "TimeUS,ArmState,ArmChecks,Forced,Method",
        )
        .format(EV, "EV", "QB", "TimeUS,Id")
        .format(ORGN, "ORGN", "QBLLe", "TimeUS,Type,Lat,Lng,Alt")
        .format(
            POS,
            "POS",
            "QLLfff",
            "TimeUS,Lat,Lng,Alt,RelHomeAlt,RelOriginAlt",
        )
        .format(
            GUIP,
            "GUIP",
            "QBfffbffffff",
            "TimeUS,Type,pX,pY,pZ,Terrain,vX,vY,vZ,aX,aY,aZ",
        )
        .format(PARM, "PARM", "QNff", "TimeUS,Name,Value,Default")
        .units(GUIP, "F-BBB-BBBBBB")
        .mult(b'B', 0.01)
        .row(
            MSG,
            &[
                F::N(0.5 * S),
                F::S(&format!("ArduCopter {version} (b1bbf0d6)")),
            ],
        )
        .row(
            PARM,
            &[F::N(0.6 * S), F::S("WPNAV_SPEED"), F::N(750.), F::N(1000.)],
        )
        .row(
            ORGN,
            &[F::N(0.7 * S), F::N(0.), F::N(LAT), F::N(LON), F::N(584.)],
        )
        .row(
            ORGN,
            &[F::N(0.7 * S), F::N(1.), F::N(LAT), F::N(LON), F::N(584.5)],
        );
    b
}

fn mode(b: &mut Builder, t: f64, mode: f64, reason: f64) {
    b.row(
        MODE_ID,
        &[F::N(t * S), F::N(mode), F::N(mode), F::N(reason)],
    );
}
fn arm(b: &mut Builder, t: f64, state: f64, method: f64) {
    b.row(
        ARM_ID,
        &[F::N(t * S), F::N(state), F::N(0.), F::N(0.), F::N(method)],
    );
}
fn ev(b: &mut Builder, t: f64, id: f64) {
    b.row(EV, &[F::N(t * S), F::N(id)]);
}
fn pos(b: &mut Builder, t: f64, north_m: f64, up: f64) {
    b.row(
        POS,
        &[
            F::N(t * S),
            F::N(LAT + north_m / M_PER_DEG),
            F::N(LON),
            F::N(584. + up),
            F::N(up),
            F::N(up),
        ],
    );
}
/// A Guided velocity target in cm/s (as Copter 4.6 logs it).
fn guip(b: &mut Builder, t: f64, vx: f64, vy: f64, vz: f64) {
    let mut fields = vec![F::N(t * S), F::N(SUB_VEL_ACCEL as f64)];
    fields.extend([0., 0., 0., 0., vx, vy, vz, 0., 0., 0.].map(F::N));
    b.row(GUIP, &fields);
}

fn parsed(b: &Builder) -> Log {
    crate::dataflash::parse(&b.bytes(), WANTED).unwrap()
}

fn lines(text: &str) -> Vec<Value> {
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn a_guided_flight_gives_its_calls_velocities_and_evidence() {
    let mut b = base("V4.6.3");
    mode(&mut b, 1., 0., 31.);
    mode(&mut b, 2., 4., 52.);
    arm(&mut b, 3., 1., 35.);
    ev(&mut b, 4., EV_NOT_LANDED as f64);
    for (t, up) in [(4.5, 3.0), (5.5, 9.96), (6.5, 10.0), (9.0, 25.0)] {
        pos(&mut b, t, 0.0, up);
    }
    guip(&mut b, 7., 150., -50., 20.);
    guip(&mut b, 8., 150., -50., 0.);
    mode(&mut b, 10., 9., 52.);
    guip(&mut b, 11., 99., 0., 0.); // in Land: not a Guided command
    ev(&mut b, 12., EV_LAND_COMPLETE as f64);
    arm(&mut b, 13., 0., 28.);
    let e = export(&parsed(&b), "field").unwrap();

    let calls: Vec<(f64, String, Value)> = lines(&e.commands)
        .into_iter()
        .map(|v| {
            (
                v["t_ns"].as_f64().unwrap() / 1e9,
                v["topic"].as_str().unwrap().to_string(),
                v["message"].clone(),
            )
        })
        .collect();
    let topics: Vec<(f64, &str)> = calls.iter().map(|(t, n, _)| (*t, n.as_str())).collect();
    assert_eq!(
        topics,
        [
            (2., MODE),
            (3., ARM),
            (4., TAKEOFF),
            (7., CMD_VEL),
            (8., CMD_VEL),
            (10., MODE)
        ]
    );
    assert_eq!(calls[0].2, json!({"mode": 4}));
    assert_eq!(
        calls[2].2,
        json!({"alt": 10.0}),
        "the height before the first command"
    );
    let twist = &calls[3].2["twist"]["linear"];
    let near = |v: &Value, want: f64| (v.as_f64().unwrap() - want).abs() < 1e-6;
    assert!(
        near(&twist["x"], -0.5) && near(&twist["y"], 1.5) && near(&twist["z"], 0.2),
        "north-east-up cm/s → map frame (east, north, up) m/s: {twist}"
    );
    assert_eq!(calls[3].2["header"]["frame_id"], "map");
    assert_eq!(
        e.counts.get(CMD_VEL),
        Some(&2),
        "Guided commands only, not the one in Land"
    );

    let r = &e.recipe;
    assert_eq!(
        (r.adapter.as_str(), r.vehicle.as_deref(), r.source.as_str()),
        ("ardupilot-sitl", Some("copter"), "dataflash")
    );
    assert_eq!(r.autopilot, AutopilotKind::ArduPilot);
    assert_eq!(r.autopilot_version, "V4.6.3");
    let home = r.home.unwrap();
    assert!((home.lat - LAT).abs() < 1e-7 && (home.alt - 584.5).abs() < 1e-3);
    assert_eq!(r.evidence_topics, [STATUS, POSE]);
    assert_eq!(
        r.command_types.keys().collect::<Vec<_>>(),
        [CMD_VEL],
        "services are called, not published"
    );
    assert!(e.params.contains("WPNAV_SPEED") && e.params.contains("ArduPilot"));

    let evidence = lines(&e.evidence);
    let statuses: Vec<&Value> = evidence
        .iter()
        .filter(|v| v["topic"] == STATUS)
        .map(|v| &v["message"])
        .collect();
    assert_eq!(
        statuses[0]["header"]["stamp"],
        json!({"sec": 1, "nanosec": 0})
    );
    assert_eq!(statuses.last().unwrap()["armed"], false);
    assert!(
        statuses
            .iter()
            .any(|s| s["flying"] == true && s["mode"] == 4)
    );
    let pose = evidence.iter().find(|v| v["topic"] == POSE).unwrap();
    assert!(near(&pose["message"]["pose"]["position"]["z"], 3.0));
}

#[test]
fn a_hand_takeoff_switched_into_guided_climbs_the_fresh_vehicle_there() {
    let mut b = base("V4.6.3");
    mode(&mut b, 1., 5., 1.);
    arm(&mut b, 2., 1., 0.);
    ev(&mut b, 3., EV_NOT_LANDED as f64);
    pos(&mut b, 5.1, 0.0, 14.2);
    mode(&mut b, 5., 4., 1.);
    guip(&mut b, 6., 0., 100., 0.);
    mode(&mut b, 9., 6., 3.); // radio failsafe: RTL
    let e = export(&parsed(&b), "hand").unwrap();
    let calls: Vec<(String, Value)> = lines(&e.commands)
        .into_iter()
        .map(|v| {
            (
                v["topic"].as_str().unwrap().to_string(),
                v["message"].clone(),
            )
        })
        .collect();
    assert_eq!(
        calls,
        [
            (MODE.to_string(), json!({"mode": 5})),
            (ARM.into(), json!({"arm": true})),
            (MODE.into(), json!({"mode": 4})),
            (ARM.into(), json!({"arm": true})),
            (TAKEOFF.into(), json!({"alt": 14.2})),
            (CMD_VEL.into(), calls[5].1.clone()),
        ],
        "the failsafe's RTL is not a call"
    );
    let failsafe = lines(&e.evidence)
        .into_iter()
        .rfind(|v| v["topic"] == STATUS)
        .unwrap();
    assert_eq!(failsafe["message"]["failsafe"], json!([3]));
}

#[test]
fn later_versions_log_north_east_down_in_metres() {
    assert!(!ned_targets("V4.6.3") && ned_targets("V4.7.0") && ned_targets("V5.0.0-dev"));
    let mut b = base("V4.7.0");
    mode(&mut b, 1., 4., 52.);
    guip(&mut b, 2., 0., 0., 50.);
    let e = export(&parsed(&b), "ned").unwrap();
    let up = lines(&e.commands)
        .into_iter()
        .find(|v| v["topic"] == CMD_VEL)
        .unwrap()["message"]["twist"]["linear"]["z"]
        .as_f64()
        .unwrap();
    assert!((up + 0.5).abs() < 1e-6, "down 0.5 m/s is up -0.5: {up}");
}

#[test]
fn a_flight_without_guided_commands_has_nothing_to_replay() {
    let mut b = base("V4.6.3");
    mode(&mut b, 1., 5., 1.);
    arm(&mut b, 2., 1., 0.);
    assert_eq!(export(&parsed(&b), "manual").unwrap_err(), MANUAL);
}

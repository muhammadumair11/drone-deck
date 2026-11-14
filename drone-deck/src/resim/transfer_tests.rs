use super::*;
use crate::mavlink::ParamValue;

fn set(params: &[(&str, ParamValue, u8)]) -> ParamSet {
    ParamSet {
        params: params
            .iter()
            .map(|(n, value, kind)| {
                (
                    n.to_string(),
                    Param {
                        name: n.to_string(),
                        value: *value,
                        kind: *kind,
                    },
                )
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn hardware_and_launch_parameters_are_never_copied() {
    for name in [
        "CAL_ACC0_XOFF",
        "SENS_BOARD_ROT",
        "PWM_MAIN_FUNC1",
        "SER_TEL1_BAUD",
        "BAT1_N_CELLS",
        "BATT_CAPACITY",
        "MAV_0_CONFIG",
        "COMPASS_OFS_X",
        "RC1_MIN",
        "SERIAL2_BAUD",
    ] {
        assert_eq!(skip(name, true), Some(Skip::Hardware), "{name}");
    }
    assert_eq!(skip("SYS_AUTOSTART", true), Some(Skip::Launch));
    assert_eq!(
        skip("UXRCE_DDS_DOM_ID", true),
        Some(Skip::Launch),
        "the run's own isolated domain"
    );
    for name in [
        "MPC_XY_VEL_MAX",
        "MAV_TYPE",
        "MAV_PROTO_VER",
        "RCL_EXCEPT",
        "NAV_RCL_ACT",
        "COM_RC_IN_MODE",
    ] {
        assert_eq!(skip(name, true), None, "{name}");
    }
}

#[test]
fn simulator_parameters_follow_only_the_same_world() {
    assert_eq!(skip("SIM_GZ_EN", true), None);
    assert_eq!(skip("SIM_GZ_EN", false), Some(Skip::OtherSimulator));
    assert_eq!(skip("SIH_MASS", false), Some(Skip::OtherSimulator));
    assert_eq!(skip("MPC_XY_VEL_MAX", false), None);
}

#[test]
fn only_changed_known_parameters_are_written_with_the_fresh_types() {
    let recorded = set(&[
        ("MPC_XY_VEL_MAX", ParamValue::Real(8.0), 9),
        ("MIS_TAKEOFF_ALT", ParamValue::Real(2.5), 9),
        ("NAV_DLL_ACT", ParamValue::Int(2), 6),
        ("CAL_GYRO0_ID", ParamValue::Int(1), 6),
        ("OLD_PARAM", ParamValue::Int(1), 6),
    ]);
    let fresh = set(&[
        ("MPC_XY_VEL_MAX", ParamValue::Real(12.0), 9),
        ("MIS_TAKEOFF_ALT", ParamValue::Real(2.5), 9),
        ("NAV_DLL_ACT", ParamValue::Int(0), 2),
    ]);
    let t = plan(&recorded, Some(&fresh), true);
    let names: Vec<(&str, u8)> = t.apply.iter().map(|p| (p.name.as_str(), p.kind)).collect();
    assert_eq!(names, [("MPC_XY_VEL_MAX", 9), ("NAV_DLL_ACT", 2)]);
    assert_eq!(t.unchanged, 1);
    assert_eq!(
        t.skipped,
        [
            ("CAL_GYRO0_ID".to_string(), Skip::Hardware),
            ("OLD_PARAM".to_string(), Skip::Unknown)
        ]
    );

    let blind = plan(&recorded, None, true);
    assert_eq!(
        blind.apply.len(),
        4,
        "without a fresh set, everything allowed is written"
    );
    assert!(Skip::Unknown.reason().contains("autopilot"));
}

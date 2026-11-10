//! Headless scenes for the physical tools (`physical-params`, `physical-logs`): a fake
//! read-only link with parameters, a loaded comparison file and onboard logs. Nothing here
//! opens a socket or reads a file.
use super::Baseline;
use crate::mavlink::{LogEntry, Param, ParamSet, ParamValue};
use crate::{State, systems, vehicle};
use std::path::PathBuf;

/// PX4 parameters of a quadcopter: name, value, MAV_PARAM_TYPE (6 int32, 9 real32).
const PARAMS: &[(&str, f64, u8)] = &[
    ("BAT1_N_CELLS", 4., 6),
    ("BAT1_V_CHARGED", 4.2, 9),
    ("BAT1_V_EMPTY", 3.5, 9),
    ("CBRK_SUPPLY_CHK", 0., 6),
    ("COM_ARM_WO_GPS", 1., 6),
    ("COM_DISARM_LAND", 2., 9),
    ("COM_LOW_BAT_ACT", 0., 6),
    ("COM_RC_LOSS_T", 0.5, 9),
    ("EKF2_GPS_CTRL", 7., 6),
    ("EKF2_HGT_REF", 1., 6),
    ("EKF2_MAG_TYPE", 0., 6),
    ("GF_ACTION", 1., 6),
    ("GF_MAX_HOR_DIST", 0., 9),
    ("MAV_SYS_ID", 1., 6),
    ("MAV_TYPE", 2., 6),
    ("MIS_TAKEOFF_ALT", 2.5, 9),
    ("MPC_THR_HOVER", 0.5, 9),
    ("MPC_XY_VEL_MAX", 12., 9),
    ("MPC_Z_VEL_MAX_DN", 1.5, 9),
    ("MPC_Z_VEL_MAX_UP", 3., 9),
    ("NAV_DLL_ACT", 0., 6),
    ("NAV_RCL_ACT", 2., 6),
    ("RTL_DESCEND_ALT", 30., 9),
    ("RTL_RETURN_ALT", 60., 9),
    ("SDLOG_MODE", 0., 6),
    ("SDLOG_PROFILE", 1., 6),
    ("SENS_BOARD_ROT", 0., 6),
    ("SYS_AUTOSTART", 4001., 6),
    ("SYS_HAS_MAG", 1., 6),
];

fn set(params: impl IntoIterator<Item = (&'static str, f64, u8)>) -> ParamSet {
    let mut set = ParamSet {
        system_id: 1,
        component_id: 1,
        stack: "PX4 Pro".into(),
        vehicle: "Multi-Rotor".into(),
        ..Default::default()
    };
    for (name, value, kind) in params {
        let value = if kind == 9 {
            ParamValue::Real(value)
        } else {
            ParamValue::Int(value as i64)
        };
        set.params.insert(
            name.into(),
            Param {
                name: name.into(),
                value,
                kind,
            },
        );
    }
    set
}

/// The vehicle's parameters for scenes and tests.
pub(crate) fn fake_params() -> ParamSet {
    set(PARAMS.iter().copied())
}

/// A file saved before a change: three values differ, one parameter is new on the vehicle and
/// one was removed from it.
pub(crate) fn fake_baseline() -> ParamSet {
    set(PARAMS
        .iter()
        .copied()
        .filter(|(n, _, _)| *n != "SDLOG_PROFILE")
        .map(|(n, v, k)| match n {
            "MPC_XY_VEL_MAX" => (n, 10., k),
            "RTL_RETURN_ALT" => (n, 30., k),
            "COM_ARM_WO_GPS" => (n, 0., k),
            _ => (n, v, k),
        })
        .chain([("CAL_MAG0_ID", 396_825., 6)]))
}

fn physical(s: &mut State) {
    s.select_system(systems::PHYSICAL);
    let id = s.systems.active_id().to_string();
    vehicle::preview(s, &id, true, false);
}

/// `physical-params`: fetched parameters compared with a loaded export.
pub(crate) fn preview_params(s: &mut State) {
    physical(s);
    s.vehicle.params = Some(fake_params());
    let path =
        PathBuf::from("workspace/exports/params/physical-vehicle-2026-11-02_14-20-00.params");
    s.hardware.params_file = rune::TextEdit::new(path.display().to_string());
    s.hardware.baseline = Some(Baseline {
        path,
        set: fake_baseline(),
        bad: vec![7],
    });
    s.hardware.exported = Some(PathBuf::from(
        "workspace/exports/params/physical-vehicle-2026-11-10_09-41-07.params",
    ));
}

/// `physical-logs`: a listed vehicle with one log downloading and one already downloaded.
pub(crate) fn preview_logs(s: &mut State) {
    physical(s);
    s.vehicle.logs = vec![
        LogEntry {
            id: 1,
            size: 1_482_112,
            time_utc: 1_793_000_000,
        },
        LogEntry {
            id: 2,
            size: 3_407_872,
            time_utc: 1_793_090_400,
        },
        LogEntry {
            id: 3,
            size: 12_582_912,
            time_utc: 1_793_174_200,
        },
        LogEntry {
            id: 4,
            size: 220_160,
            time_utc: 0,
        },
    ];
    s.vehicle.logs_complete = true;
    s.vehicle.log_progress = Some((3, 4_718_592, 12_582_912));
    s.vehicle.downloaded = Some(PathBuf::from(
        "workspace/logs/vehicle/log_2_2026-10-27_08-40-00.ulg",
    ));
}

//! Readiness from PX4's health and arming check report, ahead of SYS_STATUS and STATUSTEXT.
use super::*;
use crate::mavlink::HealthReport;
use crate::{vehicle, view};
use rune::{Env, Runtime};
use std::time::SystemTime;

const GPS: u32 = 1 << 3;
const BATTERY: u32 = 1 << 11;
const MANUAL: u32 = 1;
const POSITION: u32 = 1 << 2;

fn px4(main: u32, sub: u32) -> VehicleState {
    let mut v = vehicle::fake_snapshot(true).vehicle.expect("vehicle");
    v.armed = false;
    v.base_mode = 1;
    v.custom_mode = (main << 16) | (sub << 24);
    v
}

fn with_report(v: &mut VehicleState, r: HealthReport) {
    v.health_events.report = Some(r);
}

#[test]
fn the_event_report_decides_readiness_for_the_current_mode() {
    let mut v = px4(3, 0);
    // SYS_STATUS says ready and a STATUSTEXT says not: the stricter event report wins.
    v.sensors_present |= PREARM_BIT;
    v.sensors_enabled |= PREARM_BIT;
    v.sensors_health |= PREARM_BIT;
    let fail = StatusText {
        severity: 4,
        text: "Preflight Fail: Accel uncalibrated".into(),
        at: SystemTime::now(),
    };
    with_report(
        &mut v,
        HealthReport {
            arming_errors: GPS,
            arming_warnings: GPS | BATTERY,
            can_arm: MANUAL,
            ..Default::default()
        },
    );
    let Readiness::Checks(c) = readiness(&v, &[&fail]) else {
        panic!("the report is used");
    };
    assert_eq!(
        c,
        Checks {
            mode: Some("Position"),
            can_arm: Some(false),
            arm_modes: vec!["Manual"],
            errors: vec!["GPS"],
            warnings: vec!["Battery"],
            prearm: Some(true),
            outdated: false,
        }
    );
    let (line, _) = checks_line(&c);
    assert_eq!(
        line,
        "Not ready to arm in Position / errors: GPS / warnings: Battery / can arm in Manual \
         / SYS_STATUS pre-arm check passes, the stricter report is shown"
    );
    assert_eq!(
        source(&v, &Readiness::Checks(c)),
        "PX4 health and arming check report (EVENT), disagrees with SYS_STATUS"
    );

    with_report(
        &mut v,
        HealthReport {
            can_arm: MANUAL | POSITION,
            ..Default::default()
        },
    );
    let Readiness::Checks(c) = readiness(&v, &[]) else {
        panic!("report");
    };
    assert_eq!(
        checks_line(&c).0,
        "Ready to arm in Position / no arming check errors or warnings"
    );
    v.armed = true;
    assert_eq!(readiness(&v, &[]), Readiness::Armed);
}

#[test]
fn a_mode_the_report_does_not_cover_is_not_called_ready() {
    // PX4's "Ready" sub mode has no mode group.
    let mut v = px4(4, 1);
    with_report(
        &mut v,
        HealthReport {
            can_arm: MANUAL,
            ..Default::default()
        },
    );
    let Readiness::Checks(c) = readiness(&v, &[]) else {
        panic!("report");
    };
    assert_eq!(c.can_arm, None);
    let (line, tone) = checks_line(&c);
    assert!(
        line.starts_with("Can arm in Manual / the report does not cover"),
        "{line}"
    );
    assert_eq!(tone, warn());
    with_report(&mut v, HealthReport::default());
    let Readiness::Checks(c) = readiness(&v, &[]) else {
        panic!("report");
    };
    assert!(
        checks_line(&c)
            .0
            .starts_with("Not ready to arm in any mode")
    );
}

#[test]
fn without_a_report_the_older_sources_still_apply() {
    let mut v = px4(4, 3);
    assert_eq!(readiness(&v, &[]), Readiness::NotReported);
    assert_eq!(source(&v, &Readiness::NotReported), "Nothing received yet");
    v.sensors_present |= PREARM_BIT;
    assert_eq!(readiness(&v, &[]), Readiness::NotReady(None));
    assert_eq!(
        source(&v, &Readiness::NotReady(None)),
        "SYS_STATUS pre-arm check bit"
    );
}

#[test]
fn the_health_half_names_components_with_errors() {
    let r = HealthReport {
        present: GPS | BATTERY | 1,
        health_errors: GPS,
        health_warnings: GPS | BATTERY,
        ..Default::default()
    };
    let (line, tone) = report_line(&r, false);
    assert_eq!(
        line,
        "3 components reported / errors: GPS / warnings: Battery"
    );
    assert_eq!(tone, danger());
    let clean = HealthReport {
        present: GPS,
        ..Default::default()
    };
    let (line, tone) = report_line(&clean, false);
    assert_eq!(line, "1 component reported / no errors or warnings");
    assert_eq!(tone, fg());
    let (line, tone) = report_line(&clean, true);
    assert_eq!(
        line,
        "1 component reported / no errors or warnings / events were missed since, it may be out of date"
    );
    assert_eq!(tone, warn());
}

#[test]
fn a_report_saying_ready_yields_to_a_failing_sys_status_bit() {
    let mut v = px4(3, 0);
    v.sensors_present |= PREARM_BIT;
    with_report(
        &mut v,
        HealthReport {
            can_arm: POSITION,
            ..Default::default()
        },
    );
    let ready = readiness(&v, &[]);
    let Readiness::Checks(c) = &ready else {
        panic!("report");
    };
    let (line, tone) = checks_line(c);
    assert_eq!(
        line,
        "Not ready to arm in Position / SYS_STATUS says the pre-arm checks fail, \
         the check report says ready / no arming check errors or warnings"
    );
    assert_eq!(tone, danger());
    assert_eq!(
        source(&v, &ready),
        "PX4 health and arming check report (EVENT), disagrees with SYS_STATUS"
    );
    // When they agree, the report speaks alone.
    v.sensors_health |= PREARM_BIT;
    let ready = readiness(&v, &[]);
    let Readiness::Checks(c) = &ready else {
        panic!("report");
    };
    assert_eq!(
        checks_line(c),
        (
            "Ready to arm in Position / no arming check errors or warnings".into(),
            success()
        )
    );
    assert_eq!(
        source(&v, &ready),
        "PX4 health and arming check report (EVENT)"
    );
}

#[test]
fn a_mode_without_a_group_takes_the_sys_status_verdict_too() {
    let mut v = px4(4, 1);
    v.sensors_present |= PREARM_BIT;
    with_report(
        &mut v,
        HealthReport {
            can_arm: MANUAL,
            ..Default::default()
        },
    );
    let Readiness::Checks(c) = readiness(&v, &[]) else {
        panic!("report");
    };
    let (line, tone) = checks_line(&c);
    assert!(
        line.ends_with(" / SYS_STATUS pre-arm check fails"),
        "{line}"
    );
    assert_eq!(tone, danger());
    v.sensors_health |= PREARM_BIT;
    let Readiness::Checks(c) = readiness(&v, &[]) else {
        panic!("report");
    };
    let (line, tone) = checks_line(&c);
    assert!(
        line.ends_with(" / SYS_STATUS pre-arm check passes"),
        "{line}"
    );
    assert_eq!(tone, warn());
}

#[test]
fn a_report_with_lost_events_after_it_is_not_trusted_alone() {
    let mut v = px4(3, 0);
    with_report(
        &mut v,
        HealthReport {
            can_arm: POSITION,
            ..Default::default()
        },
    );
    v.health_events.assume_missed();
    // No SYS_STATUS bit: the report is all there is, shown as possibly out of date, not green.
    let Readiness::Checks(c) = readiness(&v, &[]) else {
        panic!("report");
    };
    assert!(c.outdated);
    let (line, tone) = checks_line(&c);
    assert_eq!(
        line,
        "Ready to arm in Position / no arming check errors or warnings \
         / events were missed since this report, so it may be out of date"
    );
    assert_eq!(tone, warn());
    // With a SYS_STATUS bit, the periodic bit decides.
    v.sensors_present |= PREARM_BIT;
    let ready = readiness(&v, &[]);
    assert_eq!(ready, Readiness::NotReady(None));
    assert_eq!(
        source(&v, &ready),
        "SYS_STATUS pre-arm check bit / the EVENT report may be out of date"
    );
    v.sensors_health |= PREARM_BIT;
    assert_eq!(readiness(&v, &[]), Readiness::Ready);
}

/// The `vehicle-health-checks` scene (the fixture) draws the report on the System page.
#[test]
fn the_fixture_renders_the_event_readiness() {
    let mut s = State::new();
    super::super::scenes::scene_health_checks(&mut s);
    s.navigate(crate::ui::Page::System);
    let mut rt = Runtime::new(
        s,
        view,
        (1440., 900.),
        Env {
            system_dark: true,
            scale: 1.,
        },
    );
    rt.frame(0.);
    rt.update(|_, tx| tx.scroll_into_view("vehicle-ready"));
    for _ in 0..10 {
        rt.frame(0.1);
    }
    assert!(rt.presented("health-report").is_some());
    let texts: Vec<String> = rt.elements().into_iter().filter_map(|e| e.text).collect();
    let want = "Not ready to arm in Hold / errors: Magnetometer / warnings: Battery / can arm in Manual, Stabilized";
    assert!(texts.iter().any(|t| t == want), "{texts:?}");
    assert!(
        texts
            .iter()
            .any(|t| t == "PX4 health and arming check report (EVENT)")
    );
    assert!(
        texts
            .iter()
            .any(|t| t == "Read-only / 1 component with errors")
    );
}

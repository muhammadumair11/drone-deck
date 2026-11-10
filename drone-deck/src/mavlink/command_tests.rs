use super::*;

fn px4() -> VehicleState {
    let mut v = VehicleState::new(1, 1, Instant::now());
    v.autopilot = Autopilot::Px4;
    v.vehicle_type = 2;
    v
}

#[test]
fn authority_needs_every_piece_of_evidence() {
    let all = SimulationEvidence {
        app_router_link: true,
        simulator_source: true,
        stack_owned_running: true,
    };
    assert!(Authority::from_evidence(all).may_fly());
    for missing in 0..3 {
        let mut e = all;
        match missing {
            0 => e.app_router_link = false,
            1 => e.simulator_source = false,
            _ => e.stack_owned_running = false,
        }
        assert_eq!(Authority::from_evidence(e), Authority::ReadOnly);
    }
    assert!(!Authority::ReadOnly.may_fly());
}

#[test]
fn takeoff_altitude_follows_the_stack() {
    let mut v = px4();
    v.alt_msl = Some(500.0);
    v.relative_alt = Some(2.0);
    let (cmd, p) = encode(&CommandKind::Takeoff { alt: 10.0 }, &v).expect("takeoff");
    assert_eq!(cmd, dialect::MavCmd::MAV_CMD_NAV_TAKEOFF);
    assert_eq!(p[6], 508.0, "PX4 takes AMSL: home 498 m + 10 m");
    v.alt_msl = None;
    let takeoff = CommandKind::Takeoff { alt: 10.0 };
    assert!(takeoff_needs_home(&takeoff, &v));
    assert_eq!(
        encode(&takeoff, &v),
        None,
        "never NaN: PX4 would pick its own height"
    );
    let mut c = Commands::new(Duration::from_millis(100), 3);
    assert!(matches!(
        c.start(1, takeoff.clone(), Some(&v), Instant::now()),
        CommandStep::Done {
            result: CommandResult::NoHome,
            ..
        }
    ));
    assert!(c.is_empty(), "nothing was sent");
    v.autopilot = Autopilot::ArduPilot;
    assert!(!takeoff_needs_home(&takeoff, &v));
    let (_, p) = encode(&CommandKind::Takeoff { alt: 10.0 }, &v).expect("takeoff");
    assert_eq!(p[6], 10.0, "ArduPilot takes a relative altitude");
}

#[test]
fn hold_maps_to_each_stack() {
    let mut v = px4();
    let (cmd, p) = encode(&CommandKind::Hold, &v).expect("hold");
    assert_eq!(
        (cmd, p[1], p[2]),
        (dialect::MavCmd::MAV_CMD_DO_SET_MODE, 4.0, 3.0)
    );
    v.autopilot = Autopilot::ArduPilot;
    let (_, p) = encode(&CommandKind::Hold, &v).expect("loiter");
    assert_eq!(p[1], 5.0);
    v.vehicle_type = 1;
    assert!(encode(&CommandKind::Hold, &v).is_none());
}

fn ack(cmd: dialect::MavCmd, result: dialect::MavResult) -> dialect::COMMAND_ACK_DATA {
    dialect::COMMAND_ACK_DATA {
        command: cmd,
        result,
        ..Default::default()
    }
}

#[test]
fn tracks_acks_retries_and_timeouts() {
    let t0 = Instant::now();
    let v = px4();
    let mut c = Commands::new(Duration::from_millis(100), 2);
    assert!(matches!(
        c.start(1, CommandKind::Arm, Some(&v), t0),
        CommandStep::Send(_)
    ));
    assert!(matches!(
        c.start(2, CommandKind::Disarm, Some(&v), t0),
        CommandStep::Done {
            result: CommandResult::Busy,
            ..
        }
    ));
    assert!(c.poll(Some(&v), t0 + Duration::from_millis(50)).is_empty());
    let retry = c.poll(Some(&v), t0 + Duration::from_millis(100));
    assert!(matches!(&retry[..], [CommandStep::Send(Msg::COMMAND_LONG(l))] if l.confirmation == 1));
    let out = c.poll(Some(&v), t0 + Duration::from_millis(200));
    assert!(matches!(
        &out[..],
        [CommandStep::Done {
            id: 1,
            result: CommandResult::TimedOut { attempts: 2 },
            ..
        }]
    ));
    assert!(c.is_empty());

    let arm = dialect::MavCmd::MAV_CMD_COMPONENT_ARM_DISARM;
    c.start(3, CommandKind::Arm, Some(&v), t0);
    let progress = ack(arm, dialect::MavResult::MAV_RESULT_IN_PROGRESS);
    assert!(c.on_ack(&progress, t0).is_none());
    assert!(
        c.poll(Some(&v), t0 + Duration::from_millis(150)).is_empty(),
        "in progress waits"
    );
    let done = c.on_ack(&ack(arm, dialect::MavResult::MAV_RESULT_ACCEPTED), t0);
    assert!(matches!(
        done,
        Some(CommandStep::Done {
            id: 3,
            result: CommandResult::Accepted,
            ..
        })
    ));
    let other = ack(
        dialect::MavCmd::MAV_CMD_NAV_LAND,
        dialect::MavResult::MAV_RESULT_ACCEPTED,
    );
    assert!(c.on_ack(&other, t0).is_none());
}

#[test]
fn a_retry_never_goes_to_another_vehicle() {
    let t0 = Instant::now();
    let mut c = Commands::new(Duration::from_millis(100), 3);
    assert!(matches!(
        c.start(1, CommandKind::Arm, Some(&px4()), t0),
        CommandStep::Send(_)
    ));
    let mut other = VehicleState::new(2, 1, t0);
    other.autopilot = Autopilot::Px4;
    other.vehicle_type = 2;
    let out = c.poll(Some(&other), t0 + Duration::from_millis(100));
    assert!(
        matches!(
            &out[..],
            [CommandStep::Done {
                id: 1,
                result: CommandResult::TimedOut { attempts: 1 },
                ..
            }]
        ),
        "the arm confirmed for system 1 must not be resent to system 2"
    );
    assert!(c.is_empty());
}

#[test]
fn no_vehicle_and_unknown_modes_finish_at_once() {
    let mut c = Commands::new(Duration::from_millis(100), 3);
    let now = Instant::now();
    assert!(matches!(
        c.start(1, CommandKind::Arm, None, now),
        CommandStep::Done {
            result: CommandResult::NoVehicle,
            ..
        }
    ));
    assert!(matches!(
        c.start(2, CommandKind::SetMode("Warp".into()), Some(&px4()), now),
        CommandStep::Done {
            result: CommandResult::Unsupported,
            ..
        }
    ));
}

#[test]
fn in_progress_commands_are_never_resent() {
    let t0 = Instant::now();
    let mut v = px4();
    v.alt_msl = Some(500.0);
    v.relative_alt = Some(0.0);
    let mut c = Commands::new(Duration::from_millis(100), 3);
    c.start(1, CommandKind::Takeoff { alt: 5.0 }, Some(&v), t0);
    let takeoff = dialect::MavCmd::MAV_CMD_NAV_TAKEOFF;
    assert!(
        c.on_ack(
            &ack(takeoff, dialect::MavResult::MAV_RESULT_IN_PROGRESS),
            t0
        )
        .is_none()
    );
    // The vehicle has the command: silence after IN_PROGRESS times out without a resend.
    let out = c.poll(Some(&v), t0 + Duration::from_millis(300));
    assert!(
        matches!(
            &out[..],
            [CommandStep::Done {
                id: 1,
                result: CommandResult::TimedOut { attempts: 1 },
                ..
            }]
        ),
        "a resend after IN_PROGRESS would repeat a running command"
    );
}

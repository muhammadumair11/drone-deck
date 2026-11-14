use super::*;

fn seen() -> Seen {
    Seen {
        ready: true,
        pending: false,
        recording: false,
        unconfirmed: false,
        topic: false,
    }
}

#[test]
fn the_stage_checks_waits_for_topics_then_records() {
    let t0 = Instant::now();
    let not_ready = Seen {
        ready: false,
        ..seen()
    };
    assert_eq!(
        decide(Auto::Idle, not_ready, true, t0),
        (Auto::Checking, Move::Ask("check"))
    );
    assert_eq!(
        decide(
            Auto::Checking,
            Seen {
                pending: true,
                ..not_ready
            },
            false,
            t0
        ),
        (Auto::Checking, Move::Wait),
        "nothing moves while a request is out"
    );
    assert_eq!(
        decide(Auto::Checking, seen(), false, t0),
        (Auto::Waiting(None), Move::Wait)
    );
    assert_eq!(
        decide(Auto::Waiting(None), seen(), false, t0),
        (Auto::Waiting(Some(t0)), Move::Ask("topics"))
    );
    let soon = t0 + Duration::from_millis(500);
    assert_eq!(
        decide(Auto::Waiting(Some(t0)), seen(), false, soon),
        (Auto::Waiting(Some(t0)), Move::Wait),
        "the topic list is asked for every two seconds, not every frame"
    );
    let later = t0 + Duration::from_secs(2);
    assert_eq!(
        decide(Auto::Waiting(Some(t0)), seen(), false, later),
        (Auto::Waiting(Some(later)), Move::Ask("topics"))
    );
    let topic = Seen {
        topic: true,
        ..seen()
    };
    assert_eq!(
        decide(Auto::Waiting(Some(t0)), topic, false, soon),
        (Auto::Starting, Move::Ask("record"))
    );
    assert_eq!(
        decide(
            Auto::Starting,
            Seen {
                recording: true,
                ..seen()
            },
            false,
            soon
        ),
        (Auto::Idle, Move::Ready)
    );
    assert_eq!(
        decide(Auto::Starting, seen(), false, soon),
        (Auto::Idle, Move::Ready),
        "confirmed"
    );
}

#[test]
fn a_recorder_that_is_not_ready_or_refuses_fails_the_stage() {
    let t0 = Instant::now();
    let not_ready = Seen {
        ready: false,
        ..seen()
    };
    assert_eq!(
        decide(Auto::Checking, not_ready, false, t0),
        (Auto::Idle, Move::Fail)
    );
    let refused = Seen {
        unconfirmed: true,
        ..seen()
    };
    assert_eq!(
        decide(Auto::Starting, refused, false, t0),
        (Auto::Idle, Move::Fail)
    );
    assert_eq!(
        decide(Auto::Idle, seen(), true, t0),
        (Auto::Waiting(Some(t0)), Move::Ask("topics")),
        "a recorder already checked goes straight to the topics"
    );
}

#[test]
fn the_stage_records_the_systems_saved_topics_else_the_autopilots() {
    let mut s = State::new();
    s.recorder.discovered = ["/fmu/out/vehicle_status_v1", "/rosout", "/camera/image_raw"]
        .iter()
        .map(|n| super::super::Topic {
            name: (*n).into(),
            ..Default::default()
        })
        .collect();
    assert_eq!(wanted(&s), ["/fmu/out/vehicle_status_v1"]);
    let id = s.systems.active_id().to_string();
    s.systems.get_mut(&id).unwrap().consumers.ros.topics = vec!["/camera/image_raw".into()];
    assert_eq!(wanted(&s), ["/camera/image_raw"]);
    assert_eq!(
        stage(&mut s, Step::Start),
        Stage::Failed("Preview: recording processes disabled.".into())
    );
}

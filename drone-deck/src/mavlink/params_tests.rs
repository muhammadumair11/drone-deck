use super::*;

fn value(
    index: u16,
    count: u16,
    name: &str,
    raw: f32,
    kind: dialect::MavParamType,
) -> dialect::PARAM_VALUE_DATA {
    dialect::PARAM_VALUE_DATA {
        param_value: raw,
        param_count: count,
        param_index: index,
        param_id: name.into(),
        param_type: kind,
    }
}

#[test]
fn decodes_integers_per_stack() {
    let int32 = 6;
    assert_eq!(
        decode_value(f32::from_bits(4001), int32, Autopilot::Px4),
        ParamValue::Int(4001)
    );
    assert_eq!(
        decode_value(f32::from_bits(-5i32 as u32), int32, Autopilot::Px4),
        ParamValue::Int(-5)
    );
    assert_eq!(
        decode_value(4001.0, int32, Autopilot::ArduPilot),
        ParamValue::Int(4001)
    );
    assert_eq!(
        decode_value(0.25, 9, Autopilot::Px4),
        ParamValue::Real(0.25)
    );
}

#[test]
fn fetch_fills_gaps_by_index() {
    let t0 = Instant::now();
    let (mut f, first) = ParamFetch::new((1, 1), Autopilot::ArduPilot, t0);
    assert!(matches!(first, Msg::PARAM_REQUEST_LIST(_)));
    let real = dialect::MavParamType::MAV_PARAM_TYPE_REAL32;
    for i in [0u16, 2, 4] {
        assert!(f.on_value(&value(i, 5, &format!("P{i}"), i as f32, real), t0));
    }
    assert_eq!(f.progress(), (3, 5));
    assert_eq!(f.poll(t0), FetchStep::Wait);
    let FetchStep::Send(reads) = f.poll(t0 + f.idle) else {
        panic!("expected reads")
    };
    let asked: Vec<i16> = reads
        .iter()
        .map(|m| match m {
            Msg::PARAM_REQUEST_READ(r) => r.param_index,
            _ => -1,
        })
        .collect();
    assert_eq!(asked, vec![1, 3]);
    let t1 = t0 + f.idle;
    f.on_value(&value(1, 5, "P1", 1.0, real), t1);
    // An answer by name (index 65535) still fills its slot when the name is known.
    f.on_value(&value(3, 5, "P3", 3.0, real), t1);
    assert!(!f.on_value(&value(u16::MAX, 5, "P3", 3.5, real), t1));
    assert_eq!(f.poll(t1), FetchStep::Complete);
    let set = f.into_set("ArduPilot", "Multi-Rotor");
    assert_eq!(set.params.len(), 5);
    assert_eq!(set.params["P3"].value, ParamValue::Real(3.5));
}

#[test]
fn fetch_gives_up_without_progress() {
    let t0 = Instant::now();
    let (mut f, _) = ParamFetch::new((1, 1), Autopilot::Px4, t0);
    let mut t = t0;
    let mut list_requests = 0;
    loop {
        t += f.idle;
        match f.poll(t) {
            FetchStep::Send(_) => list_requests += 1,
            FetchStep::Incomplete(missing) => {
                assert!(missing.is_empty());
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(list_requests, 2, "re-asks for the list twice, then stops");

    let (mut f, _) = ParamFetch::new((1, 1), Autopilot::Px4, t0);
    f.on_value(
        &value(0, 3, "A", 1.0, dialect::MavParamType::MAV_PARAM_TYPE_REAL32),
        t0,
    );
    let mut t = t0;
    let mut rounds = 0;
    loop {
        t += f.idle;
        match f.poll(t) {
            FetchStep::Send(_) => rounds += 1,
            FetchStep::Incomplete(missing) => {
                assert_eq!(missing, vec![1, 2]);
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(rounds, f.max_rounds);
}

fn set(entries: &[(&str, ParamValue, u8)]) -> ParamSet {
    ParamSet {
        system_id: 1,
        component_id: 1,
        stack: "PX4 Pro".into(),
        vehicle: "Multi-Rotor".into(),
        params: entries
            .iter()
            .map(|(n, v, k)| {
                (
                    n.to_string(),
                    Param {
                        name: n.to_string(),
                        value: *v,
                        kind: *k,
                    },
                )
            })
            .collect(),
    }
}

#[test]
fn qgc_format_round_trips() {
    let s = set(&[
        ("MPC_XY_VEL_MAX", ParamValue::Real(12.0), 9),
        ("SYS_AUTOSTART", ParamValue::Int(10016), 6),
        ("NEG", ParamValue::Int(-3), 6),
    ]);
    let text = s.to_qgc();
    assert!(text.starts_with("# Onboard parameters for Vehicle 1\n"));
    assert!(text.contains("# Vehicle-Id Component-Id Name Value Type\n"));
    assert!(text.contains("1\t1\tMPC_XY_VEL_MAX\t12.000000000000000000\t9\n"));
    assert!(text.contains("1\t1\tSYS_AUTOSTART\t10016\t6\n"));
    let (back, bad) = ParamSet::from_qgc(&format!("{text}garbage line\n"));
    assert_eq!(bad, vec![text.lines().count() + 1]);
    assert_eq!(back, s);
}

#[test]
fn diff_reports_added_removed_and_changed() {
    let a = set(&[
        ("A", ParamValue::Real(1.0), 9),
        ("B", ParamValue::Int(2), 6),
        ("C", ParamValue::Real(0.1), 9),
    ]);
    let b = set(&[
        ("A", ParamValue::Real(1.5), 9),
        ("C", ParamValue::Real(0.1f32 as f64), 9),
        ("D", ParamValue::Int(4), 6),
    ]);
    let changes = diff(&a, &b);
    let names: Vec<&str> = changes.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["A", "B", "D"], "C is equal at float precision");
    assert_eq!(changes[1].after, None);
    assert_eq!(changes[2].before, None);
}

#[test]
fn encoding_is_the_inverse_of_decoding() {
    for (value, kind) in [
        (ParamValue::Int(4001), 6),
        (ParamValue::Int(-5), 6),
        (ParamValue::Int(-3), 2),
        (ParamValue::Int(200), 1),
        (ParamValue::Int(-1200), 4),
        (ParamValue::Int(60000), 3),
        (ParamValue::Int(3_000_000_000), 5),
        (ParamValue::Real(0.25), 9),
    ] {
        for autopilot in [Autopilot::Px4, Autopilot::ArduPilot] {
            let raw = encode_value(&value, kind, autopilot);
            assert_eq!(
                decode_value(raw, kind, autopilot),
                value,
                "{value:?} {kind} {autopilot:?}"
            );
        }
    }
    assert_eq!(
        encode_value(&ParamValue::Int(4001), 6, Autopilot::Px4).to_bits(),
        4001
    );
    assert_eq!(
        encode_value(&ParamValue::Int(4001), 6, Autopilot::ArduPilot),
        4001.0
    );
}

fn param(name: &str, value: i64) -> Param {
    Param {
        name: name.into(),
        value: ParamValue::Int(value),
        kind: 6,
    }
}

fn sent_name(step: &WriteStep) -> String {
    match step {
        WriteStep::Send(Msg::PARAM_SET(p)) => super::super::frame::text(&p.param_id),
        other => panic!("expected a PARAM_SET, got {other:?}"),
    }
}

#[test]
fn a_write_sends_one_parameter_at_a_time_and_waits_for_its_value() {
    let t0 = Instant::now();
    let (mut w, step) = ParamWrite::new(
        (1, 1),
        Autopilot::Px4,
        vec![param("A", 1), param("B", 2)],
        t0,
    );
    assert_eq!(sent_name(&step), "A");
    let int32 = dialect::MavParamType::MAV_PARAM_TYPE_INT32;
    assert!(
        w.on_value(&value(0, 2, "B", f32::from_bits(2), int32), t0)
            .is_none(),
        "not the one being written"
    );
    assert!(
        w.on_value(&value(0, 2, "A", f32::from_bits(9), int32), t0)
            .is_none(),
        "the old value"
    );
    let step = w
        .on_value(&value(0, 2, "A", f32::from_bits(1), int32), t0)
        .unwrap();
    assert_eq!(sent_name(&step), "B");
    assert_eq!(
        w.on_value(&value(1, 2, "B", f32::from_bits(2), int32), t0),
        Some(WriteStep::Done {
            applied: 2,
            failed: vec![]
        })
    );
    assert_eq!(w.poll(t0 + Duration::from_secs(5)), WriteStep::Wait);
    assert_eq!(w.next_deadline(), None);
}

#[test]
fn an_unconfirmed_parameter_is_resent_then_reported() {
    let t0 = Instant::now();
    let (mut w, _) = ParamWrite::new(
        (1, 1),
        Autopilot::Px4,
        vec![param("A", 1), param("B", 2)],
        t0,
    );
    let step = Duration::from_millis(800);
    assert_eq!(w.poll(t0 + step / 2), WriteStep::Wait);
    assert_eq!(sent_name(&w.poll(t0 + step)), "A", "second try");
    assert_eq!(sent_name(&w.poll(t0 + step * 2)), "A", "third try");
    assert_eq!(sent_name(&w.poll(t0 + step * 3)), "B", "gave up on A");
    let int32 = dialect::MavParamType::MAV_PARAM_TYPE_INT32;
    assert_eq!(
        w.on_value(&value(1, 2, "B", f32::from_bits(2), int32), t0 + step * 3),
        Some(WriteStep::Done {
            applied: 1,
            failed: vec!["A".into()]
        })
    );
    let (_, empty) = ParamWrite::new((1, 1), Autopilot::Px4, vec![], t0);
    assert_eq!(
        empty,
        WriteStep::Done {
            applied: 0,
            failed: vec![]
        }
    );
}

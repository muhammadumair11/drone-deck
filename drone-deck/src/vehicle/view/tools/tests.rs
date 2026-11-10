use super::params::{Mark, rows};
use crate::hardware::{self, preview_logs, preview_params};
use crate::mavlink::{ParamSet, ParamValue};
use crate::ui::Page;
use crate::{State, TextEdit, vehicle};
use rune::{Env, Runtime};
use std::path::PathBuf;

fn runtime(w: f32, h: f32, scene: fn(&mut State)) -> Runtime<State> {
    let mut state = State::new();
    scene(&mut state);
    let mut rt = Runtime::new(
        state,
        crate::view,
        (w, h),
        Env {
            scale: 1.,
            system_dark: true,
        },
    );
    rt.frame(0.);
    rt
}
fn show(rt: &mut Runtime<State>, key: &str) {
    let target = key.to_string();
    rt.update(move |_, tx| tx.scroll_into_view(&target));
    for _ in 0..10 {
        rt.frame(0.1);
    }
}
fn tap(rt: &mut Runtime<State>, key: &str) {
    show(rt, key);
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("{key} is not on screen"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
}
fn texts(rt: &Runtime<State>) -> Vec<String> {
    rt.elements().into_iter().filter_map(|e| e.text).collect()
}
fn fake() -> (ParamSet, ParamSet) {
    (
        crate::hardware::fake_params(),
        crate::hardware::fake_baseline(),
    )
}

#[test]
fn export_load_and_diff_round_trip() {
    let mut s = State::new();
    let (params, _) = fake();
    s.vehicle.params = Some(params.clone());
    s.vehicle.owner = Some(format!("b3-roundtrip-{}", std::process::id()));
    let path = vehicle::export_params(&s).expect("export");
    assert!(path.starts_with(hardware::export_dir()));
    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("params"));

    // A quoted path, as Explorer's "Copy as path" gives it.
    s.hardware.params_file = TextEdit::new(format!("\"{}\"", path.display()));
    hardware::load_baseline(&mut s);
    let b = s.hardware.baseline.as_ref().expect("loaded");
    assert_eq!(b.set.params, params.params, "every value survives the file");
    assert!(b.bad.is_empty());
    assert!(
        s.hardware.params_changed,
        "with vehicle parameters, a loaded file shows the changes first"
    );
    assert!(rows(Some(&params), Some(&b.set), "", true).is_empty());

    let mut changed = params.clone();
    changed.params.get_mut("MPC_XY_VEL_MAX").unwrap().value = ParamValue::Real(8.);
    let diff = rows(Some(&changed), Some(&b.set), "", true);
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].name, "MPC_XY_VEL_MAX");
    assert_eq!(diff[0].mark, Mark::Changed);
    assert_eq!(diff[0].vehicle, Some(ParamValue::Real(8.)));
    assert_eq!(diff[0].file, Some(ParamValue::Real(12.)));
    std::fs::remove_file(&path).ok();

    hardware::clear_baseline(&mut s);
    assert!(s.hardware.baseline.is_none() && !s.hardware.params_changed);
}

#[test]
fn rows_mark_changed_added_and_removed_and_filter_by_search() {
    let (vehicle, file) = fake();
    let all = rows(Some(&vehicle), Some(&file), "", false);
    assert_eq!(all.len(), vehicle.params.len() + 1, "union of both sides");
    let diff = rows(Some(&vehicle), Some(&file), "", true);
    let marks: Vec<(&str, Mark)> = diff.iter().map(|r| (r.name.as_str(), r.mark)).collect();
    assert_eq!(
        marks,
        [
            ("CAL_MAG0_ID", Mark::OnlyFile),
            ("COM_ARM_WO_GPS", Mark::Changed),
            ("MPC_XY_VEL_MAX", Mark::Changed),
            ("RTL_RETURN_ALT", Mark::Changed),
            ("SDLOG_PROFILE", Mark::OnlyVehicle),
        ]
    );
    let rtl: Vec<String> = rows(Some(&vehicle), Some(&file), " rtl ", false)
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(rtl, ["RTL_DESCEND_ALT", "RTL_RETURN_ALT"]);
    let alone = rows(Some(&vehicle), None, "", true);
    assert_eq!(alone.len(), vehicle.params.len(), "nothing to compare with");
    assert!(
        alone
            .iter()
            .all(|r| r.mark == Mark::Plain && r.file.is_none())
    );
    let file_only = rows(None, Some(&file), "", false);
    assert!(
        file_only
            .iter()
            .all(|r| r.vehicle.is_none() && r.file.is_some())
    );
}

#[test]
fn loading_refuses_missing_and_unreadable_files_and_reports_bad_lines() {
    let mut s = State::new();
    hardware::load_baseline(&mut s);
    assert!(s.notice.contains("Enter the path"));
    s.hardware.params_file = TextEdit::new("Z:/no/such/file.params");
    hardware::load_baseline(&mut s);
    assert!(s.notice.starts_with("Could not read"), "{}", s.notice);
    assert!(s.hardware.baseline.is_none());

    let dir = std::env::temp_dir().join(format!("drone-deck-b3-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let junk = dir.join("junk.params");
    std::fs::write(&junk, "not a parameter\nfile at all\n").unwrap();
    s.hardware.params_file = TextEdit::new(junk.display().to_string());
    hardware::load_baseline(&mut s);
    assert!(s.notice.starts_with("No parameters found"), "{}", s.notice);
    assert!(s.hardware.baseline.is_none());

    let mixed = dir.join("mixed.params");
    std::fs::write(
        &mixed,
        "# Onboard parameters\n1\t1\tMAV_SYS_ID\t1\t6\nbroken line\n1\t1\tMIS_TAKEOFF_ALT\t2.5\t9\n",
    )
    .unwrap();
    s.hardware.params_file = TextEdit::new(mixed.display().to_string());
    hardware::load_baseline(&mut s);
    let b = s.hardware.baseline.as_ref().expect("loaded");
    assert_eq!(b.set.params.len(), 2);
    assert_eq!(b.bad, [3]);
    assert!(
        !s.hardware.params_changed,
        "without vehicle parameters there is nothing to filter"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Every control on the physical tools and connection panel reads, exports, compares,
/// downloads or navigates; none writes to the vehicle.
#[test]
fn physical_tools_expose_no_write_control() {
    const ALLOWED: &[&str] = &[
        "Disconnect",
        "Connect read-only",
        "Find serial devices",
        "Save to system",
        "Open capture folder ↗",
        "Fetch parameters",
        "Fetch again",
        "Export .params",
        "Open exports folder ↗",
        "Compare later with this file",
        "Load and compare",
        "Clear comparison",
        "Changed only",
        "List logs",
        "List again",
        "Download",
        "Cancel download",
        "Open in Flight logs →",
        "Show in folder ↗",
    ];
    for scene in [preview_params as fn(&mut State), preview_logs] {
        let mut rt = runtime(1440., 900., scene);
        for key in ["hardware-workspace", "vehicle-tools"] {
            show(&mut rt, key);
            let mut seen = 0;
            for e in rt.elements() {
                let inside = e.path.split('/').any(|p| p.contains(key));
                if !inside || !matches!(e.role, "button" | "checkbox") {
                    continue;
                }
                let label = e.label.unwrap_or_default();
                assert!(
                    ALLOWED.contains(&label.as_str()),
                    "unexpected control {label:?} in {key}"
                );
                seen += 1;
            }
            assert!(seen > 2, "{key} has controls");
        }
        assert!(rt.find("flight-actions").is_none());
    }
    for name in ["mod.rs", "params.rs", "logs.rs"] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/vehicle/view/tools")
            .join(name);
        let source = std::fs::read_to_string(path).unwrap();
        for call in ["vehicle::command", "console_send", "PARAM_SET", "LOG_ERASE"] {
            assert!(
                !source.contains(&format!("{call}(")) && !source.contains(&format!("{call} ")),
                "{name} calls {call}"
            );
        }
    }
}

#[test]
fn params_scene_shows_the_comparison_and_filters_changed_rows() {
    let mut rt = runtime(1440., 900., preview_params);
    show(&mut rt, "params-table");
    for key in [
        "params-summary",
        "params-exported",
        "params-baseline",
        "params-changed",
        "param-MPC_XY_VEL_MAX",
    ] {
        assert!(rt.find(key).is_some(), "missing {key}");
    }
    assert!(
        rt.find("param-BAT1_N_CELLS").is_some(),
        "all rows by default"
    );
    let all = texts(&rt);
    assert!(
        all.iter()
            .any(|t| t.contains("3 changed, 1 only in file, 1 only on vehicle"))
    );
    assert!(all.iter().any(|t| t.contains("line(s) did not parse")));

    tap(&mut rt, "params-changed");
    assert!(rt.state.hardware.params_changed);
    show(&mut rt, "params-table");
    assert!(rt.find("param-BAT1_N_CELLS").is_none());
    assert!(rt.find("param-CAL_MAG0_ID").is_some());
    assert!(texts(&rt).iter().any(|t| t == "5 shown"));

    rt.update(|s, _| s.hardware.params_search = TextEdit::new("rtl"));
    show(&mut rt, "params-table");
    assert!(rt.find("param-RTL_RETURN_ALT").is_some());
    assert!(rt.find("param-MPC_XY_VEL_MAX").is_none());

    tap(&mut rt, "params-compare-export");
    assert!(
        rt.state
            .hardware
            .params_file
            .text
            .ends_with("09-41-07.params")
    );
}

#[test]
fn fetch_progress_renders_and_blocks_a_second_fetch() {
    let mut rt = runtime(1440., 900., preview_params);
    rt.update(|s, _| s.vehicle.params_progress = Some((412, 1043)));
    show(&mut rt, "params-progress");
    assert!(rt.find("params-bar").is_some());
    assert!(texts(&rt).iter().any(|t| t == "Reading 412 / 1043 (39%)"));
    let fetch = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("params-fetch"))
        .unwrap();
    rt.update(|s, _| s.notice.clear());
    tap(&mut rt, "params-fetch");
    assert!(rt.state.notice.is_empty(), "a disabled Fetch does nothing");
    assert!(fetch.label.as_deref() == Some("Fetch again"));
}

#[test]
fn logs_scene_renders_download_progress_and_opens_flight_logs() {
    let mut rt = runtime(1440., 900., preview_logs);
    show(&mut rt, "logs-progress");
    for key in ["logs-bar", "log-bar-3", "logs-cancel", "logs-downloaded"] {
        assert!(rt.find(key).is_some(), "missing {key}");
    }
    let all = texts(&rt);
    assert!(
        all.iter()
            .any(|t| t.contains("Downloading log 3: 4.5 MiB / 12.0 MiB (37%)"))
    );
    assert!(all.iter().any(|t| t == "No vehicle clock"));
    assert!(
        rt.find("log-download-3").is_none(),
        "the running one shows progress"
    );

    // One download at a time: another Download does nothing while one runs.
    rt.update(|s, _| s.notice.clear());
    tap(&mut rt, "log-download-1");
    assert!(rt.state.notice.is_empty());

    let path = rt.state.vehicle.downloaded.clone().unwrap();
    tap(&mut rt, "logs-open-flight");
    assert_eq!(rt.state.page, Page::Data);
    assert_eq!(rt.state.ui.data_tab, crate::ulog::LOGS_TAB);
    assert_eq!(rt.state.flight_logs.selected, Some(path));
}

#[test]
fn download_and_list_without_a_link_say_why() {
    let mut s = State::new();
    preview_logs(&mut s);
    s.vehicle.log_progress = None;
    hardware::download(&mut s, 1);
    assert!(s.notice.contains("No live link"), "{}", s.notice);
    hardware::download(&mut s, 99);
    assert!(s.notice.contains("no longer listed"), "{}", s.notice);
    hardware::list_logs(&mut s);
    assert!(s.notice.contains("No live link"), "{}", s.notice);
    hardware::fetch(&mut s);
    assert!(s.notice.contains("No live link"), "{}", s.notice);
    hardware::export(&mut s);
    assert!(s.notice.contains("Headless"), "{}", s.notice);
}

#[test]
fn tools_fit_compact_widths() {
    for scene in [preview_params as fn(&mut State), preview_logs] {
        for (w, h) in [(390., 844.), (360., 640.)] {
            let mut rt = runtime(w, h, scene);
            for key in [
                "hardware-readings",
                "vehicle-params",
                "params-table",
                "params-exported",
                "params-search",
                "params-baseline",
                "logs-progress",
                "logs-downloaded",
                "vehicle-logs",
            ] {
                show(&mut rt, key);
                let Some((r, _)) = rt.presented(key) else {
                    continue;
                };
                assert!(
                    r[0] >= -1. && r[0] + r[2] <= w + 1.,
                    "{key} overflows {w}x{h}: {r:?}"
                );
            }
            for root in ["hardware-workspace", "vehicle-tools"] {
                show(&mut rt, root);
                for e in rt.elements() {
                    let inside = e.path.split('/').any(|p| p.contains(root));
                    if inside && e.role != "textbox" && e.rect[2] > 0. {
                        assert!(
                            e.rect[0] + e.rect[2] <= w + 1.,
                            "{} overflows {w}x{h}: {:?}",
                            e.path,
                            e.rect
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn the_tools_only_show_the_active_systems_data() {
    let mut s = State::new();
    preview_params(&mut s);
    s.vehicle.owner = Some("someone-else".into());
    let mut rt = Runtime::new(
        s,
        crate::view,
        (1440., 900.),
        Env {
            scale: 1.,
            system_dark: true,
        },
    );
    rt.frame(0.);
    show(&mut rt, "vehicle-params");
    assert!(rt.find("params-summary").is_none());
    assert!(rt.find("params-empty").is_some());
}

/// The whole read-only path on a loopback fake vehicle, driven through the tool buttons:
/// connect, fetch, export, list, download, open in Flight logs. The vehicle must receive only
/// read requests (the one HOME_POSITION request included): no GCS heartbeat, flight command,
/// console, parameter write or log erase.
#[test]
fn the_tools_read_a_fake_vehicle_end_to_end_and_never_write_to_it() {
    use crate::mavlink::fake_vehicle::{FakeVehicle, is_home_request, wait_for};
    use crate::mavlink::{Msg, dialect};
    use std::time::Duration;
    const LIMIT: Duration = Duration::from_secs(10);

    let fake = FakeVehicle::tcp();
    let mut s = State::new();
    s.headless = false;
    let id = s.systems.create(crate::systems::PHYSICAL, "B3 bench");
    s.select_system(&id);
    // No forwarding peers: every frame the vehicle receives was sent by Drone Deck.
    s.systems.get_mut(&id).unwrap().consumers.qgc = false;
    s.systems.get_mut(&id).unwrap().consumers.scripts_udp = None;
    s.hardware.endpoint = TextEdit::new(format!("tcp:127.0.0.1:{}", fake.addr.port()));
    let mut rt = Runtime::new(
        s,
        crate::view,
        (1440., 900.),
        Env {
            scale: 1.,
            system_dark: true,
        },
    );
    rt.frame(0.);
    tap(&mut rt, "monitor-start");
    assert!(
        hardware::connected(&rt.state),
        "{}",
        rt.state.hardware.message
    );
    fn until(rt: &mut Runtime<State>, what: &str, done: &dyn Fn(&State) -> bool) {
        wait_for(LIMIT, || {
            vehicle::poll(&mut rt.state);
            done(&rt.state).then_some(())
        })
        .unwrap_or_else(|| panic!("{what}: {}", rt.state.notice));
        rt.frame(0.1);
    }
    until(&mut rt, "heartbeat", &|s| {
        s.vehicle.fresh() && s.vehicle.up()
    });

    tap(&mut rt, "params-fetch");
    until(&mut rt, "parameters", &|s| s.vehicle.params.is_some());
    assert_eq!(rt.state.vehicle.params.as_ref().unwrap().params.len(), 50);
    show(&mut rt, "params-summary");
    assert!(
        texts(&rt)
            .iter()
            .any(|t| t.starts_with("50 parameters") && !t.contains("last fetch"))
    );
    tap(&mut rt, "params-export");
    let exported = rt.state.hardware.exported.clone().expect("exported");
    assert!(exported.is_file());
    tap(&mut rt, "params-compare-export");
    tap(&mut rt, "params-load");
    let b = rt.state.hardware.baseline.as_ref().expect("compared");
    assert_eq!(
        rows(rt.state.vehicle.params.as_ref(), Some(&b.set), "", true),
        [],
        "an export compares equal to the vehicle it came from"
    );
    std::fs::remove_file(&exported).ok();

    tap(&mut rt, "logs-list");
    until(&mut rt, "log list", &|s| s.vehicle.logs.len() == 2);
    show(&mut rt, "log-entry-2");
    tap(&mut rt, "log-download-2");
    until(&mut rt, "download", &|s| s.vehicle.downloaded.is_some());
    let file = rt.state.vehicle.downloaded.clone().unwrap();
    assert!(file.starts_with(vehicle::log_dir()));
    assert_eq!(
        std::fs::read(&file).unwrap(),
        fake.with(|f| f.log.clone()),
        "the downloaded log is byte for byte the vehicle's"
    );
    tap(&mut rt, "logs-open-flight");
    assert_eq!(rt.state.page, Page::Data);
    assert_eq!(
        rt.state.flight_logs.selected.as_deref(),
        Some(file.as_path())
    );
    std::fs::remove_file(&file).ok();

    assert_eq!(
        fake.received(is_home_request).len(),
        1,
        "the link asks for HOME_POSITION once"
    );
    let written = fake.received(|m| {
        !is_home_request(m)
            && !matches!(
                m,
                Msg::PARAM_REQUEST_LIST(_)
                    | Msg::PARAM_REQUEST_READ(_)
                    | Msg::LOG_REQUEST_LIST(_)
                    | Msg::LOG_REQUEST_DATA(_)
                    | Msg::LOG_REQUEST_END(_)
            )
    });
    assert!(
        written.is_empty(),
        "the vehicle received more than read requests: {written:?}"
    );
    let ours = fake.received(
        |m| matches!(m, Msg::HEARTBEAT(h) if h.mavtype == dialect::MavType::MAV_TYPE_GCS),
    );
    assert!(ours.is_empty());

    rt.state.navigate(Page::System);
    rt.frame(0.1);
    tap(&mut rt, "monitor-start");
    assert!(rt.state.vehicle.link.is_none());
    show(&mut rt, "params-summary");
    assert!(
        texts(&rt).iter().any(|t| t.contains("from the last fetch")),
        "after disconnect the parameters say they are not live"
    );
}

#[test]
fn saving_an_endpoint_refuses_text_that_is_not_one() {
    let mut s = State::new();
    s.select_system(crate::systems::PHYSICAL);
    let before = hardware::saved_endpoint(&s).map(str::to_string);
    for bad in ["COM4", "udp:0.0.0.0", "serial:COM4:fast", "http://x:80"] {
        s.hardware.endpoint = TextEdit::new(bad);
        hardware::save_endpoint(&mut s);
        assert!(s.hardware.message.contains("is not an endpoint"), "{bad}");
        assert_eq!(hardware::saved_endpoint(&s).map(str::to_string), before);
    }
    s.hardware.endpoint = TextEdit::new("serial:COM7:57600");
    hardware::save_endpoint(&mut s);
    assert_eq!(hardware::saved_endpoint(&s), Some("serial:COM7:57600"));
}

#[test]
fn a_dataflash_download_offers_its_folder_not_flight_logs() {
    let mut rt = runtime(1440., 900., preview_logs);
    rt.update(|s, _| {
        s.vehicle.downloaded = Some(vehicle::log_dir().join("log_3_20261110T094107Z.bin"))
    });
    show(&mut rt, "logs-downloaded");
    assert!(rt.find("logs-show-folder").is_some());
    assert!(
        rt.find("logs-open-flight").is_none(),
        "Flight logs lists ULog files only"
    );
}

use super::client::AgentError;
use super::fake::{self, Emit, eventually};
use super::*;
use crate::backend::{Event, Profile};
use serde_json::json;
use std::{
    sync::{Arc, mpsc},
    thread,
};

fn client(
    handler: impl Fn(&Value, &Emit) + Send + Sync + 'static,
) -> (Arc<AgentClient>, fake::Seen) {
    let (connector, seen) = fake::connector(Arc::new(handler));
    (AgentClient::new("Fake", connector), seen)
}

#[test]
fn replies_match_their_requests_even_out_of_order() {
    let (agent, _) = client(|request, out| {
        let (request, out) = (request.clone(), out.clone());
        let delay = request["params"]["delay"].as_u64().unwrap_or(0);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(delay));
            out.reply(&request, request["params"].clone());
        });
    });
    let slow = agent.request("echo", json!({"delay":150,"n":1}), Duration::from_secs(5));
    let fast = agent.request("echo", json!({"delay":0,"n":2}), Duration::from_secs(5));
    assert_ne!(slow.id(), fast.id());
    assert_eq!(fast.wait().unwrap()["n"], 2);
    assert_eq!(slow.wait().unwrap()["n"], 1);
}

#[test]
fn errors_timeouts_and_cancels_are_distinct_and_late_replies_are_dropped() {
    let (agent, seen) = client(|request, out| match request["method"].as_str() {
        Some("fail") => out.error(request, "Select a recording first."),
        Some("late") => {
            let (request, out) = (request.clone(), out.clone());
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(300));
                out.reply(&request, json!("late"));
            });
        }
        Some("hang") | Some("cancel") => {}
        _ => out.reply(request, json!("ok")),
    });
    assert_eq!(
        agent.call("fail", json!({}), Duration::from_secs(5)),
        Err(AgentError::Remote("Select a recording first.".into()))
    );
    assert_eq!(
        agent.call("late", json!({}), Duration::from_millis(50)),
        Err(AgentError::Timeout)
    );
    let hang = agent.request("hang", json!({}), Duration::from_secs(30));
    let id = hang.id();
    hang.cancel();
    eventually(|| {
        seen.requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r["method"] == "cancel" && r["params"]["id"] == id)
            .then_some(())
    });
    thread::sleep(Duration::from_millis(350));
    // The late reply arrived after its timeout and was dropped; this reply is this request's.
    assert_eq!(
        agent.call("ping", json!({}), Duration::from_secs(5)),
        Ok(json!("ok"))
    );
    let mut ticket = agent.request("hang", json!({}), Duration::from_millis(30));
    assert_eq!(eventually(|| ticket.try_take()), Err(AgentError::Timeout));
    assert!(ticket.try_take().is_none());
}

#[test]
fn events_are_parsed_bounded_and_router_updates_coalesce() {
    let (agent, _) = client(|request, out| {
        let count = request["params"]["lines"].as_u64().unwrap_or(0);
        for i in 0..count {
            out.send(
                json!({"event":"output","key":"sih","stream":"stdout","line":format!("line {i}")}),
            );
        }
        for packets in 1..=5 {
            out.send(json!({"event":"router","state":"forwarding","clients":1,"packets":packets}));
        }
        out.send(json!({"event":"readiness","key":"sih","state":"Ready","detail":"checks passed"}));
        out.send(json!({"event":"exited","key":"sih","code":3}));
        out.send(json!({"event":"exited","key":"old","code":null}));
        out.raw("not json from a library");
        out.reply(request, json!({}));
    });
    let events = agent.subscribe();
    let lines = 2100;
    agent
        .call("emit", json!({"lines":lines}), Duration::from_secs(5))
        .unwrap();
    let mut all = Vec::new();
    eventually(|| {
        all.extend(events.drain(5000));
        all.iter()
            .any(|e| matches!(e, AgentEvent::Log { level, .. } if level == "stdout"))
            .then_some(())
    });
    let routers: Vec<_> = all
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Router(r) => Some(r.packets),
            _ => None,
        })
        .collect();
    assert_eq!(routers, vec![5], "router updates coalesce to the latest");
    let outputs = all
        .iter()
        .filter(|e| matches!(e, AgentEvent::Output { .. }))
        .count() as u64;
    let dropped: u64 = all
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Dropped(n) => Some(*n),
            _ => None,
        })
        .sum();
    assert!(outputs <= 2048);
    assert_eq!(
        outputs + dropped,
        lines,
        "every line is delivered or counted"
    );
    assert!(all.contains(&AgentEvent::Readiness {
        key: "sih".into(),
        state: "Ready".into(),
        detail: "checks passed".into()
    }));
    assert!(all.contains(&AgentEvent::Exited {
        key: "sih".into(),
        code: Some(3)
    }));
    assert!(all.contains(&AgentEvent::Exited {
        key: "old".into(),
        code: None
    }));
}

#[test]
fn an_agent_that_exits_fails_pending_requests_and_the_next_request_reconnects() {
    let (agent, seen) = client(|request, out| match request["method"].as_str() {
        Some("die") => out.close(),
        _ => out.reply(request, json!("ok")),
    });
    let events = agent.subscribe();
    assert_eq!(
        agent.call("ping", json!({}), Duration::from_secs(5)),
        Ok(json!("ok"))
    );
    let hello = eventually(|| agent.hello());
    assert_eq!(hello.home, "/home/pilot");
    assert_eq!(
        agent.call("die", json!({}), Duration::from_secs(5)),
        Err(AgentError::Disconnected)
    );
    let mut all = Vec::new();
    eventually(|| {
        all.extend(events.drain(100));
        all.iter()
            .any(|e| {
                matches!(
                    e,
                    AgentEvent::Disconnected {
                        expected: false,
                        ..
                    }
                )
            })
            .then_some(())
    });
    assert!(!agent.connected());
    assert_eq!(
        agent.call("ping", json!({}), Duration::from_secs(5)),
        Ok(json!("ok"))
    );
    assert_eq!(seen.spawns(), 2);
    eventually(|| {
        all.extend(events.drain(100));
        (all.iter()
            .filter(|e| matches!(e, AgentEvent::Hello(_)))
            .count()
            == 2)
            .then_some(())
    });
    assert_eq!(
        all.iter().filter(|e| **e == AgentEvent::Connected).count(),
        2
    );
    agent.close(Duration::from_secs(2));
    eventually(|| {
        events
            .drain(100)
            .into_iter()
            .any(|e| matches!(e, AgentEvent::Disconnected { expected: true, .. }))
            .then_some(())
    });
    assert!(seen.methods().contains(&"shutdown".to_owned()));
}

#[test]
fn progress_reaches_only_its_request() {
    let (agent, _) = client(|request, out| {
        out.send(json!({"event":"progress","id":request["id"],"message":"Hashing a.mcap"}));
        out.send(json!({"event":"progress","id":999_999,"message":"someone else"}));
        out.reply(request, json!({"done":true}));
    });
    let (tx, rx) = mpsc::channel();
    let reply = agent
        .request_with_progress("data.library", json!({}), Duration::from_secs(5), Some(tx))
        .wait();
    assert_eq!(reply, Ok(json!({"done":true})));
    assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec!["Hashing a.mcap"]);
}

#[test]
fn unavailable_agents_fail_fast_and_headless_never_spawns() {
    let agent = AgentClient::new(
        "Missing",
        Arc::new(|| Err(std::io::Error::other("wsl.exe is not installed"))),
    );
    let error = agent
        .call("hello", json!({}), Duration::from_secs(5))
        .unwrap_err();
    assert!(matches!(error, AgentError::Unavailable(ref e) if e.contains("wsl.exe")));
    assert!(error.to_string().contains("could not start"));
    // Tests never call `enable`, like headless previews: no agent for an unknown distribution.
    assert!(shared("Never-Enabled-Distro").is_none());
}

#[test]
fn linux_paths_follow_the_automount_rule() {
    let path = |p: &str, root: &str| linux_path(Path::new(p), root);
    assert_eq!(
        path("D:\\drone-control-center\\runtime\\agent\\agent.py", "").as_deref(),
        Some("/mnt/d/drone-control-center/runtime/agent/agent.py")
    );
    assert_eq!(
        path("\\\\?\\C:\\Programs\\Drone Deck\\runtime", "/mnt/").as_deref(),
        Some("/mnt/c/Programs/Drone Deck/runtime")
    );
    // A distro with `[automount] root = /` or `/win` mounts drives elsewhere.
    assert_eq!(path("C:\\Data", "/").as_deref(), Some("/c/Data"));
    assert_eq!(path("C:\\Data", "/win").as_deref(), Some("/win/c/Data"));
    assert_eq!(path("C:\\Data", "relative").as_deref(), Some("/mnt/c/Data"));
    assert_eq!(path("\\\\server\\share\\agent.py", ""), None);
    assert_eq!(path("\\\\?\\UNC\\server\\share", ""), None);
    assert_eq!(path("runtime/agent.py", ""), None);
}

#[test]
fn the_agent_starts_in_its_folder_so_wsl_maps_the_drive_mount() {
    let args = agent_args(
        "Ubuntu",
        Path::new("\\\\?\\C:\\Programs\\Drone Deck\\runtime\\agent"),
    );
    assert_eq!(
        args,
        [
            "-d",
            "Ubuntu",
            "--cd",
            "C:\\Programs\\Drone Deck\\runtime\\agent",
            "--exec",
            "python3",
            "-u",
            "agent.py"
        ]
    );
}

/// A process whose exit code takes a while, like wsl.exe tearing down.
struct SlowExit(Box<dyn client::Process>);
impl client::Process for SlowExit {
    fn kill(&mut self) {
        self.0.kill();
    }
    fn exit_code(&mut self, wait: Duration) -> Option<i32> {
        thread::sleep(Duration::from_millis(300));
        self.0.exit_code(wait)
    }
}

#[test]
fn a_reconnect_never_overtakes_the_old_connections_disconnect() {
    let (inner, seen) =
        fake::connector(Arc::new(|request, out| match request["method"].as_str() {
            Some("die") => out.close(),
            _ => out.reply(request, json!("ok")),
        }));
    let connector: Connector = Arc::new(move || {
        let mut spawned = inner()?;
        spawned.process = Box::new(SlowExit(spawned.process));
        Ok(spawned)
    });
    let agent = AgentClient::new("Fake-Order", connector);
    let events = agent.subscribe_filtered(|e| {
        matches!(e, AgentEvent::Connected | AgentEvent::Disconnected { .. })
    });
    assert_eq!(
        agent.call("die", json!({}), Duration::from_secs(5)),
        Err(AgentError::Disconnected)
    );
    // At once, while the old reader still waits for the exit code.
    assert_eq!(
        agent.call("ping", json!({}), Duration::from_secs(5)),
        Ok(json!("ok"))
    );
    assert_eq!(seen.spawns(), 2);
    let mut all = Vec::new();
    eventually(|| {
        all.extend(events.drain(10));
        (all.len() >= 3).then_some(())
    });
    assert_eq!(
        all[..3],
        [
            AgentEvent::Connected,
            AgentEvent::Disconnected {
                code: Some(0),
                expected: false
            },
            AgentEvent::Connected
        ]
    );
}

fn wsl_profile(id: &str) -> Profile {
    let mut p = crate::backend::defaults()
        .profiles
        .into_iter()
        .find(|p| p.id == "sih")
        .unwrap();
    p.id = id.into();
    p.args = vec!["echo hi".into()];
    p
}
fn statuses(
    rx: &mpsc::Receiver<Event>,
    launcher: &mut Launcher,
    out: &mpsc::SyncSender<Event>,
) -> Vec<Event> {
    launcher.poll(out);
    rx.try_iter().collect()
}

#[test]
fn launcher_runs_wsl_profiles_through_the_agent_and_maps_events() {
    let distro = "Fake-Launch";
    let (connector, seen) =
        fake::connector(Arc::new(|request, out| match request["method"].as_str() {
            Some("run") => {
                out.reply(request, json!({"key":request["params"]["key"],"pid":42}));
                let key = request["params"]["key"].clone();
                out.send(json!({"event":"output","key":key,"stream":"stdout",
                    "line":"@readiness {\"state\":\"Forwarding\",\"detail\":\"relay up\"}"}));
                out.send(
                    json!({"event":"output","key":"someone-else","stream":"stdout","line":"x"}),
                );
                out.send(
                    json!({"event":"readiness","key":key,"state":"Ready","detail":"SIH ready"}),
                );
            }
            Some("stop") => {
                out.reply(request, json!({"stopped":true,"running":false}));
                out.send(json!({"event":"exited","key":request["params"]["key"],"code":-15}));
            }
            Some("emit-exit") => out.send(json!({"event":"exited","key":"relay-test","code":0})),
            _ => out.reply(request, json!({})),
        }));
    let agent = install(distro, connector);
    let (out, rx) = mpsc::sync_channel(256);
    let mut launcher = Launcher::default();
    launcher.start(&wsl_profile("relay-test"), distro, &out);
    assert!(launcher.contains("relay-test"));
    let mut events = Vec::new();
    eventually(|| {
        events.extend(statuses(&rx, &mut launcher, &out));
        events
            .iter()
            .any(|e| matches!(e, Event::Readiness(id, s) if id == "relay-test" && s == "Ready"))
            .then_some(())
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Status(id, s) if id == "relay-test" && s == "Running"))
    );
    // Output is only ever a log line: the retired `@readiness` stdout protocol changes no state.
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Log(id, line) if id == "relay-test" && line.starts_with("@readiness ")
    )));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Readiness(_, s) if s == "Forwarding"))
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Log(id, _) if id == "someone-else"))
    );
    let run = seen
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|r| r["method"] == "run")
        .cloned()
        .unwrap();
    assert_eq!(run["params"]["bash"], "echo hi");
    // The agent sets DRONE_DECK_SCRIPTS itself, as its distro mounts the runtime folder.
    assert_eq!(run["params"]["env"], json!({}));
    assert_eq!(run["params"]["readiness"], Value::Null);

    agent.request("emit-exit", json!({}), Duration::from_millis(1));
    let mut events = Vec::new();
    eventually(|| {
        events.extend(statuses(&rx, &mut launcher, &out));
        events
            .iter()
            .any(|e| matches!(e, Event::Status(_, s) if s == "Exited 0"))
            .then_some(())
    });
    assert!(!launcher.contains("relay-test"));

    let mut sih = wsl_profile("sih");
    sih.exclusive = true;
    launcher.start(&sih, distro, &out);
    assert!(launcher.exclusive_running());
    eventually(|| {
        statuses(&rx, &mut launcher, &out)
            .into_iter()
            .any(|e| matches!(e, Event::Status(id, s) if id == "sih" && s == "Running"))
            .then_some(())
    });
    let run = seen
        .requests
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|r| r["method"] == "run")
        .cloned()
        .unwrap();
    assert_eq!(run["params"]["readiness"], "sih");
    assert!(launcher.stop("sih", &out));
    let events = statuses(&rx, &mut launcher, &out);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Status(id, s) if id == "sih" && s == "Stopped"))
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Status(_, s) if s.starts_with("Exited")))
    );
    assert!(
        !launcher.stop("sih", &out),
        "a stopped process is no longer owned"
    );
}

#[test]
fn launcher_reports_failed_runs_and_adopts_leftovers() {
    let distro = "Fake-Adopt";
    let (connector, seen) =
        fake::connector(Arc::new(|request, out| match request["method"].as_str() {
            Some("run") => out.error(
                request,
                "sih is already running from an earlier Drone Deck session.",
            ),
            Some("adopt") => out.reply(request, json!({"key":"gazebo","pid":7,"output":true})),
            Some("input") => out.error(request, "gazebo is not reading its input."),
            _ => out.reply(request, json!({})),
        }));
    install(distro, connector);
    let (out, rx) = mpsc::sync_channel(256);
    let mut launcher = Launcher::default();
    launcher.start(&wsl_profile("sih"), distro, &out);
    let mut events = Vec::new();
    eventually(|| {
        events.extend(statuses(&rx, &mut launcher, &out));
        events
            .iter()
            .any(|e| matches!(e, Event::Status(_, s) if s == "Failed"))
            .then_some(())
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Log(_, l) if l.contains("earlier Drone Deck session")))
    );
    assert!(!launcher.contains("sih"));
    launcher.adopt("gazebo", distro, true, &out);
    eventually(|| {
        statuses(&rx, &mut launcher, &out)
            .into_iter()
            .any(|e| matches!(e, Event::Status(id, s) if id == "gazebo" && s == "Running"))
            .then_some(())
    });
    assert!(launcher.exclusive_running());
    assert!(launcher.input("gazebo", "commander status", &out));
    assert!(
        rx.try_iter()
            .any(|e| matches!(e, Event::Log(_, l) if l.contains("not reading")))
    );
    let input = seen
        .requests
        .lock()
        .unwrap()
        .iter()
        .find(|r| r["method"] == "input")
        .cloned()
        .unwrap();
    assert_eq!(
        input["params"],
        json!({"key":"gazebo","line":"commander status"})
    );
}

#[test]
fn after_an_agent_restart_the_launcher_reattaches_or_reports_what_ended() {
    let distro = "Fake-Reattach";
    let (connector, seen) =
        fake::connector(Arc::new(|request, out| match request["method"].as_str() {
            Some("run") => out.reply(request, json!({"key":request["params"]["key"],"pid":5})),
            Some("die") => out.close(),
            Some("adopt") if request["params"]["key"] == "alive" => {
                out.reply(request, json!({"key":"alive","pid":5,"output":true}));
            }
            Some("adopt") => out.error(request, "ended is no longer running."),
            Some("stop") => out.reply(request, json!({"stopped":true,"running":true})),
            _ => out.reply(request, json!({})),
        }));
    let agent = install(distro, connector);
    let (out, rx) = mpsc::sync_channel(256);
    let mut launcher = Launcher::default();
    for key in ["alive", "ended"] {
        launcher.start(&wsl_profile(key), distro, &out);
    }
    let mut events = Vec::new();
    eventually(|| {
        events.extend(statuses(&rx, &mut launcher, &out));
        (events
            .iter()
            .filter(|e| matches!(e, Event::Status(_, s) if s == "Running"))
            .count()
            == 2)
            .then_some(())
    });
    drop(agent.request("die", json!({}), Duration::from_secs(1)));
    eventually(|| {
        statuses(&rx, &mut launcher, &out)
            .into_iter()
            .any(|e| matches!(e, Event::Log(id, l) if id == "alive" && l.contains("reattached")))
            .then_some(())
    });
    // What the UI does on an unexpected exit: start the agent again.
    agent.connect().unwrap();
    let mut events = Vec::new();
    eventually(|| {
        events.extend(statuses(&rx, &mut launcher, &out));
        let alive = events
            .iter()
            .any(|e| matches!(e, Event::Status(id, s) if id == "alive" && s == "Running"));
        let ended = events
            .iter()
            .any(|e| matches!(e, Event::Status(id, s) if id == "ended" && s == "Exited"));
        (alive && ended).then_some(())
    });
    assert!(launcher.contains("alive"));
    assert!(!launcher.contains("ended"));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Status(_, s) if s == "Failed")),
        "an adopt after a restart that finds nothing is an exit, not a failure"
    );
    assert_eq!(seen.methods().iter().filter(|m| *m == "adopt").count(), 2);
    // A stop the agent could not confirm is never reported as stopped.
    assert!(launcher.stop("alive", &out));
    let events: Vec<_> = rx.try_iter().collect();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Status(id, s) if id == "alive" && s == "Unknown"))
    );
}

#[test]
fn leftovers_are_alive_unattached_rows() {
    let rows = json!([
        {"key":"sih","pid":10,"since":1.5,"alive":true,"attached":false,"output":true,"command":"make px4_sitl_sih"},
        {"key":"relay","pid":11,"alive":true,"attached":true},
        {"key":"dead","pid":12,"alive":false},
        {"pid":13,"alive":true}
    ]);
    assert_eq!(
        state::leftovers_from(&rows),
        vec![Leftover {
            key: "sih".into(),
            pid: 10,
            since: Some(1.5),
            started: None,
            command: "make px4_sitl_sih".into(),
            output: true,
            busy: false,
        }]
    );
}

#[test]
fn leftover_panel_offers_adopt_and_stop() {
    let mut s = crate::State::new();
    s.agent.leftovers = vec![Leftover {
        key: "sih".into(),
        pid: 10,
        command: "make px4_sitl_sih sihsim_quadx".into(),
        output: true,
        ..Default::default()
    }];
    let mut rt = rune::Runtime::new(
        s,
        crate::ui::view,
        (1280., 800.),
        rune::Env {
            system_dark: true,
            scale: 1.,
        },
    );
    rt.frame(0.);
    assert!(rt.find("agent-leftovers").is_some());
    assert!(rt.find("leftover-stop-sih").is_some());
    let (x, y) = rt.find("leftover-adopt-sih").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
    assert!(rt.state.agent.leftovers.is_empty());
    assert_eq!(
        rt.state.status.get("sih").map(String::as_str),
        Some("Starting")
    );
    rt.frame(0.2);
    assert!(rt.find("agent-leftovers").is_none());
}

#[test]
fn reconciliation_reattaches_own_processes_and_lists_the_rest() {
    let mut s = crate::State::new();
    s.status.insert("sih".into(), "Running".into());
    let rows = json!([
        {"key":"sih","pid":10,"alive":true,"attached":false,"output":true},
        {"key":"gazebo","pid":11,"alive":true,"attached":false,"output":false,"command":"make px4_sitl gz_x500"},
        {"key":"dds","pid":12,"alive":false,"attached":false}
    ]);
    state::reconcile(&mut s, &rows);
    assert_eq!(
        s.agent
            .leftovers
            .iter()
            .map(|l| l.key.as_str())
            .collect::<Vec<_>>(),
        vec!["gazebo"]
    );
    assert!(s.notice.contains("1 process from a previous session"));
    assert!(s.logs.iter().any(|l| l.contains("Reattaching to sih")));
    assert!(
        s.logs
            .iter()
            .any(|l| l.contains("Running (previous session): gazebo"))
    );
}

#[test]
fn unexpected_agent_exits_reconnect_a_bounded_number_of_times() {
    let mut s = crate::State::new();
    s.agent.leftovers = vec![Leftover::default()];
    for _ in 0..3 {
        state::apply(
            &mut s,
            AgentEvent::Disconnected {
                code: Some(1),
                expected: false,
            },
        );
        assert!(s.notice.contains("Reconnecting"));
        assert!(s.agent.failed.is_none());
    }
    assert!(s.agent.leftovers.is_empty());
    state::apply(
        &mut s,
        AgentEvent::Disconnected {
            code: Some(1),
            expected: false,
        },
    );
    assert!(s.notice.contains("Linux tools are unavailable"));
    assert!(s.agent.failed.is_some());
    let before = s.notice.clone();
    state::apply(
        &mut s,
        AgentEvent::Disconnected {
            code: None,
            expected: true,
        },
    );
    assert_eq!(s.notice, before, "an expected exit is not announced");
}

#[test]
fn the_agents_home_reaches_flight_log_discovery() {
    let mut s = crate::State::new();
    s.config.distro = "Ubuntu".into();
    s.agent.distro = "Ubuntu".into();
    super::state::apply(
        &mut s,
        AgentEvent::Hello(Hello {
            home: "/home/pilot".into(),
            ..Default::default()
        }),
    );
    assert_eq!(
        crate::ulog::locations(&s).home.as_deref(),
        Some("/home/pilot")
    );
    assert!(s.agent.hello.is_some());
}

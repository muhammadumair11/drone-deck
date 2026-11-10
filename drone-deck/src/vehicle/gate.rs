//! The safety gate: flight commands and the console need evidence that the link is the app's
//! own simulation. Everything else is read-only.
use crate::State;
use crate::mavlink::{Authority, LinkOrigin, SimulationEvidence, is_vehicle_heartbeat};
use crate::systems::{self, Source};

/// Simulator profiles. They share UDP 14550 and TCP 5760, so one runs at a time.
pub(crate) const SIMULATORS: [&str; 3] = ["sih", "gazebo", "ardupilot"];

/// What the app observes about the open link (see `SimulationEvidence`).
pub(crate) fn evidence(s: &State) -> SimulationEvidence {
    let v = &s.vehicle;
    if s.headless && v.link.is_none() {
        return v.preview.unwrap_or_default();
    }
    let (Some(link), Some(owner)) = (&v.link, v.owner.as_deref()) else {
        return SimulationEvidence::default();
    };
    let sys = s.systems.get(owner);
    let ardupilot = sys.is_some_and(|x| matches!(x.source, Source::ArduPilot { .. }));
    // PX4 is reached through the router this app started, only while it reports itself up
    // (after an agent restart something else could be listening on TCP 5760) and has a
    // client: with none, the link reached another program on the Windows port.
    let own_port = if sys.is_some_and(systems::routed) {
        super::stage::router_up(s) && super::stage::router_has_client(s)
    } else {
        ardupilot
    };
    let app_router_link = v.sim
        && link.origin() == LinkOrigin::SimulatorRouter
        && own_port
        && foreign_vehicle(s).is_none();
    let simulator_source = sys.is_some_and(|x| x.source.simulated());
    let stack_owned_running = sys.and_then(systems::vehicle_profile).is_some_and(|id| {
        s.status.get(id).is_some_and(|st| st == "Running") && s.systems.owner(id) == Some(owner)
    });
    SimulationEvidence {
        app_router_link,
        simulator_source,
        stack_owned_running,
    }
}

/// Why the link may reach a vehicle this app did not start, or `None`: the router of a routed
/// system saw a second UDP sender (another flight stack, or a forwarder for a real flight
/// controller), or heartbeats from more than one autopilot system id arrived on the link.
pub(crate) fn foreign_vehicle(s: &State) -> Option<String> {
    let routed = s
        .vehicle
        .owner
        .as_deref()
        .and_then(|id| s.systems.get(id))
        .is_some_and(systems::routed);
    let foreign = s.agent.router.as_ref().map_or(0, |r| r.foreign);
    if routed && foreign > 0 {
        return Some(format!(
            "Another program sends MAVLink to UDP 14550 inside WSL ({foreign} datagrams ignored), so the link could reach a vehicle Drone Deck did not start. Stop the other flight stack or forwarder, then restart the workspace."
        ));
    }
    let mut ids: Vec<u8> = s
        .vehicle
        .snapshot
        .iter()
        .flat_map(|snap| &snap.systems)
        .filter(|x| is_vehicle_heartbeat(x.autopilot, x.vehicle_type))
        .map(|x| x.system_id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    (ids.len() > 1).then(|| {
        let list: Vec<String> = ids.iter().map(u8::to_string).collect();
        format!(
            "Autopilots with system ids {} share this link, so commands could reach a vehicle Drone Deck did not start. Stop the other vehicle, then restart the workspace.",
            list.join(", ")
        )
    })
}

/// `OwnedSimulation` only when every piece of evidence holds; otherwise `ReadOnly`.
pub(crate) fn authority(s: &State) -> Authority {
    Authority::from_evidence(evidence(s))
}

/// Why flight commands and the console are refused right now, in user words; `None` when
/// they are allowed.
pub(crate) fn gate_reason(s: &State) -> Option<String> {
    if authority(s).may_fly() {
        return None;
    }
    let v = &s.vehicle;
    if v.link.is_none() && !(s.headless && v.preview.is_some()) {
        return Some("No live link. Start the workspace of a simulated system first.".into());
    }
    if v.link
        .as_ref()
        .is_some_and(|l| l.origin() == LinkOrigin::Physical)
    {
        return Some("Physical vehicle links are read-only in Drone Deck.".into());
    }
    let e = evidence(s);
    let owner = v.owner.as_deref().and_then(|id| s.systems.get(id));
    Some(if !e.simulator_source {
        "Commands are only sent to a simulated vehicle.".into()
    } else if let Some(why) = foreign_vehicle(s) {
        why
    } else if let (false, Some(router)) = (e.app_router_link, &v.router.error) {
        router.clone()
    } else if !e.app_router_link
        && v.sim
        && owner.is_some_and(systems::routed)
        && super::stage::router_up(s)
        && !super::stage::router_has_client(s)
    {
        "The MAVLink router in WSL reports no client, so the link on TCP 5760 reached another program. Stop whatever listens on Windows TCP 5760, then start the session again.".into()
    } else if !e.app_router_link {
        "The link is not the app's own simulator link. Start the workspace from Drone Deck.".into()
    } else {
        let stack = owner
            .and_then(systems::vehicle_profile)
            .and_then(systems::component)
            .map_or("the simulator", |c| c.name);
        format!("{stack} is not running under this system. Start its workspace first.")
    })
}

/// Why simulator `id` must not start for system `for_system` now: another simulator runs,
/// the same one runs for another system, or something on Windows already listens on the
/// simulator port (`stage::sim_port_taken`). `None` when it may start.
pub(crate) fn simulator_conflict(s: &State, id: &str, for_system: &str) -> Option<String> {
    if !SIMULATORS.contains(&id) {
        return None;
    }
    let name = |sim: &str| systems::component(sim).map_or(sim.to_string(), |c| c.name.into());
    let owner = |sim: &str| {
        s.systems
            .owner(sim)
            .and_then(|o| s.systems.get(o))
            .map_or_else(|| "another system".to_string(), |x| x.label().to_string())
    };
    for other in SIMULATORS {
        if !systems::running(s, other) {
            continue;
        }
        if other != id {
            return Some(format!(
                "{} is running for {}. Stop it before starting {}: simulators share UDP 14550 and TCP 5760.",
                name(other),
                owner(other),
                name(id)
            ));
        }
        if s.systems.owner(other).is_some_and(|o| o != for_system) {
            return Some(format!(
                "{} already runs for {}. Stop that session first.",
                name(other),
                owner(other)
            ));
        }
    }
    // A simulator already running (ArduPilot serves the port itself) or the app's own router
    // holds the port legitimately; otherwise it must be free before a simulator starts.
    if systems::running(s, id) || super::stage::router_up(s) {
        return None;
    }
    super::stage::sim_port_taken(s)
}

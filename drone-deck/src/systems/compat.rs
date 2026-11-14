//! Which consumers a vehicle can feed. The editor hides the others and saving turns them off,
//! so a toggle left over from another vehicle never shapes components or lists.
use super::model::System;

/// Consumers that only some vehicles can feed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Consumer {
    /// The native live view of the vehicle link: any live vehicle, not a recording.
    LiveView,
    /// The ROS 2 bridge: a simulator whose adapter has one.
    RosAgent,
    /// MAVLink over UDP for user scripts: any live vehicle, not a recording.
    Scripts,
}

/// Whether a vehicle of `VEHICLES` index `vehicle`, simulated with a ROS bridge when
/// `ros_bridge`, can feed consumer `c`.
pub(crate) fn consumer_ok(vehicle: usize, ros_bridge: bool, c: Consumer) -> bool {
    match c {
        Consumer::LiveView | Consumer::Scripts => vehicle != 2,
        Consumer::RosAgent => vehicle == 0 && ros_bridge,
    }
}

/// Turns off the consumers `sys`'s vehicle cannot feed.
pub(crate) fn prune(sys: &mut System) {
    let vehicle = sys.vehicle.index();
    let ros_bridge = sys.adapter().is_some_and(|a| a.ros.is_some());
    let ok = |c| consumer_ok(vehicle, ros_bridge, c);
    let c = &mut sys.consumers;
    c.live_view &= ok(Consumer::LiveView);
    c.ros.agent &= ok(Consumer::RosAgent);
    if !ok(Consumer::Scripts) {
        c.scripts_udp = None;
    }
}

/// Whether Drone Deck itself may send flight commands and console lines to `sys`'s vehicle:
/// today only a simulator it starts and owns. This is the one place a later, deliberate
/// opt-in for real drones would change. The gate's evidence (`vehicle::gate`) and the link's
/// own refusal of physical links (`mavlink::Service::check_flight`) still apply underneath.
pub(crate) fn may_command(sys: &System) -> bool {
    sys.simulated()
}

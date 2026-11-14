//! The supported (autopilot, world) pairs.
use super::{Adapter, AutopilotKind, Link, Param, Readiness, Ros, Scope, Stage};

fn same(value: &str) -> String {
    value.to_string()
}

/// `sim_vehicle.py -v` names for the ArduPilot vehicles a system can name.
pub(crate) fn ardupilot_vehicle(vehicle: &str) -> String {
    match vehicle.trim().to_ascii_lowercase().as_str() {
        "copter" | "arducopter" => "ArduCopter".into(),
        "plane" | "arduplane" => "ArduPlane".into(),
        "rover" => "Rover".into(),
        "sub" | "ardusub" => "ArduSub".into(),
        "heli" | "helicopter" => "Helicopter".into(),
        _ => vehicle.trim().into(),
    }
}

const PX4_ROS: Ros = Ros {
    agent: "dds",
    topic_prefix: "/fmu/",
    extra: &[],
};

pub(crate) static ALL: [Adapter; 3] = [
    Adapter {
        id: "px4-sih",
        name: "PX4 SIH",
        autopilot: AutopilotKind::Px4,
        world: "internal",
        stages: &[Stage {
            profile: "sih",
            readiness: Readiness::Px4Status {
                simulator: "sihsim",
            },
        }],
        link: Link::Router,
        ros: Some(PX4_ROS),
        params: &[],
        recipes: &["px4"],
        shell: true,
    },
    Adapter {
        id: "px4-gazebo",
        name: "PX4 + Gazebo",
        autopilot: AutopilotKind::Px4,
        world: "gazebo",
        stages: &[Stage {
            profile: "gazebo",
            readiness: Readiness::ProcessRunning,
        }],
        link: Link::Router,
        ros: Some(Ros {
            extra: &["rosgz"],
            ..PX4_ROS
        }),
        params: &[
            Param {
                key: "world",
                label: "Gazebo world",
                scope: Scope::World,
                default: "default",
                env: "DRONE_DECK_GZ_WORLD",
                map: same,
            },
            Param {
                key: "model",
                label: "Vehicle model",
                scope: Scope::World,
                default: "x500_mono_cam",
                env: "DRONE_DECK_GZ_MODEL",
                map: same,
            },
        ],
        recipes: &["px4", "gazebo"],
        shell: true,
    },
    Adapter {
        id: "ardupilot-sitl",
        name: "ArduPilot SITL",
        autopilot: AutopilotKind::ArduPilot,
        world: "internal",
        stages: &[Stage {
            profile: "ardupilot",
            readiness: Readiness::ProcessRunning,
        }],
        link: Link::Direct,
        // ArduPilot's DDS client publishes under /ap/. The workspace launch does not start
        // the agent for it (its DDS port differs from PX4's), so only the chain shows it.
        ros: Some(Ros {
            agent: "dds",
            topic_prefix: "/ap/",
            extra: &[],
        }),
        params: &[Param {
            key: "vehicle",
            label: "ArduPilot vehicle",
            scope: Scope::Autopilot,
            default: "copter",
            env: "DRONE_DECK_AP_VEHICLE",
            map: ardupilot_vehicle,
        }],
        recipes: &["ardupilot"],
        shell: false,
    },
];

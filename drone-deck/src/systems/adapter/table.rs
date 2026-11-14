//! The supported (autopilot, world) pairs.
use super::{
    Adapter, AutopilotKind, Condition, HomeEnv, Link, Param, Readiness, Ros, Scope, Stage,
};

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

const ARDUPILOT_VEHICLE: Param = Param {
    key: "vehicle",
    label: "ArduPilot vehicle",
    scope: Scope::Autopilot,
    default: "copter",
    env: "DRONE_DECK_AP_VEHICLE",
    map: ardupilot_vehicle,
};

/// PX4 SITL takes its home position from these (SIH, Gazebo and jMAVSim).
const PX4_HOME: HomeEnv = HomeEnv {
    lat: "PX4_HOME_LAT",
    lon: "PX4_HOME_LON",
    alt: "PX4_HOME_ALT",
};

const PX4_ROS: Ros = Ros {
    agent: "dds",
    topic_prefix: "/fmu/",
    commands: &["/fmu/in/"],
    extra: &[],
};

pub(crate) static ALL: [Adapter; 8] = [
    Adapter {
        id: "px4-sih",
        name: "PX4 SIH",
        autopilot: AutopilotKind::Px4,
        world: "internal",
        vehicle: "sih",
        stages: &[Stage {
            profile: "sih",
            readiness: Readiness::Px4Status {
                simulator: Some("sihsim"),
            },
        }],
        link: Link::Router,
        ros: Some(PX4_ROS),
        params: &[],
        recipes: &["px4"],
        shell: true,
        home: Some(PX4_HOME),
        conditions: &[],
    },
    Adapter {
        id: "px4-gazebo",
        name: "PX4 + Gazebo",
        autopilot: AutopilotKind::Px4,
        world: "gazebo",
        vehicle: "gazebo",
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
        home: Some(PX4_HOME),
        conditions: &[Condition {
            key: "wind",
            desc: "Flies in the windy world that ships with PX4's Gazebo models.",
            set: &[("world", "windy")],
        }],
    },
    Adapter {
        id: "ardupilot-sitl",
        name: "ArduPilot SITL",
        autopilot: AutopilotKind::ArduPilot,
        world: "internal",
        vehicle: "ardupilot",
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
            commands: &["/ap/cmd_", "/ap/joy"],
            extra: &[],
        }),
        params: &[ARDUPILOT_VEHICLE],
        recipes: &["ardupilot"],
        shell: false,
        home: None,
        conditions: &[],
    },
    Adapter {
        id: "px4-jmavsim",
        name: "PX4 + jMAVSim",
        autopilot: AutopilotKind::Px4,
        world: "jmavsim",
        vehicle: "jmavsim",
        // `make px4_sitl jmavsim` starts PX4 and jMAVSim together.
        stages: &[Stage {
            profile: "jmavsim",
            readiness: Readiness::Px4Status { simulator: None },
        }],
        link: Link::Router,
        ros: Some(PX4_ROS),
        params: &[],
        recipes: &["px4", "jmavsim"],
        shell: true,
        home: Some(PX4_HOME),
        conditions: &[],
    },
    Adapter {
        id: "px4-xplane",
        name: "PX4 + X-Plane",
        autopilot: AutopilotKind::Px4,
        world: "xplane",
        vehicle: "px4-none",
        // PX4 waits in WSL for a simulator; X-Plane's px4xplane plugin connects to it from
        // Windows. Written from the plugin's documentation and not run here.
        stages: &[
            Stage {
                profile: "px4-none",
                readiness: Readiness::ProcessRunning,
            },
            Stage {
                profile: "xplane",
                readiness: Readiness::ProcessRunning,
            },
        ],
        link: Link::Router,
        ros: Some(PX4_ROS),
        params: &[],
        recipes: &["px4", "xplane"],
        shell: true,
        home: None,
        conditions: &[],
    },
    Adapter {
        id: "px4-flightgear",
        name: "PX4 + FlightGear",
        autopilot: AutopilotKind::Px4,
        world: "flightgear",
        vehicle: "flightgear",
        // `make px4_sitl flightgear_rascal` starts PX4, its FlightGear bridge and FlightGear.
        stages: &[Stage {
            profile: "flightgear",
            readiness: Readiness::Px4Status { simulator: None },
        }],
        link: Link::Router,
        ros: Some(PX4_ROS),
        params: &[],
        recipes: &["px4", "flightgear"],
        shell: true,
        home: None,
        conditions: &[],
    },
    Adapter {
        id: "ardupilot-webots",
        name: "ArduPilot + Webots",
        autopilot: AutopilotKind::ArduPilot,
        world: "webots",
        vehicle: "ardupilot-webots",
        // SITL with the webots-python model waits for the world you open in Webots.
        stages: &[
            Stage {
                profile: "ardupilot-webots",
                readiness: Readiness::ProcessRunning,
            },
            Stage {
                profile: "webots",
                readiness: Readiness::ProcessRunning,
            },
        ],
        link: Link::Direct,
        ros: None,
        params: &[ARDUPILOT_VEHICLE],
        recipes: &["ardupilot", "webots"],
        shell: false,
        home: None,
        conditions: &[],
    },
    Adapter {
        id: "ardupilot-realflight",
        name: "ArduPilot + RealFlight",
        autopilot: AutopilotKind::ArduPilot,
        world: "realflight",
        vehicle: "ardupilot-realflight",
        // RealFlight serves FlightAxis; SITL connects to it, so RealFlight starts first.
        stages: &[
            Stage {
                profile: "realflight",
                readiness: Readiness::ProcessRunning,
            },
            Stage {
                profile: "ardupilot-realflight",
                readiness: Readiness::ProcessRunning,
            },
        ],
        link: Link::Direct,
        ros: None,
        params: &[
            ARDUPILOT_VEHICLE,
            Param {
                key: "host",
                label: "RealFlight computer (IP address)",
                scope: Scope::World,
                default: "127.0.0.1",
                env: "DRONE_DECK_FLIGHTAXIS_HOST",
                map: same,
            },
        ],
        recipes: &["ardupilot", "realflight"],
        shell: false,
        home: None,
        conditions: &[],
    },
];

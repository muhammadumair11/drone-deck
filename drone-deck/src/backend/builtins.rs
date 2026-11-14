//! The built-in launch profiles, earlier built-in commands that migrations recognise, and the
//! environment a launch passes to a profile.
use super::{Config, Profile};
use std::collections::HashMap;

const PX4_DIR: &str = "cd \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\"";
/// The PX4 + Gazebo command; world and model come from the system (`launch_env`).
pub(super) const GAZEBO_ARGS: &str = "cd \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\" && test -z \"$(pgrep -x px4)\" && PX4_GZ_WORLD=\"${DRONE_DECK_GZ_WORLD:-default}\" make px4_sitl \"gz_${DRONE_DECK_GZ_MODEL:-x500_mono_cam}\"";
/// ArduPilot SITL serves MAVLink on TCP 5760 itself (`--no-mavproxy`).
pub(super) const ARDUPILOT_ARGS: &str = "cd ~/ardupilot && Tools/autotest/sim_vehicle.py -v \"${DRONE_DECK_AP_VEHICLE:-ArduCopter}\" --no-mavproxy";
/// Camera image and camera_info of PX4's `x500_mono_cam` (sensor `camera` on `camera_link`,
/// instance `_0`) into ROS as `/camera/image_raw` and `/camera/camera_info`.
const ROSGZ_ARGS: &str = "source /opt/ros/jazzy/setup.bash && G=\"/world/${DRONE_DECK_GZ_WORLD:-default}/model/${DRONE_DECK_GZ_MODEL:-x500_mono_cam}_0/link/camera_link/sensor/camera\" && exec ros2 run ros_gz_bridge parameter_bridge \"$G/image@sensor_msgs/msg/Image[gz.msgs.Image\" \"$G/camera_info@sensor_msgs/msg/CameraInfo[gz.msgs.CameraInfo\" --ros-args -r \"$G/image:=/camera/image_raw\" -r \"$G/camera_info:=/camera/camera_info\"";
const FOXGLOVE_ARGS: &str = "source /opt/ros/jazzy/setup.bash && exec ros2 launch foxglove_bridge foxglove_bridge_launch.xml port:=8765";
/// Built-in profiles of earlier versions that the native vehicle link replaced. Stored
/// profiles with these ids are dropped when `profiles.json` is upgraded to schema 2.
/// `jmavsim` came back in schema 4 as a PX4 launch profile through the router.
pub(crate) const RETIRED: [&str; 4] = ["relay", "jmavsim", "telemetry", "hardware"];
/// Built-in profiles schema 4 adds when a file lacks them.
pub(crate) const ADDED_IN_4: [&str; 8] = [
    "jmavsim",
    "px4-none",
    "xplane",
    "flightgear",
    "ardupilot-webots",
    "webots",
    "realflight",
    "ardupilot-realflight",
];
/// ArduPilot SITL with the Webots Python model: it waits for the world opened in Webots.
const ARDUPILOT_WEBOTS_ARGS: &str = "cd ~/ardupilot && Tools/autotest/sim_vehicle.py -v \"${DRONE_DECK_AP_VEHICLE:-ArduCopter}\" --model webots-python --no-mavproxy";
/// ArduPilot SITL driving RealFlight over FlightAxis on the system's RealFlight computer.
const ARDUPILOT_REALFLIGHT_ARGS: &str = "cd ~/ardupilot && Tools/autotest/sim_vehicle.py -v \"${DRONE_DECK_AP_VEHICLE:-ArduCopter}\" -f \"flightaxis:${DRONE_DECK_FLIGHTAXIS_HOST:-127.0.0.1}\" --no-mavproxy";
/// Earlier built-in commands that schema 2 replaces when they were never edited.
/// The interpreter the built-in MAVSDK telemetry profile named before adoption.
pub(super) const OLD_MAVSDK_PYTHON: &str =
    "\"$HOME/.local/share/drone-deck/tools/mavsdk/bin/python\"";
pub(super) const OLD_GAZEBO: &str = "cd \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\" && test -z \"$(pgrep -x px4)\" && make px4_sitl gz_x500";
pub(super) const OLD_ARDUPILOT: &str =
    "cd ~/ardupilot && Tools/autotest/sim_vehicle.py -v ArduCopter --no-mavproxy";

pub fn defaults() -> Config {
    let mut profiles = Vec::new();
    let mut add = |id: &str,
                   name: &str,
                   cat: &str,
                   desc: &str,
                   program: &str,
                   args: Vec<String>,
                   probe: &str,
                   docs: &str| {
        profiles.push(Profile {
            px4: match id {
                "sih" => Some(crate::px4::Mode::Sih),
                "gazebo" => Some(crate::px4::Mode::Gazebo),
                "jmavsim" => Some(crate::px4::Mode::JMavSim),
                "px4-none" => Some(crate::px4::Mode::External),
                "flightgear" => Some(crate::px4::Mode::FlightGear),
                _ => None,
            },
            id: id.into(),
            name: name.into(),
            category: cat.into(),
            description: desc.into(),
            program: program.into(),
            args,
            cwd: String::new(),
            probe: probe.into(),
            docs: docs.into(),
            download_url: String::new(),
            // Flight stacks share ports: the worker refuses a second one as well as the UI.
            // Later stages of one simulator (X-Plane beside its PX4) are not flight stacks.
            exclusive: crate::systems::adapter::is_vehicle_profile(id),
        })
    };
    add(
        "sih",
        "PX4 / SIH",
        "Simulation",
        "Lightweight internal flight physics · Quad X",
        "wsl",
        vec![format!(
            "{PX4_DIR} && test -z \"$(pgrep -x px4)\" && make px4_sitl_sih sihsim_quadx"
        )],
        "test -x \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot/build/px4_sitl_sih/bin/px4\"",
        "https://docs.px4.io/main/en/sim_sih/",
    );
    add(
        "gazebo",
        "Gazebo Harmonic",
        "Simulation",
        "PX4 with the system's Gazebo world and model",
        "wsl",
        vec![GAZEBO_ARGS.into()],
        "command -v gz && test -d \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\"",
        "https://docs.px4.io/main/en/sim_gazebo_gz/",
    );
    add(
        "qgc",
        "QGroundControl",
        "Flight operations",
        "Vehicle setup, missions and flight control",
        r"C:\Program Files\QGroundControl\bin\QGroundControl.exe",
        vec![],
        r"C:\Program Files\QGroundControl\bin\QGroundControl.exe",
        "https://docs.qgroundcontrol.com/master/en/qgc-user-guide/getting_started/download_and_install.html",
    );
    add(
        "ros2",
        "ROS 2 Jazzy",
        "Development",
        "ROS graph discovery · configure your node command",
        "wsl",
        vec!["source /opt/ros/jazzy/setup.bash && ros2 topic list".into()],
        "test -f /opt/ros/jazzy/setup.bash",
        "https://docs.ros.org/en/jazzy/Installation/Ubuntu-Install-Debs.html",
    );
    add(
        "dds",
        "Micro XRCE-DDS Agent",
        "Infrastructure",
        "PX4 ↔ ROS 2 transport · UDP 8888",
        "wsl",
        vec!["MicroXRCEAgent udp4 -p 8888".into()],
        "command -v MicroXRCEAgent",
        "https://docs.px4.io/main/en/middleware/uxrce_dds",
    );
    add(
        "rosgz",
        "ROS–Gazebo camera bridge",
        "Infrastructure",
        "x500_mono_cam image + camera_info → /camera/image_raw, /camera/camera_info",
        "wsl",
        vec![ROSGZ_ARGS.into()],
        "test -f /opt/ros/jazzy/share/ros_gz_bridge/package.xml",
        "https://docs.px4.io/main/en/sim_gazebo_gz/#ros-2-and-gazebo",
    );
    add(
        "foxglove",
        "Foxglove bridge",
        "Development",
        "Live ROS topics for Foxglove · WebSocket port 8765",
        "wsl",
        vec![FOXGLOVE_ARGS.into()],
        "test -f /opt/ros/jazzy/share/foxglove_bridge/package.xml",
        "https://docs.foxglove.dev/docs/connecting-to-data/ros-foxglove-bridge",
    );
    add(
        "jmavsim",
        "jMAVSim",
        "Simulation",
        "PX4 with the lightweight jMAVSim 3D view",
        "wsl",
        vec![format!(
            "{PX4_DIR} && test -z \"$(pgrep -x px4)\" && make px4_sitl jmavsim"
        )],
        "test -f \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot/Tools/simulation/jmavsim/jMAVSim/out/production/jmavsim_run.jar\"",
        "https://docs.px4.io/v1.17/en/sim_jmavsim/",
    );
    add(
        "px4-none",
        "PX4 for an external simulator",
        "Simulation",
        "PX4 waiting for a simulator such as X-Plane on TCP 4560",
        "wsl",
        vec![format!(
            "{PX4_DIR} && test -z \"$(pgrep -x px4)\" && make px4_sitl none_iris"
        )],
        "test -f \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot/Makefile\"",
        "https://docs.px4.io/v1.17/en/simulation/",
    );
    add(
        "xplane",
        "X-Plane 12",
        "Simulation",
        "X-Plane with the px4xplane plugin, installed by you",
        r"C:\X-Plane 12\X-Plane.exe",
        vec![],
        r"C:\X-Plane 12\X-Plane.exe",
        "https://github.com/alireza787b/px4xplane",
    );
    add(
        "flightgear",
        "FlightGear",
        "Simulation",
        "PX4 with FlightGear through PX4's FlightGear bridge",
        "wsl",
        vec![format!(
            "{PX4_DIR} && test -z \"$(pgrep -x px4)\" && make px4_sitl flightgear_rascal"
        )],
        "command -v fgfs && test -f \"$HOME/.local/share/drone-deck/tools/PX4-Autopilot/Makefile\"",
        "https://docs.px4.io/v1.17/en/sim_flightgear/",
    );
    add(
        "ardupilot-webots",
        "ArduPilot SITL for Webots",
        "Simulation",
        "ArduPilot SITL waiting for an ArduPilot world in Webots",
        "wsl",
        vec![ARDUPILOT_WEBOTS_ARGS.into()],
        "test -x ~/ardupilot/Tools/autotest/sim_vehicle.py",
        "https://ardupilot.org/dev/docs/sitl-with-webots-python.html",
    );
    add(
        "webots",
        "Webots",
        "Simulation",
        "Webots robot simulator, installed by you",
        r"C:\Program Files\Webots\msys64\mingw64\bin\webotsw.exe",
        vec![],
        r"C:\Program Files\Webots\msys64\mingw64\bin\webotsw.exe",
        "https://cyberbotics.com/",
    );
    add(
        "realflight",
        "RealFlight",
        "Simulation",
        "RealFlight with FlightAxis enabled, installed by you",
        r"C:\Program Files\RealFlight\RealFlight.exe",
        vec![],
        r"C:\Program Files\RealFlight\RealFlight.exe",
        "https://ardupilot.org/dev/docs/sitl-with-realflight.html",
    );
    add(
        "ardupilot-realflight",
        "ArduPilot SITL for RealFlight",
        "Simulation",
        "ArduPilot SITL driving RealFlight over FlightAxis",
        "wsl",
        vec![ARDUPILOT_REALFLIGHT_ARGS.into()],
        "test -x ~/ardupilot/Tools/autotest/sim_vehicle.py",
        "https://ardupilot.org/dev/docs/sitl-with-realflight.html",
    );
    add(
        "ardupilot",
        "ArduPilot SITL",
        "Simulation",
        "Alternative flight stack · serves MAVLink on TCP 5760",
        "wsl",
        vec![ARDUPILOT_ARGS.into()],
        "test -x ~/ardupilot/Tools/autotest/sim_vehicle.py",
        "https://ardupilot.org/dev/docs/sitl-simulator-software-in-the-loop.html",
    );
    Config {
        px4_installations: HashMap::new(),
        distro: "Ubuntu-24.04".into(),
        profiles,
    }
}
/// Environment for profile `id` started for `sys`: the world and vehicle details its simulator
/// adapter takes, and the ROS domain for ROS tools. Values reach the process through the
/// agent's environment, never through the command line.
pub(crate) fn launch_env(id: &str, sys: &crate::systems::System) -> Vec<(String, String)> {
    let mut env = crate::systems::adapter::launch_env(sys, id);
    if matches!(id, "dds" | "ros2" | "rosgz" | "foxglove" | "ros-workspace") {
        env.push(("ROS_DOMAIN_ID".into(), sys.consumers.ros.domain.to_string()));
    }
    env.extend(crate::resim::launch_env(id, sys));
    env
}

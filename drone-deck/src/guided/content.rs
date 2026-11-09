//! The words of guided setup for each system, from `Design/system-setup.html`.

pub(crate) struct Story {
    pub name: &'static str,
    pub desc: &'static str,
    pub tag: &'static str,
    pub title: &'static str,
    pub result: &'static str,
    /// Blueprint stations: name, role, explanation, where it runs.
    pub steps: [(&'static str, &'static str, &'static str, &'static str); 3],
    pub extra: &'static str,
    /// First-run tasks: title, what happens, what to observe.
    pub tasks: [(&'static str, &'static str, &'static str); 3],
    /// Preparation rows for parts that need no installation: name, role, note.
    pub builtin: &'static [(&'static str, &'static str, &'static str)],
}

pub(crate) const STORIES: [Story; 4] = [
    Story {
        name: "Watch a virtual drone",
        desc: "See an autopilot, a simulated vehicle and a ground station working together.",
        tag: "Start here / no hardware",
        title: "A flight lab on your laptop.",
        result: "A virtual drone you can observe, with flight controls in a separate ground-station window.",
        steps: [
            (
                "PX4 / SIH",
                "Simulate the vehicle",
                "The autopilot and internal physics run together in Ubuntu.",
                "Ubuntu / WSL",
            ),
            (
                "Display relay",
                "Move state to Windows",
                "Forwards vehicle state across the Linux-to-Windows boundary.",
                "WSL / UDP 19410",
            ),
            (
                "jMAVSim",
                "See the result",
                "A separate Windows window displays the simulated movement.",
                "Windows / Java",
            ),
        ],
        extra: "QGroundControl opens separately for vehicle setup and flight controls. MAVSDK is optional for read-only telemetry on UDP 14540.",
        tasks: [
            (
                "Start the virtual vehicle",
                "Drone Deck starts PX4, waits for SIH readiness, starts the relay, then opens jMAVSim.",
                "Confirm that live state reaches the visualizer.",
            ),
            (
                "Open the ground station",
                "QGroundControl opens as a separate window. Drone Deck does not arm or fly the vehicle.",
                "Confirm a vehicle appears in QGroundControl.",
            ),
            (
                "Read the result",
                "Start read-only MAVSDK telemetry and compare it with the vehicle shown in the visualizer and the ground station.",
                "Process running and vehicle readiness remain separate checks.",
            ),
        ],
        builtin: &[],
    },
    Story {
        name: "Capture sensor data",
        desc: "Connect ROS streams, record them, and keep a dataset you can return to.",
        tag: "ROS / recordings",
        title: "From live messages to a reusable recording.",
        result: "A finalized MCAP recording, ready for isolated replay and a versioned dataset.",
        steps: [
            (
                "PX4 + matching messages",
                "Provide the source",
                "A compatible DDS client and firmware-matched px4_msgs are prerequisites.",
                "Ubuntu / WSL",
            ),
            (
                "DDS Agent + ROS 2",
                "Connect the streams",
                "The Agent bridges messages. Live publishers must actually be present.",
                "UDP 8888 / ROS Jazzy",
            ),
            (
                "rosbag2 / MCAP",
                "Keep the evidence",
                "Capture selected topics, then stop gracefully to finalize metadata.",
                "Local Linux storage",
            ),
        ],
        extra: "Replay uses a separate ROS domain, normally 77. Camera and LiDAR streams need their own publishers; SIH does not automatically create them.",
        tasks: [
            (
                "Check publishers",
                "Drone Deck starts PX4 and the DDS Agent, checks the recording tools, then discovers live ROS topics.",
                "Verify /fmu/out or your sensor topics are listed.",
            ),
            (
                "Record selected topics",
                "Records the topics selected in Data & models, or every discovered topic when none are selected.",
                "A running recorder may still contain zero messages.",
            ),
            (
                "Finalize and keep the recording",
                "Stop gracefully, then inspect the message count of the finalized recording.",
                "Keep originals; dataset versions do not copy MCAP files.",
            ),
        ],
        builtin: &[],
    },
    Story {
        name: "Test a model offline",
        desc: "Use prepared inputs to inspect a local model and retain its outputs.",
        tag: "AI / no vehicle needed",
        title: "Test the model before connecting it to flight.",
        result: "Local output tensors and a reproducible run record; evaluation requires supplied labels.",
        steps: [
            (
                "Model + prepared input",
                "Choose the material",
                "Select a compatible ONNX file and correctly preprocessed NPZ tensors.",
                "Local assets",
            ),
            (
                "ONNX CPU runtime",
                "Run the system",
                "The editable starter executes inference inside the selected WSL environment.",
                "Ubuntu / WSL",
            ),
            (
                "Artifacts + evaluation",
                "Inspect the evidence",
                "Retain hashes and outputs. Use a predictions CSV for labeled evaluation.",
                "Local results",
            ),
        ],
        extra: "Hugging Face downloads are optional. Downloaded weights do not supply a sensor adapter, calibration, preprocessing or flight capability.",
        tasks: [
            (
                "Select compatible inputs",
                "In Data & models → Projects, provide the ONNX model and prepared NPZ tensors named for the model inputs.",
                "Input preparation is task-specific; it is not automatic.",
            ),
            (
                "Run the local project",
                "Create or choose the editable ONNX starter and run it in its saved WSL environment.",
                "Model and input hashes and runtime versions are recorded.",
            ),
            (
                "Inspect artifacts",
                "Read output tensors; evaluate a predictions CSV only when labels are available.",
                "No accuracy score is invented when targets are absent.",
            ),
        ],
        builtin: &[(
            "Model + input files",
            "Provided by you / local",
            "Verify input names and preprocessing",
        )],
    },
    Story {
        name: "Observe my physical drone",
        desc: "Connect a flight controller for read-only monitoring and local logs.",
        tag: "Hardware / Windows native",
        title: "Know what is connected before doing anything.",
        result: "An identified vehicle with fresh telemetry and a local tlog. Flight controls stay in the ground station.",
        steps: [
            (
                "Serial / UDP / TCP",
                "Choose the connection",
                "Use a dedicated endpoint so tools do not compete for the same port.",
                "Windows native",
            ),
            (
                "MAVLink monitor",
                "Identify the vehicle",
                "Look for a fresh autopilot heartbeat and select its identity.",
                "Read-only monitoring",
            ),
            (
                "Telemetry + tlog",
                "Keep the record",
                "Inspect freshness and retain the local telemetry log.",
                "Windows local storage",
            ),
        ],
        extra: "This route does not need PX4 simulation or WSL. Parameter requests are read-only; direct flight commands and parameter writes are not implemented.",
        tasks: [
            (
                "Choose an endpoint",
                "Enter serial:COM4:115200, a dedicated UDP endpoint, or a TCP address.",
                "Avoid an endpoint already owned by another ground station.",
            ),
            (
                "Identify the vehicle",
                "Start read-only monitoring and wait for a fresh autopilot heartbeat.",
                "Select the actual vehicle identity before requesting parameters.",
            ),
            (
                "Inspect and retain",
                "Review freshness and open the local tlog folder.",
                "Signatures are not authenticated; signed parameter links are refused.",
            ),
        ],
        builtin: &[
            (
                "Windows MAVLink monitor",
                "Built into Drone Deck / read-only",
                "No simulation install",
            ),
            (
                "Vehicle endpoint",
                "Serial or network / Windows",
                "Chosen in First run",
            ),
        ],
    },
];

/// Workflow preset for each system (`setup::workflow::PRESETS`).
pub(crate) const PRESET: [usize; 4] = [0, 1, 3, 4];

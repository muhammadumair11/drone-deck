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
        result: "A virtual drone you can observe and command from the System page (this simulation only), with missions and vehicle setup in QGroundControl.",
        steps: [
            (
                "PX4 / SIH",
                "Simulate the vehicle",
                "The autopilot and internal physics run together in Ubuntu.",
                "Ubuntu / WSL",
            ),
            (
                "MAVLink router",
                "Move state to Windows",
                "Bridges PX4's UDP 14550 to TCP 5760, which Windows reaches through WSL.",
                "WSL / TCP 5760",
            ),
            (
                "Vehicle link",
                "See the result",
                "Drone Deck shows the live vehicle and feeds QGroundControl on UDP 14550.",
                "Windows native",
            ),
        ],
        extra: "QGroundControl opens separately for vehicle setup and missions. Your own scripts on Windows (MAVSDK, pymavlink) can use UDP 14540 through the same link.",
        tasks: [
            (
                "Start the virtual vehicle",
                "Drone Deck starts PX4, waits for SIH readiness, starts the router, then opens the vehicle link.",
                "Confirm a fresh heartbeat reaches the live view.",
            ),
            (
                "Open the ground station",
                "QGroundControl opens as a separate window and connects through the vehicle link. Missions and vehicle setup happen there.",
                "Confirm a vehicle appears in QGroundControl.",
            ),
            (
                "Read the result",
                "Compare the live view on the System page with the vehicle shown in the ground station.",
                "Process running and vehicle readiness remain separate checks.",
            ),
        ],
        builtin: &[],
    },
    Story {
        name: "Capture sensor data",
        desc: "Connect ROS streams, record them, and replay or extract them later.",
        tag: "ROS / recordings",
        title: "From live messages to a reusable recording.",
        result: "A finalized MCAP recording, ready for isolated replay, frame extraction and inference on replay.",
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
                "Pipelines write extracted frames and inference runs next to the recording and never change it.",
            ),
        ],
        builtin: &[],
    },
    Story {
        name: "Test a model offline",
        desc: "Run a local model on prepared inputs, then on a replayed recording.",
        tag: "AI / no vehicle needed",
        title: "Test the model before connecting it to flight.",
        result: "Output tensors from prepared inputs, then detections recorded from a replayed recording, each with a run record. No accuracy is computed.",
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
                "Pipelines",
                "Infer on replay",
                "Replays a finalized recording on an isolated ROS domain and records the model's detections with model and source hashes.",
                "Ubuntu / WSL",
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
                "Infer on replay",
                "In Data & models → Pipelines, select a finalized recording with an image topic, enter the ONNX model and run Infer on replay.",
                "Detections are model output, not a measured accuracy.",
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
        result: "An identified vehicle with fresh telemetry and a local tlog. Drone Deck sends it no flight commands; flight control stays in the ground station.",
        steps: [
            (
                "Serial / UDP / TCP",
                "Choose the connection",
                "Use a dedicated endpoint so tools do not compete for the same port.",
                "Windows native",
            ),
            (
                "Vehicle link",
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
        extra: "This route does not need PX4 simulation or WSL. Drone Deck's link is read-only: it sends no flight commands, parameter writes or ground-station heartbeat. If you enable QGroundControl or scripts for this system, the link forwards their messages, so those programs can command the vehicle.",
        tasks: [
            (
                "Choose an endpoint",
                "Enter serial:COM4:115200, a dedicated UDP endpoint, or a TCP address.",
                "Avoid an endpoint already owned by another ground station.",
            ),
            (
                "Identify the vehicle",
                "Connect and wait for a fresh autopilot heartbeat.",
                "Check the vehicle identity before reading parameters.",
            ),
            (
                "Inspect and retain",
                "Review freshness and open the local tlog folder.",
                "Signatures are not authenticated by the read-only link.",
            ),
        ],
        builtin: &[
            (
                "Native vehicle link",
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

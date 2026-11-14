//! First-run task identities are shared by UI, actions, polling and evidence.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Task {
    Start,
    GroundStation,
    LiveView,
    Ros,
    Record,
    Finalize,
    Recording,
    Playback,
    Inputs,
    Project,
    Infer,
    Endpoint,
    Connect,
    Logs,
}

pub(super) fn tasks(s: &State) -> Vec<Task> {
    use Task::*;
    let Some(sys) = target(s) else {
        return vec![];
    };
    let c = &sys.consumers;
    let mut list = if sys.is_physical() {
        vec![Endpoint, Connect, Logs]
    } else if sys.is_replay() {
        vec![Recording, Playback]
    } else {
        vec![Start]
    };
    if !sys.is_replay() && c.qgc {
        list.push(GroundStation);
    }
    if sys.simulated() && c.live_view {
        list.push(LiveView);
    }
    if !sys.is_replay() && (c.ros.agent || c.ros.recorder) {
        list.push(Ros);
    }
    if !sys.is_replay() && c.ros.recorder {
        list.extend([Record, Finalize]);
    }
    if c.inference.is_some() {
        list.extend([Inputs, Project]);
        if sys.is_replay() || c.ros.recorder {
            list.push(Infer);
        }
    }
    list
}

impl Task {
    pub fn words(self) -> (&'static str, &'static str, &'static str) {
        use Task::*;
        match self {
            Start => (
                "Start the simulated vehicle",
                "Launch this system through its selected simulator adapter and connection stages.",
                "Confirm a fresh vehicle heartbeat, or adapter readiness when no vehicle link is used.",
            ),
            GroundStation => (
                "Open QGroundControl",
                "Open the selected ground station through the existing vehicle connection.",
                "Confirm the vehicle appears in its window.",
            ),
            LiveView => (
                "Inspect live telemetry",
                "Inspect this system's live vehicle view.",
                "A running process alone is not evidence of a fresh vehicle connection.",
            ),
            Ros => (
                "Check ROS 2 publishers",
                "Discover topics on this system's ROS domain, using its selected adapter and bridge.",
                "External sensor publishers must already be running; infrastructure topics do not count.",
            ),
            Record => (
                "Record selected topics",
                "Record the selected ROS 2 topics. When the selection is empty, use discovered application topics.",
                "Watch for recording activity before continuing.",
            ),
            Finalize => (
                "Finalize the recording",
                "Stop the recorder gracefully and inspect the saved recording.",
                "A finalized recording must contain messages.",
            ),
            Recording => (
                "Inspect the recording",
                "List recordings and verify the recording selected for this system.",
                "Choose a finalized recording containing messages.",
            ),
            Playback => (
                "Play recorded data",
                "Replay the selected recording on the isolated replay domain.",
                "Playback publishes saved data; command re-simulation is available in its separate workspace.",
            ),
            Inputs => (
                "Select inference inputs",
                "Open Projects and choose the model and prepared input tensors.",
                "Input preparation depends on the model.",
            ),
            Project => (
                "Run the inference project",
                "Run the selected project in Projects, then return here.",
                "Confirm that this run completes successfully.",
            ),
            Infer => (
                "Inspect inference on replay",
                "Open Pipelines and run inference on a compatible recorded image topic.",
                "Model output is not a measured accuracy.",
            ),
            Endpoint => (
                "Check the connection endpoint",
                "Review the saved physical vehicle connection.",
                "Avoid an endpoint already owned by another ground station.",
            ),
            Connect => (
                "Identify the physical vehicle",
                "Connect using the existing monitoring interface.",
                "Confirm the identity from a fresh heartbeat.",
            ),
            Logs => (
                "Inspect the telemetry log",
                "Open the telemetry log captured by the vehicle connection.",
                "Drone Deck's own physical flight-command restrictions remain in effect.",
            ),
        }
    }
}

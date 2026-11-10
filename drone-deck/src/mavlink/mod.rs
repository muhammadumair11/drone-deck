//! Native MAVLink: one service per vehicle link (simulator router over TCP, UDP, or a serial
//! radio) that fans packets out to local UDP peers, records a `.tlog`, and exposes the
//! vehicle as a coalesced `VehicleSnapshot` plus discrete `Event`s.
//!
//! Flight commands and the console need `Authority::OwnedSimulation` and a service started
//! with `LinkOrigin::SimulatorRouter`; parameter and log downloads are read-only and work on
//! any link, as does the one HOME_POSITION request sent when a vehicle is first found (see
//! `home`). On any link, fan-out peers are bridged both ways: what QGroundControl or a script
//! sends reaches the vehicle unchanged, and on a physical link it is counted in
//! `LinkStatus::peer_to_vehicle_frames`.

mod command;
mod engine;
mod frame;
mod home;
mod link;
mod logs;
mod modes;
mod params;
mod service;
mod shell;
mod state;
mod tlog;

pub use command::{
    Authority, CommandKind, CommandResult, Refusal, SimulationEvidence, takeoff_needs_home,
};
pub use link::Endpoint;
pub use logs::{LogEntry, utc_stamp};
pub use modes::{Autopilot, available as available_modes, is_vehicle_heartbeat};
pub use params::{Param, ParamChange, ParamSet, ParamValue, diff as diff_params};
pub use service::{
    Config, Event, LinkOrigin, LinkState, LinkStatus, PeerStatus, Service, SystemSeen,
    VehicleSnapshot, Waker,
};
pub use state::{StatusText, VehicleState};
pub use tlog::{
    TlogSummary, default_path as default_tlog_path, summarize as summarize_tlog,
    summarize_bytes as summarize_tlog_bytes,
};

pub(crate) use frame::{Encoder, Msg, dialect};
#[cfg(test)]
#[path = "fake_vehicle_tests.rs"]
pub(crate) mod fake_vehicle;
#[cfg(test)]
#[path = "link_safety_tests.rs"]
mod link_safety_tests;
#[cfg(test)]
#[path = "service_tests.rs"]
mod service_tests;

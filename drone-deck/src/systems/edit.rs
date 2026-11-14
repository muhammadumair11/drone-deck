//! The Settings → Systems editor's working copy and its validation.
use super::adapter::{self, AutopilotKind, Scope};
use super::compat;
use super::model::{
    Autopilot, DEFAULT_DOMAIN, DEFAULT_SCRIPTS_UDP, Inference, System, Vehicle, World,
};
use rune::TextEdit;
use std::collections::BTreeMap;
use std::net::SocketAddr;

pub(crate) struct Draft {
    /// The system being edited.
    pub id: String,
    pub name: TextEdit,
    /// `Vehicle::index` of the chosen vehicle kind.
    pub vehicle: usize,
    /// The autopilot of a simulated vehicle.
    pub autopilot: AutopilotKind,
    /// The world id of a simulated vehicle.
    pub world: String,
    /// Every adapter detail by key (the Gazebo world and model, the ArduPilot vehicle), so a
    /// change of autopilot or world starts from sensible values.
    pub params: BTreeMap<&'static str, TextEdit>,
    pub endpoint: TextEdit,
    pub recording: TextEdit,
    pub qgc: bool,
    pub live_view: bool,
    pub ros_agent: bool,
    pub recorder: bool,
    pub domain: TextEdit,
    pub inference: bool,
    pub project: TextEdit,
    pub foxglove: bool,
    pub scripts: bool,
    pub scripts_udp: TextEdit,
    pub error: String,
}
impl Default for Draft {
    fn default() -> Self {
        Self::of(&super::model::template(0, "", ""))
    }
}

/// A name such as `default` or `x500_mono_cam`: starts with a letter or digit (so never `..`
/// or a dash that reads as an option), then letters, digits, `_`, `-` or `.`.
fn token_ok(text: &str) -> bool {
    text.len() <= 64
        && text
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// `host:port` with a port in 1..=65535 and a host of letters, digits, dots or dashes.
pub(crate) fn udp_address_ok(text: &str) -> bool {
    let Some((host, port)) = text.rsplit_once(':') else {
        return false;
    };
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && port.parse::<u16>().is_ok_and(|p| p > 0)
}

/// What is wrong with a scripts address, or `None` for an `ip:port` the vehicle link forwards
/// to. The link sends only to addresses that parse as an IP address and port: a host name
/// (`localhost` included) is never resolved, and the unspecified 0.0.0.0 reaches no other
/// computer.
pub(crate) fn scripts_address_error(text: &str) -> Option<String> {
    let addr = text.trim();
    match addr.parse::<SocketAddr>() {
        Ok(a) if a.port() == 0 => {
            Some("The scripts port is a number from 1 to 65535, as in 127.0.0.1:14540.".into())
        }
        Ok(a) if a.ip().is_unspecified() => Some(format!(
            "{addr} is a listen address, not a destination: nothing sent to it reaches another computer. Use 127.0.0.1:14540 for scripts on this computer, or the other computer's IP address."
        )),
        Ok(_) => None,
        Err(_) if udp_address_ok(addr) => Some(format!(
            "{addr} names a host. Host names are not resolved, so nothing would be forwarded to it. Use an IP address such as 127.0.0.1:14540 or 192.168.1.20:14540."
        )),
        Err(_) => Some("User scripts listen on an ip:port such as 127.0.0.1:14540.".into()),
    }
}

/// Whether a scripts address is a real IP address on another computer: an `ip:port` that is
/// neither loopback nor unspecified. A host name never counts: it is refused, not forwarded.
pub(crate) fn off_this_computer(addr: &str) -> bool {
    addr.trim()
        .parse::<SocketAddr>()
        .is_ok_and(|a| !a.ip().is_loopback() && !a.ip().is_unspecified())
}

/// `serial:COMn:baud`, `udp:host:port` or `tcp:host:port`, or empty (chosen later).
pub(crate) fn endpoint_ok(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    if text.len() > 160 || text.contains(['\n', '\r', ' ']) {
        return false;
    }
    match text.split_once(':') {
        Some(("serial", rest)) => rest.rsplit_once(':').is_some_and(|(port, baud)| {
            port.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '/')
                && port
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
                && baud.parse::<u32>().is_ok_and(|b| b > 0)
        }),
        Some(("udp" | "tcp", rest)) => udp_address_ok(rest),
        _ => false,
    }
}

impl Draft {
    /// A working copy of `sys`. Details of other autopilots and worlds hold their defaults so
    /// a change starts from sensible values.
    pub fn of(sys: &System) -> Self {
        let text = |v: &str| TextEdit::new(v);
        let mut params = BTreeMap::new();
        for p in adapter::ALL.iter().flat_map(|a| a.params) {
            params
                .entry(p.key)
                .or_insert_with(|| text(sys.param(p.key).unwrap_or(p.default)));
        }
        let world = match &sys.vehicle {
            Vehicle::Simulated { world, .. } => world.id.clone(),
            _ => "internal".into(),
        };
        let recording = match &sys.vehicle {
            Vehicle::Replay { recording } => recording.as_deref().unwrap_or(""),
            _ => "",
        };
        let c = &sys.consumers;
        Self {
            id: sys.id.clone(),
            name: text(&sys.name),
            vehicle: sys.vehicle.index(),
            autopilot: sys.autopilot().unwrap_or(AutopilotKind::Px4),
            world,
            params,
            endpoint: text(sys.endpoint().unwrap_or("")),
            recording: text(recording),
            qgc: c.qgc,
            live_view: c.live_view,
            ros_agent: c.ros.agent,
            recorder: c.ros.recorder,
            domain: text(&c.ros.domain.to_string()),
            inference: c.inference.is_some(),
            project: text(
                c.inference
                    .as_ref()
                    .and_then(|i| i.project.as_deref())
                    .unwrap_or(""),
            ),
            foxglove: c.foxglove,
            scripts: c.scripts_udp.is_some(),
            scripts_udp: text(c.scripts_udp.as_deref().unwrap_or(DEFAULT_SCRIPTS_UDP)),
            error: String::new(),
        }
    }

    /// The adapter the draft's autopilot and world choose, when it is simulated.
    pub fn adapter(&self) -> Option<&'static adapter::Adapter> {
        (self.vehicle == 0)
            .then(|| adapter::find(self.autopilot, &self.world))
            .flatten()
    }

    /// Whether the draft's vehicle can feed consumer `c` (the editor shows only those).
    pub fn feeds(&self, c: compat::Consumer) -> bool {
        let ros_bridge = self.adapter().is_some_and(|a| a.ros.is_some());
        compat::consumer_ok(self.vehicle, ros_bridge, c)
    }

    /// Why the scripts address can't be saved, shown under the field as it is typed.
    pub fn scripts_problem(&self) -> Option<String> {
        scripts_address_error(&self.scripts_udp.text)
    }

    /// A caution that does not block saving: a physical system whose scripts consumer sends to
    /// another computer. The bridge forwards both ways, so that computer gets the vehicle's
    /// telemetry and what it sends reaches the vehicle.
    pub fn caution(&self) -> Option<String> {
        let addr = self.scripts_udp.text.trim();
        (self.vehicle == 1 && self.scripts && off_this_computer(addr))
            .then(|| {
                format!(
                    "{addr} is not this computer. With scripts on, the computer at that address receives this vehicle's telemetry and can command the vehicle through the bridge. Use 127.0.0.1 unless that computer should control it."
                )
            })
    }

    /// The simulated vehicle the draft names, or what to fix first. A world this version has
    /// no adapter for is kept unchanged when the draft still names it.
    fn simulated(&self, base: &System) -> Result<Vehicle, String> {
        let a = match adapter::world_for(self.autopilot, &self.world) {
            Ok(a) => a,
            Err(why) => {
                return match &base.vehicle {
                    Vehicle::Simulated { autopilot, world }
                        if autopilot.kind() == self.autopilot && world.id == self.world =>
                    {
                        Ok(base.vehicle.clone())
                    }
                    _ => Err(why),
                };
            }
        };
        let value = |key: &str| self.params.get(key).map_or("", |t| t.text.trim());
        if let Some(p) = a.params.iter().find(|p| !token_ok(value(p.key))) {
            return Err(format!(
                "{} is a name such as {} (letters, digits, _ - .).",
                p.label, p.default
            ));
        }
        let autopilot = match self.autopilot {
            AutopilotKind::Px4 => Autopilot::Px4,
            AutopilotKind::ArduPilot => Autopilot::ArduPilot {
                vehicle: value("vehicle").into(),
            },
        };
        let params: Vec<(&str, &str)> = a
            .params
            .iter()
            .filter(|p| p.scope == Scope::World)
            .map(|p| (p.key, value(p.key)))
            .collect();
        Ok(Vehicle::Simulated {
            autopilot,
            world: World::new(a.world, &params),
        })
    }

    /// `base` with this draft's name, vehicle and consumers, or what to fix first.
    pub fn apply(&self, base: &System) -> Result<System, String> {
        let name = super::clean_name(&self.name.text)?;
        let vehicle = match self.vehicle {
            1 => {
                let endpoint = self.endpoint.text.trim();
                if !endpoint_ok(endpoint) {
                    return Err("Use serial:COM4:115200, udp:0.0.0.0:14550 or tcp:192.168.4.1:5760 for the endpoint.".into());
                }
                let autopilot_hint = match &base.vehicle {
                    Vehicle::Physical { autopilot_hint, .. } => *autopilot_hint,
                    _ => None,
                };
                Vehicle::Physical {
                    endpoint: endpoint.into(),
                    autopilot_hint,
                }
            }
            2 => {
                let r = self.recording.text.trim();
                if r.len() > 200 || r.contains(['\n', '\r']) {
                    return Err("The recording is one session name or folder.".into());
                }
                Vehicle::replay((!r.is_empty()).then_some(r))
            }
            _ => self.simulated(base)?,
        };
        let mut sys = base.clone();
        sys.name = name;
        sys.vehicle = vehicle;
        let ros_agent = self.ros_agent && self.feeds(compat::Consumer::RosAgent);
        let domain = if !(ros_agent || self.recorder) {
            base.consumers.ros.domain
        } else {
            match self.domain.text.trim() {
                "" => DEFAULT_DOMAIN,
                d => d
                    .parse::<u32>()
                    .ok()
                    .filter(|d| *d <= 232)
                    .ok_or("The ROS domain is a number from 0 to 232.")?,
            }
        };
        let project = self.project.text.trim();
        if self.inference && !project.is_empty() && !token_ok(project) {
            return Err("The project is a project name from Data & models → Projects.".into());
        }
        let scripts = self.scripts && self.feeds(compat::Consumer::Scripts);
        let scripts_udp = self.scripts_udp.text.trim();
        if let Some(e) = scripts
            .then(|| scripts_address_error(scripts_udp))
            .flatten()
        {
            return Err(e);
        }
        let c = &mut sys.consumers;
        c.qgc = self.qgc;
        c.live_view = self.live_view;
        c.ros.agent = self.ros_agent;
        c.ros.recorder = self.recorder;
        c.ros.domain = domain;
        c.inference = self.inference.then(|| Inference {
            project: (!project.is_empty()).then(|| project.to_string()),
        });
        c.foxglove = self.foxglove;
        c.scripts_udp = self.scripts.then(|| scripts_udp.to_string());
        compat::prune(&mut sys);
        Ok(sys)
    }
}

//! The Settings → Systems editor's working copy and its validation.
use super::model::{
    DEFAULT_DOMAIN, DEFAULT_MODEL, DEFAULT_SCRIPTS_UDP, DEFAULT_VEHICLE, DEFAULT_WORLD, Inference,
    Source, System,
};
use rune::TextEdit;

pub(crate) struct Draft {
    /// The system being edited.
    pub id: String,
    pub name: TextEdit,
    /// `Source::index` of the chosen source.
    pub source: usize,
    pub world: TextEdit,
    pub model: TextEdit,
    pub vehicle: TextEdit,
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
    /// A working copy of `sys`. Detail fields of other sources hold their defaults so a source
    /// change starts from sensible values.
    pub fn of(sys: &System) -> Self {
        let text = |v: &str| TextEdit::new(v);
        let (mut world, mut model, mut vehicle) = (
            text(DEFAULT_WORLD),
            text(DEFAULT_MODEL),
            text(DEFAULT_VEHICLE),
        );
        let (mut endpoint, mut recording) = (text(""), text(""));
        match &sys.source {
            Source::Gazebo { world: w, model: m } => {
                world = text(w);
                model = text(m);
            }
            Source::ArduPilot { vehicle: v } => vehicle = text(v),
            Source::Physical { endpoint: e } => endpoint = text(e),
            Source::Replay { recording: r } => recording = text(r.as_deref().unwrap_or("")),
            Source::Sih => {}
        }
        let c = &sys.consumers;
        Self {
            id: sys.id.clone(),
            name: text(&sys.name),
            source: sys.source.index(),
            world,
            model,
            vehicle,
            endpoint,
            recording,
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

    /// `base` with this draft's name, source and consumers, or what to fix first.
    pub fn apply(&self, base: &System) -> Result<System, String> {
        let name = super::clean_name(&self.name.text)?;
        let source = match self.source {
            1 => {
                let (world, model) = (self.world.text.trim(), self.model.text.trim());
                if !token_ok(world) || !token_ok(model) {
                    return Err("Gazebo world and model are names such as default and x500_mono_cam (letters, digits, _ - .).".into());
                }
                Source::Gazebo {
                    world: world.into(),
                    model: model.into(),
                }
            }
            2 => {
                let vehicle = self.vehicle.text.trim();
                if !token_ok(vehicle) {
                    return Err(
                        "The ArduPilot vehicle is a name such as copter, plane or rover.".into(),
                    );
                }
                Source::ArduPilot {
                    vehicle: vehicle.into(),
                }
            }
            3 => {
                let endpoint = self.endpoint.text.trim();
                if !endpoint_ok(endpoint) {
                    return Err("Use serial:COM4:115200, udp:0.0.0.0:14550 or tcp:192.168.4.1:5760 for the endpoint.".into());
                }
                Source::Physical {
                    endpoint: endpoint.into(),
                }
            }
            4 => {
                let r = self.recording.text.trim();
                if r.len() > 200 || r.contains(['\n', '\r']) {
                    return Err("The recording is one session name or folder.".into());
                }
                Source::Replay {
                    recording: (!r.is_empty()).then(|| r.to_string()),
                }
            }
            _ => Source::Sih,
        };
        // Consumers the editor hides for this source are saved off, so a hidden toggle left
        // over from another source never shapes components or lists.
        let live_view = self.live_view && matches!(source, Source::Sih | Source::Physical { .. });
        let ros_agent = self.ros_agent && source.simulated();
        let scripts = self.scripts && !matches!(source, Source::Replay { .. });
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
        let scripts_udp = self.scripts_udp.text.trim();
        if scripts && !udp_address_ok(scripts_udp) {
            return Err("User scripts listen on a host:port such as 127.0.0.1:14540.".into());
        }
        let mut sys = base.clone();
        sys.name = name;
        sys.source = source;
        let c = &mut sys.consumers;
        c.qgc = self.qgc;
        c.live_view = live_view;
        c.ros.agent = ros_agent;
        c.ros.recorder = self.recorder;
        c.ros.domain = domain;
        c.inference = self.inference.then(|| Inference {
            project: (!project.is_empty()).then(|| project.to_string()),
        });
        c.foxglove = self.foxglove;
        c.scripts_udp = scripts.then(|| scripts_udp.to_string());
        Ok(sys)
    }
}

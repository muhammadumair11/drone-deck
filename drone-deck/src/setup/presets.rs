//! Workflow presets: the setup recipes a system needs, computed from its vehicle and consumers
//! instead of picked from a fixed list, so any combination of autopilot, world and consumers
//! has one. Checkpoints keep the recipe list itself; the six names earlier versions saved
//! resolve through [`LEGACY`], so an interrupted queue from them still resumes.
use crate::systems::System;
#[cfg(test)]
#[path = "presets_tests.rs"]
mod tests;

/// A named list of setup recipe ids, in installation order.
#[derive(Clone, Debug)]
pub(crate) struct Preset {
    pub name: String,
    pub recipes: Vec<String>,
}
/// Two presets are the same workflow when they install the same recipes.
impl PartialEq for Preset {
    fn eq(&self, other: &Self) -> bool {
        self.recipes == other.recipes
    }
}
impl Default for Preset {
    fn default() -> Self {
        for_system(&crate::systems::model::template(0, "", ""))
    }
}

/// The fixed presets of earlier versions, by the names their checkpoints saved, with the
/// recipes each installed. Their names are kept for the systems that still compute them.
const LEGACY: &[(&str, &[&str])] = &[
    ("PX4 SIH simulation", &["px4", "qgc"]),
    (
        "PX4 + Gazebo with ROS 2 and recordings",
        &[
            "px4",
            "gazebo",
            "ros2",
            "ros-gz",
            "dds",
            "ros-workspace",
            "qgc",
        ],
    ),
    ("ArduPilot development", &["ardupilot", "qgc"]),
    ("Offline CPU inference", &["onnx"]),
    ("Physical vehicle", &["qgc"]),
    // Dropped "(read-only link)": the preset installs QGroundControl, which is bridged both
    // ways when enabled.
    ("Physical vehicle (read-only link)", &["qgc"]),
    (
        "PX4 SIH with ROS 2 and recordings",
        &["px4", "ros2", "dds", "ros-workspace", "qgc"],
    ),
];

/// The recipes `sys` needs, in installation order: its simulator's, then the autopilot ↔ ROS 2
/// stack when the bridge runs (ROS 2, the Gazebo camera bridge, then the adapter's agent and
/// messages), then local inference and QGroundControl. A physical vehicle always gets
/// QGroundControl, where vehicle setup happens.
pub(crate) fn for_system(sys: &System) -> Preset {
    let c = &sys.consumers;
    let adapter = sys.adapter();
    let mut recipes: Vec<&str> = adapter.map_or(&[][..], |a| a.recipes).to_vec();
    if let (true, Some(ros)) = (c.ros.agent, adapter.and_then(|a| a.ros.as_ref())) {
        recipes.push("ros2");
        if ros.extra.contains(&"rosgz") {
            recipes.push("ros-gz");
        }
        recipes.extend(ros.recipes);
    }
    if c.inference.is_some() {
        recipes.push("onnx");
    }
    if c.qgc || sys.is_physical() {
        recipes.push("qgc");
    }
    let recipes: Vec<String> = recipes.into_iter().map(String::from).collect();
    Preset {
        name: name(sys, &recipes),
        recipes,
    }
}

/// The earlier preset name for these recipes, or one made from the vehicle and consumers.
fn name(sys: &System, recipes: &[String]) -> String {
    // QGroundControl alone was the physical preset; a replay with it is not one.
    let legacy = sys.is_physical() || recipes != ["qgc"];
    if let Some((n, _)) = LEGACY.iter().find(|(_, r)| legacy && *r == recipes) {
        return (*n).to_string();
    }
    let base = match sys.adapter() {
        Some(a) => a.name.to_string(),
        None if sys.is_physical() => "Physical vehicle".into(),
        None if sys.is_replay() => "Recording replay".into(),
        None => sys.summary(),
    };
    let c = &sys.consumers;
    let mut with = Vec::new();
    if recipes.iter().any(|r| r == "dds") {
        with.push("ROS 2");
    }
    if c.inference.is_some() {
        with.push("inference");
    }
    if recipes.iter().any(|r| r == "qgc") {
        with.push("QGroundControl");
    }
    match with.as_slice() {
        [] => base,
        [one] => format!("{base} with {one}"),
        [rest @ .., last] => format!("{base} with {} and {last}", rest.join(", ")),
    }
}

/// The preset a checkpoint saved: its recipe list when it has one, else the recipes of the
/// earlier preset named `name`. `None` when neither is known.
pub(crate) fn resolve(name: &str, recipes: Option<Vec<String>>) -> Option<Preset> {
    let recipes = recipes.or_else(|| {
        LEGACY
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, r)| r.iter().map(|id| id.to_string()).collect())
    })?;
    let name = LEGACY
        .iter()
        .find(|(_, r)| *r == recipes)
        .map_or(name, |(n, _)| n)
        .to_string();
    Some(Preset { name, recipes })
}

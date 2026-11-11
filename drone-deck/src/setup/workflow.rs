//! Workflow presets: the tools a system needs, checked one at a time and listed for review.
use super::*;
use crate::systems::Tone;
use crate::ui::kit::*;
use crate::ui::theme::*;

pub(crate) const PRESETS: &[(&str, &[&str])] = &[
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
    (
        "PX4 SIH with ROS 2 and recordings",
        &["px4", "ros2", "dds", "ros-workspace", "qgc"],
    ),
];

/// Earlier names of presets (old name, current name). Checkpoints name their preset, so a
/// checkpoint saved under an old name still resumes. "(read-only link)" was dropped because
/// the preset installs QGroundControl, which is bridged both ways when enabled.
const RENAMED: &[(&str, &str)] = &[("Physical vehicle (read-only link)", "Physical vehicle")];

/// The position in [`PRESETS`] of the preset saved as `name`, current or earlier.
pub(crate) fn preset_index(name: &str) -> Option<usize> {
    let name = RENAMED
        .iter()
        .find(|(old, _)| *old == name)
        .map_or(name, |(_, new)| new);
    PRESETS.iter().position(|(n, _)| *n == name)
}

pub(crate) fn ids(s: &State) -> &'static [&'static str] {
    PRESETS[s.setup.workflow.min(PRESETS.len() - 1)].1
}
/// Any tool of the current preset runs inside WSL.
pub(crate) fn needs_wsl(s: &State) -> bool {
    ids(s).iter().any(|id| {
        s.setup
            .recipes
            .iter()
            .any(|r| r.id == *id && r.target == "WSL")
    })
}
/// Starts detecting every tool of the current preset. Nothing is installed or changed.
pub(crate) fn scan(s: &mut State) {
    if s.headless {
        s.setup.message = "Headless preview: tool discovery is disabled.".into();
        return;
    }
    if s.setup.pending {
        return;
    }
    s.setup.queue = ids(s)
        .iter()
        .filter_map(|id| s.setup.recipe_index(id))
        .collect();
    s.setup.checks.clear();
    s.setup.message =
        "Checking existing components. Nothing will be installed or reconfigured.".into();
}
pub(super) fn poll(s: &mut State) -> bool {
    if s.setup.pending || s.setup.queue.is_empty() {
        return false;
    }
    let index = s.setup.queue.remove(0);
    choose(s, index);
    if let Some(location) = s.setup.overrides.get(&s.setup.recipes[index].id) {
        s.setup.location = TextEdit::new(location);
    }
    request(s, "detect");
    true
}
/// A check result that belongs to the selected environment.
pub(crate) fn valid<'a>(s: &'a State, id: &str) -> Option<&'a Candidate> {
    s.setup.checks.get(id).filter(|c| {
        c.environment.is_empty()
            || c.environment
                .eq_ignore_ascii_case(s.setup.distro.text.trim())
    })
}
/// Every tool of the preset has a result in the selected environment.
pub(crate) fn checked_all(s: &State) -> bool {
    ids(s).iter().all(|id| valid(s, id).is_some())
}
pub(crate) fn verified_all(s: &State) -> bool {
    ids(s)
        .iter()
        .all(|id| valid(s, id).is_some_and(|c| c.compatible))
}
/// The tool currently being detected, as (position, total), while a check runs.
pub(crate) fn progress(s: &State) -> Option<(usize, usize)> {
    if s.setup.queue.is_empty() && !s.setup.pending {
        return None;
    }
    let total = ids(s).len();
    Some((total.saturating_sub(s.setup.queue.len()), total))
}
/// Status label, tone and the second line for one preset tool.
pub(crate) fn status(s: &State, id: &str) -> (&'static str, &'static str, Tone) {
    if let (Some(batch), Some(index)) = (&s.setup.batch, s.setup.recipe_index(id)) {
        let name = &s.setup.recipes[index].name;
        let current = batch
            .waiting()
            .and_then(|job| s.setup.jobs.iter().find(|j| j.id == job))
            .is_some_and(|j| &j.tool == name);
        if current {
            return ("Installing", "Configured after verification", Tone::Active);
        }
        if batch.remaining().contains(&index) {
            return ("Queued", "Installs after the current job", Tone::Idle);
        }
    }
    let stopped = s
        .setup
        .selected_job
        .as_ref()
        .filter(|_| s.setup.batch.is_none())
        .and_then(|job| s.setup.jobs.iter().find(|j| &j.id == job))
        .filter(|j| {
            !j.alive
                && s.setup
                    .recipe_index(id)
                    .is_some_and(|i| s.setup.recipes[i].name == j.tool)
        })
        .map(|j| j.status.as_str());
    let found = valid(s, id).is_some_and(|c| c.compatible);
    match stopped {
        Some("Needs restart") if !found => {
            return (
                "Needs restart",
                "Restart Windows, then check again",
                Tone::Warn,
            );
        }
        Some("Needs input") if !found => {
            return ("Needs input", "Review the job log", Tone::Warn);
        }
        Some("Failed" | "Interrupted" | "Needs attention") if !found => {
            return (
                "Install stopped",
                "Review the job log, then retry",
                Tone::Bad,
            );
        }
        _ => {}
    }
    match valid(s, id) {
        Some(c) if c.compatible => ("Found", "Reuse after verification", Tone::Good),
        Some(c) if c.found => (
            "Found / incompatible",
            "Install reviewed recipe",
            Tone::Warn,
        ),
        Some(_) => ("Not found", "Review setup", Tone::Warn),
        None if s.setup.queue.iter().any(|i| s.setup.recipes[*i].id == id) => {
            ("Waiting", "Queued for checking", Tone::Idle)
        }
        None => ("Not checked", "Check this computer first", Tone::Idle),
    }
}
/// Applies a verified existing installation to its launch profile.
pub(crate) fn reuse(s: &mut State, id: &str) {
    let (Some(index), Some(candidate)) = (s.setup.recipe_index(id), valid(s, id).cloned()) else {
        return;
    };
    choose(s, index);
    s.setup.candidate = Some(candidate);
    profiles::adopt(s);
    if s.setup.last_applied.is_some() {
        let name = s.setup.recipes[index].name.clone();
        activity::record(s, format!("Reused verified installation: {name}"));
    }
}

fn source(s: &State, id: &str, recipe: &Recipe) -> El {
    let mut lines = col()
        .w_full()
        .min_w(0.)
        .gap(8.)
        .child(p(&recipe.summary, 12.).max_w(760.))
        .child(p(
            format!(
                "Version {} / {}",
                recipe.version,
                if recipe.target == "WSL" {
                    format!("WSL / {}", s.setup.distro.text.trim())
                } else {
                    recipe.target.clone()
                }
            ),
            12.,
        ));
    if id == "px4" {
        lines = lines.child(p("Reusing PX4 saves one folder for this Ubuntu distribution. Linked SIH and Gazebo profiles use it together; custom launch commands keep their own configuration.", 12.));
    }
    if !recipe.destination.is_empty() {
        lines = lines.child(p(format!("Destination: {}", recipe.destination), 12.));
    }
    if !recipe.privileges.is_empty() {
        lines = lines.child(p(format!("Privileges: {}", recipe.privileges), 12.));
    }
    if !recipe.verification.is_empty() {
        lines = lines.child(p(format!("Verification: {}", recipe.verification), 12.));
    }
    if let Some(c) = valid(s, id).filter(|c| c.found) {
        lines = lines.child(p(
            format!(
                "Found {} / {}",
                if c.version.is_empty() {
                    "version unknown"
                } else {
                    &c.version
                },
                if c.managed {
                    "app-managed location"
                } else {
                    "existing installation, not owned by Drone Deck"
                }
            ),
            12.,
        ));
        if !c.path.is_empty() {
            lines = lines.child(code(c.path.clone(), 11., muted()).w_full().wrap_anywhere());
        }
    }
    lines
}

/// One numbered dependency row (`.log-row`: number | name + detail | status).
/// `extra` (a disclosure) closes the row: under the main text, or under the status when compact.
pub(crate) fn row_layout(
    key: impl Into<String>,
    lead: &str,
    main: El,
    pull: El,
    extra: El,
    compact: bool,
) -> El {
    let r = ruled(true).key(key).py(22.).items_start();
    if compact {
        r.gap(14.).child(meta(lead).w(24.).pt(4.)).child(
            col()
                .grow(1.)
                .min_w(0.)
                .gap(10.)
                .child(main)
                .child(pull)
                .child(extra),
        )
    } else {
        r.gap(24.)
            .child(meta(lead).w(40.).pt(4.))
            .child(col().grow(1.).min_w(0.).child(main).child(extra))
            .child(col().w(150.).min_w(0.).child(pull))
    }
}

/// The preset's tools with their real check status (`workflow-wizard`). `builtin` rows come
/// first: parts of the system that need no installation.
pub(crate) fn dependencies(s: &State, compact: bool, builtin: &[(&str, &str, &str)]) -> El {
    let mut list = col().key("workflow-wizard").w_full().min_w(0.);
    for (n, (name, role, note)) in builtin.iter().enumerate() {
        list = list.child(row_layout(
            format!("dependency-builtin-{n}"),
            &format!("{:02}", n + 1),
            col()
                .w_full()
                .min_w(0.)
                .gap(6.)
                .child(h3(*name, 17.))
                .child(p(*role, 12.)),
            col()
                .gap(4.)
                .child(meta_sized("Built in", 10., fg()))
                .child(meta(*note).wrap()),
            el(),
            compact,
        ));
    }
    for (i, id) in ids(s).iter().enumerate() {
        let Some(recipe) = s.setup.recipes.iter().find(|r| r.id == *id) else {
            continue;
        };
        let (label, next, tone) = status(s, id);
        let reason = valid(s, id).map(|c| c.reason.clone()).unwrap_or_default();
        let mut main = col()
            .w_full()
            .min_w(0.)
            .gap(6.)
            .child(h3(&recipe.name, 17.))
            .child(p(format!("{} / {}", recipe.version, recipe.target), 12.));
        if !reason.is_empty() {
            main = main.child(p(reason, 12.).max_w(760.));
        }
        let rid = id.to_string();
        let details = disclosure(
            s,
            &format!("source-{id}"),
            "Source, location & verification",
            || source(s, &rid, recipe),
        );
        let mut pull = col()
            .gap(4.)
            .child(
                row()
                    .gap(8.)
                    .center_y()
                    .child(dot(systems::tone_color(tone)))
                    .child(meta_sized(label, 10., fg()).wrap()),
            )
            .child(meta(next).wrap());
        if valid(s, id).is_some_and(|c| c.compatible) {
            let rid = id.to_string();
            pull = pull.child(ghost(
                format!("workflow-reuse-{id}"),
                "Reuse now",
                !s.busy() && !s.setup.active(),
                move |s| reuse(s, &rid),
            ));
        }
        list = list.child(row_layout(
            format!("dependency-{i}"),
            &format!("{:02}", builtin.len() + i + 1),
            main,
            pull,
            details,
            compact,
        ));
    }
    list
}

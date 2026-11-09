use super::*;
use crate::ui::*;
pub(super) const PRESETS: &[(&str, &[&str])] = &[
    (
        "Lightweight visual simulation",
        &["px4", "jmavsim", "qgc", "mavsdk"],
    ),
    (
        "PX4 with ROS 2 and recordings",
        &["px4", "ros2", "dds", "ros-workspace", "qgc"],
    ),
    ("ArduPilot development", &["ardupilot", "qgc"]),
    ("Offline CPU inference", &["onnx"]),
    ("CPU training and inference", &["torch", "onnx"]),
];
pub(super) fn scan(s: &mut State) {
    if s.headless {
        s.setup.message = "Preview: workflow discovery disabled.".into();
        return;
    }
    if s.setup.pending {
        return;
    }
    s.setup.queue = PRESETS[s.setup.workflow]
        .1
        .iter()
        .filter_map(|id| s.setup.recipes.iter().position(|r| r.id == *id))
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
    request(s, "detect");
    true
}
fn valid<'a>(s: &'a State, id: &str) -> Option<&'a Candidate> {
    s.setup.checks.get(id).filter(|c| {
        c.environment.is_empty()
            || c.environment
                .eq_ignore_ascii_case(s.setup.distro.text.trim())
    })
}
pub(super) fn view(s: &State, width: f32) -> El {
    let mut goals = row().gap(8.).flex_wrap();
    for (index, (name, _)) in PRESETS.iter().enumerate() {
        goals = goals.child(
            button(
                format!("workflow-{index}"),
                *name,
                s.setup.workflow == index,
                move |s| {
                    s.setup.workflow = index;
                    s.setup.queue.clear();
                },
            )
            .disabled(s.setup.pending || s.setup.batch.is_some()),
        );
    }
    let mut steps = col().w_full().min_w(0.).gap(10.);
    for id in PRESETS[s.setup.workflow].1 {
        let index = s.setup.recipes.iter().position(|r| r.id == *id).unwrap();
        let recipe = &s.setup.recipes[index];
        let result = valid(s, id);
        let status = match result {
            Some(c) if c.compatible => "Verified installation",
            Some(c) if c.found => "Needs compatibility review",
            Some(_) => "Not found",
            None => "Not checked",
        };
        let detail = result
            .map(|c| c.reason.as_str())
            .unwrap_or("Check this workflow to discover existing tools.");
        let mut actions = row().gap(8.).flex_wrap();
        if result.is_some_and(|c| c.compatible) {
            let candidate = result.unwrap().clone();
            actions = actions.child(
                button(
                    format!("workflow-reuse-{id}"),
                    "Use this installation",
                    true,
                    move |s| {
                        choose(s, index);
                        s.setup.candidate = Some(candidate.clone());
                        profiles::adopt(s);
                    },
                )
                .disabled(s.busy()),
            );
        }
        actions = actions.child(
            button(
                format!("workflow-configure-{id}"),
                if recipe.automatic {
                    "Review setup"
                } else {
                    "Set up / select location"
                },
                false,
                move |s| {
                    choose(s, index);
                    if s.setup.recipes[index].automatic {
                        review(s);
                    }
                },
            )
            .on_press_tx(move |s, tx| {
                choose(s, index);
                if s.setup.recipes[index].automatic {
                    review(s);
                }
                tx.scroll_into_view_in(
                    "main-scroll",
                    if s.setup.recipes[index].automatic {
                        "installation-review"
                    } else {
                        "setup-tool-catalog"
                    },
                );
            })
            .disabled(s.setup.pending),
        );
        let info = col()
            .grow(1.)
            .min_w(0.)
            .gap(4.)
            .child(title(&recipe.name, 14.).wrap())
            .child(label(status, 12., muted()).wrap())
            .child(label(detail, 11., muted()).wrap());
        let content = if width < 680. {
            col().gap(8.).child(info).child(actions)
        } else {
            row()
                .gap(12.)
                .justify_between()
                .center_y()
                .child(info)
                .child(actions)
        };
        steps = steps.child(
            col()
                .w_full()
                .min_w(0.)
                .padding(10.)
                .radius(6.)
                .border(1., line())
                .child(content),
        );
    }
    let verified = PRESETS[s.setup.workflow]
        .1
        .iter()
        .filter(|id| valid(s, id).is_some_and(|c| c.compatible))
        .count();
    let connection = if s.setup.workflow == 1 {
        "After installation: create a ROS project with firmware-matched px4_msgs, start the Agent, and verify /fmu/out topics. This checklist verifies tools; it does not claim a working ROS vehicle connection."
    } else if s.setup.workflow == 2 {
        "ArduPilot remains an advanced integration. Verify its SITL build and dependencies before launch."
    } else if s.setup.workflow == 0 {
        "After configuration: Start SIH workspace waits for SIH status and live relay packets. Connect QGroundControl and confirm flight readiness there."
    } else {
        "After configuration: create a CPU inference/training project in AI & data, select prepared inputs, then run it and inspect its artifacts. Model/task compatibility needs a real input test."
    };
    card().key("workflow-wizard").child(heading("Set up a complete workflow","1 CHOOSE · 2 CHECK · 3 CONFIGURE · 4 CONNECT"))
        .child(goals).child(label(format!("Target: {} · {verified}/{} tool checks passed",s.setup.distro.text,PRESETS[s.setup.workflow].1.len()),14.,ink()).wrap())
        .child(button("check-workflow","Check this workflow",true,scan).disabled(s.setup.pending||s.setup.batch.is_some()||!s.setup.queue.is_empty()))
        .child(label("Existing tools are checked first. Each installation and configuration change remains an explicit action. Choose your Linux environment below if needed.",13.,muted()).wrap())
        .child(steps).child(super::batch::view(s)).child(label(connection,13.,ink()).wrap())
}

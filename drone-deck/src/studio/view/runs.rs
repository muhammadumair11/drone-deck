//! Projects: editable autonomy projects in the selected Ubuntu and their runs.
use super::*;

const KINDS: [&str; 3] = ["ONNX inference", "ROS 2 subscriber", "MAVSDK monitor"];

fn kind_name(kind: &str) -> &str {
    match kind {
        "onnx" => KINDS[0],
        "ros2" => KINDS[1],
        "mavsdk" => KINDS[2],
        other => other,
    }
}

fn run_state(s: &State, id: &str) -> (String, rune::Color) {
    match s.status.get(id).map(String::as_str) {
        Some("Running") => ("Running".into(), fg()),
        Some("Starting") => ("Starting".into(), fg()),
        Some("Exited 0") => ("Exited 0".into(), success()),
        Some(other) if other.starts_with("Exited") || other == "Failed" => (other.into(), danger()),
        _ => ("Not running".into(), muted()),
    }
}

fn list(s: &State, vp: &Viewport) -> El {
    let r = &s.studio;
    let mut list = col().key("project-list").w_full().min_w(0.);
    let projects = r.library["projects"].as_array();
    for project in projects.into_iter().flatten() {
        let name = project["name"].as_str().unwrap_or("").to_string();
        let id = format!("project-{name}");
        let running = s
            .status
            .get(&id)
            .is_some_and(|v| v == "Running" || v == "Starting");
        let (state, tone) = run_state(s, &id);
        let compact = vp.compact();
        let kind = lead(
            kind_name(project["kind"].as_str().unwrap_or("")),
            muted(),
            compact,
        );
        let process = row()
            .w_full()
            .min_w(0.)
            .gap(6.)
            .center_y()
            .child(dot(tone))
            .child(meta_sized(state, 9., fg()).wrap().grow(1.));
        let mut main = col()
            .w_full()
            .min_w(0.)
            .gap(4.)
            .child(h3(name.clone(), 15.))
            .child(code(project["path"].as_str().unwrap_or(""), 11., muted()).wrap_anywhere());
        // On compact the 56px lead only holds the kind; the process state moves under the name.
        let lead = if compact {
            main = main.child(process);
            kind
        } else {
            col().w_full().min_w(0.).gap(6.).child(kind).child(process)
        };
        let here = project["distro"].as_str() == Some(s.config.distro.as_str());
        if !here {
            main = main.child(body(
                "Belongs to another WSL distribution. Select it in Settings to run this project.",
                11.,
                warn(),
            ));
        }
        let value = project.clone();
        let pull = ghost(
            format!("run-{id}"),
            if running {
                "Stop project"
            } else {
                "Run project"
            },
            !r.pending && here,
            move |s| {
                let name = value["name"].as_str().unwrap_or("").to_string();
                let id = format!("project-{name}");
                let before = systems::running(s, &id);
                // `run` reports every refusal in the status message; a stop that went out leaves it empty.
                let previous = std::mem::take(&mut s.studio.message);
                super::super::projects::run(s, value.clone());
                if before && s.studio.message.is_empty() {
                    s.studio.message = format!("Stop requested for project {name}.");
                    activity::record(s, format!("Requested stop of project {name}"));
                } else if !before && systems::running(s, &id) {
                    activity::record(s, format!("Started project {name}"));
                } else if s.studio.message.is_empty() {
                    s.studio.message = previous;
                }
            },
        );
        list = list.child(log_row(
            format!("project-row-{name}"),
            lead,
            main,
            pull,
            vp.compact(),
        ));
    }
    if projects.is_none_or(Vec::is_empty) {
        let (title, text) = if !s.wsl_ready() {
            (
                "Linux environment not ready",
                "Projects are editable source inside the selected Ubuntu. Select a working WSL 2 distribution in Settings first.",
            )
        } else if projects.is_some() {
            (
                "No projects yet",
                "Create a project above. Nothing is installed or started until you choose Run project.",
            )
        } else {
            (
                "Projects not loaded",
                "Refresh to list projects in the selected Ubuntu.",
            )
        };
        list = list.child(empty("projects-empty", title, text));
    }
    col()
        .w_full()
        .min_w(0.)
        .pt(16.)
        .child(section_head(
            "Projects",
            meta("Output appears in Activity / Process output"),
            vp.compact(),
        ))
        .child(list)
}

pub(super) fn projects(s: &State, vp: &Viewport) -> El {
    let r = &s.studio;
    let ready = s.wsl_ready();
    let left = col()
        .key("project-workspace")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(h2("Editable autonomy projects", 22.))
        .child(p(
            "Creates editable source and a manifest without installing or starting code. ROS 2 and MAVSDK starters only monitor; they never arm or fly.",
            12.,
        ))
        .child(labeled(
            "Project name",
            field(&r.project_name, "project-name", "drone-project")
                .on_edit(|s: &mut State| &mut s.studio.project_name),
        ))
        .child(tabs("project-kind", &KINDS, r.kind.min(2), |s: &mut State, i: usize| {
            s.studio.kind = i
        }))
        .child(
            row()
                .gaps(8., 20.)
                .flex_wrap()
                .center_y()
                .child(pill("create-project", "Create project", ready && !r.pending, |s| {
                    let name = s.studio.project_name.text.trim().to_string();
                    act(s, "project_create", format!("Requested project {name}"))
                }))
                .child(ghost("refresh-projects", "Refresh projects", ready && !r.pending, |s| {
                    request(s, "data_library")
                }))
                .child(ghost("project-setup", "Prepare AI runtimes ↗", true, |s| {
                    if s.systems.active != systems::AI {
                        s.select_system(systems::AI);
                    }
                    if s.systems.active == systems::AI {
                        crate::guided::open(s);
                    }
                })),
        );
    let linked = |label: &str, value: Option<&str>, hint: &str| match value {
        Some(v) => kv(label, v, true),
        None => col()
            .w_full()
            .min_w(0.)
            .gap(2.)
            .child(meta(label))
            .child(p(hint, 11.)),
    };
    let dataset = r.dataset.as_ref().and_then(|d| d["name"].as_str());
    let model = (!r.model_path.is_empty()).then_some(r.model_path.as_str());
    let right = col()
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(meta("Linked at creation"))
        .child(linked(
            "Model",
            model,
            "None selected / choose one under Models",
        ))
        .child(linked(
            "Dataset",
            dataset,
            "None selected / choose a version under Datasets & evaluation",
        ))
        .child(meta("Run inputs"))
        .child(labeled(
            "Linux ONNX model path",
            field(&r.model_file, "project-model-file", "/home/user/model.onnx")
                .on_edit(|s: &mut State| &mut s.studio.model_file),
        ))
        .child(labeled(
            "NPZ inputs path",
            field(&r.inputs, "project-inputs", "/home/user/inputs.npz")
                .on_edit(|s: &mut State| &mut s.studio.inputs),
        ))
        .child(notice(
            "ONNX inference needs a model and correctly preprocessed NPZ tensors.",
        ));
    let mut page = col().w_full().min_w(0.);
    if !ready {
        page = page.child(helpstrip(
            "projects-environment",
            None,
            "Projects run inside the selected Ubuntu. Select a working WSL 2 distribution before creating or running one.",
            ghost("projects-environment-settings", "Environment settings ↗", true, |s| {
                s.navigate(Page::Settings)
            }),
            vp.compact(),
        ));
    }
    page.child(asset(left, right, vp.compact()))
        .child(list(s, vp))
}

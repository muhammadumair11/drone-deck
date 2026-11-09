use super::*;
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
pub(super) fn run(s: &mut State, project: Value) {
    if s.headless {
        s.studio.message = "Preview: project execution disabled.".into();
        return;
    }
    if !s.wsl_ready() || s.setup.batch.is_some() || s.setup.jobs.iter().any(|j| j.alive) {
        s.studio.message =
            "Select a working WSL environment and finish installations first.".into();
        return;
    }
    if project["distro"].as_str() != Some(s.config.distro.as_str()) {
        s.studio.message = "This project belongs to another WSL distribution.".into();
        return;
    }
    let name = project["name"].as_str().unwrap_or("");
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return;
    }
    let id = format!("project-{name}");
    if s.status
        .get(&id)
        .is_some_and(|v| v == "Running" || v == "Starting")
    {
        s.send(Request::Stop(id));
        return;
    }
    let path = project["path"].as_str().unwrap_or("");
    let script = quote(&format!("{path}/main.py"));
    let kind = project["kind"].as_str().unwrap_or("");
    let profile_id = match kind {
        "onnx" => "onnx",
        "ros2" => "ros-workspace",
        "mavsdk" => "telemetry",
        "training" => {
            s.studio.message = "Drone Deck no longer runs PyTorch training projects. The project files are kept; run its main.py in your own environment.".into();
            return;
        }
        _ => {
            s.studio.message = "Unknown project type. Review its manifest.".into();
            return;
        }
    };
    let Some(profile) = s.config.profiles.iter().find(|p| p.id == profile_id) else {
        let recipe = if profile_id == "telemetry" {
            "mavsdk"
        } else {
            profile_id
        };
        let name = s
            .setup
            .recipes
            .iter()
            .find(|r| r.id == recipe)
            .map_or(profile_id, |r| r.name.as_str());
        s.studio.message = format!(
            "Install or adopt {profile_id} first: Tools & config → All registered tools → {name} → Review installation."
        );
        return;
    };
    let configured = profile.args.join(" ");
    let command = match kind {
        "onnx" => {
            if s.studio.model_file.text.trim().is_empty() || s.studio.inputs.text.trim().is_empty()
            {
                s.studio.message =
                    "Enter an ONNX file and a correctly preprocessed NPZ input file first.".into();
                return;
            }
            let Some((python, _)) = configured.split_once(" -c ") else {
                s.studio.message = "Apply the verified ONNX runtime configuration first.".into();
                return;
            };
            format!(
                "{python} {script} --model {} --inputs {} --output {}",
                quote(s.studio.model_file.text.trim()),
                quote(s.studio.inputs.text.trim()),
                quote(&format!("{path}/artifacts"))
            )
        }
        "ros2" => {
            let Some((environment, _)) = configured.rsplit_once(" && ros2 ") else {
                s.studio.message = "Apply the verified PX4 ROS workspace first.".into();
                return;
            };
            format!("{environment} && python3 {script}")
        }
        _ => {
            let Some((python, _)) = configured.split_once(" -u ") else {
                s.studio.message =
                    "Apply the verified MAVSDK interpreter configuration first.".into();
                return;
            };
            format!("{python} -u {script}")
        }
    };
    let profile = backend::Profile {
        id: id.clone(),
        name: format!("Project {name}"),
        category: "AI & data".into(),
        description: "User project with local runtime/assets".into(),
        program: "wsl".into(),
        args: vec![command],
        cwd: String::new(),
        probe: String::new(),
        docs: String::new(),
        download_url: String::new(),
        exclusive: false,
    };
    s.status.insert(id, "Starting".into());
    s.send(Request::Start(profile, s.config.distro.clone()));
    s.studio.message="Project requested. Output appears in the process console; Stop session includes project processes.".into();
}

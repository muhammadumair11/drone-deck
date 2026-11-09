use super::*;
pub(super) fn can_reconfigure(s: &mut State) -> bool {
    if s.busy() {
        s.setup.message="Stop the active workspace/recording before changing its environment or launch configuration.".into();
        false
    } else {
        true
    }
}
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
pub(crate) fn adopt(s: &mut State) {
    s.setup.last_applied = None;
    let Some(candidate) = s.setup.candidate.clone() else {
        return;
    };
    if !candidate.found || !candidate.compatible {
        s.setup.message="This discovery result needs manual compatibility/setup checks before automatic adoption.".into();
        return;
    }
    if candidate.tool != s.setup.recipes[s.setup.selected].id {
        s.setup.message = "Select and detect the matching tool before applying this result.".into();
        return;
    }
    if !["qgc", "jmavsim"].contains(&candidate.tool.as_str())
        && (candidate.environment.is_empty()
            || !candidate
                .environment
                .eq_ignore_ascii_case(s.setup.distro.text.trim()))
    {
        s.setup.message="This result belongs to another WSL environment. Detect the tool in the selected environment first.".into();
        return;
    }
    if !can_reconfigure(s) {
        return;
    }
    let id = match candidate.tool.as_str() {
        "px4" => "sih",
        "mavsdk" => "telemetry",
        other => other,
    };
    let mut config = s.config.clone();
    if !config.profiles.iter().any(|p| p.id == id)
        && ["ros-workspace", "onnx", "torch"].contains(&id)
    {
        config.profiles.push(backend::Profile {
            id: id.into(),
            name: match id {
                "onnx" => "ONNX CPU runtime",
                "torch" => "PyTorch CPU training",
                _ => "PX4 ROS 2 workspace",
            }
            .into(),
            category: if id != "ros-workspace" {
                "AI & data".into()
            } else {
                "Development".into()
            },
            description: candidate.reason.clone(),
            program: "wsl".into(),
            args: Vec::new(),
            cwd: String::new(),
            probe: String::new(),
            docs: String::new(),
            download_url: String::new(),
            exclusive: false,
        });
    }
    let Some(profile) = config.profiles.iter_mut().find(|p| p.id == id) else {
        s.setup.message = "Add a matching tool profile in Integrations first.".into();
        return;
    };
    let exe = quote(&candidate.executable);
    match candidate.tool.as_str() {
        "ros2" => {
            profile.program = "wsl".into();
            profile.args = vec!["source /opt/ros/jazzy/setup.bash && ros2 topic list".into()];
            profile.probe = "test -f /opt/ros/jazzy/setup.bash".into();
        }
        "mavsdk" => {
            profile.program = "wsl".into();
            profile.args = vec![format!("{exe} -u \"$DRONE_DECK_SCRIPTS/telemetry.py\"")];
            profile.probe = format!("test -x {exe}");
        }
        "px4" => {
            profile.program = "wsl".into();
            let target = if candidate.build_target == "px4_sitl_sih" {
                "px4_sitl_sih"
            } else {
                "px4_sitl"
            };
            profile.args = vec![format!(
                "cd {} && test -z \"$(pgrep -x px4)\" && make {target} sihsim_quadx",
                quote(&candidate.path)
            )];
            profile.probe = format!("test -x {exe}");
        }
        "dds" => {
            profile.program = "wsl".into();
            profile.args = vec![format!("{exe} udp4 -p 8888")];
            profile.probe = format!("test -x {exe}");
        }
        "ros-workspace" => {
            profile.program = "wsl".into();
            profile.args = vec![format!(
                "source /opt/ros/jazzy/setup.bash && source {exe} && ros2 topic list -t"
            )];
            profile.probe = format!("test -f {exe}");
        }
        "onnx" => {
            profile.program = "wsl".into();
            profile.args = vec![format!(
                "{exe} -c 'import onnxruntime; print(onnxruntime.get_available_providers())'"
            )];
            profile.probe = format!("test -x {exe}");
        }
        "torch" => {
            profile.program = "wsl".into();
            profile.args = vec![format!(
                "{exe} -c 'import torch, numpy, onnx; print(torch.__version__)'"
            )];
            profile.probe = format!("test -x {exe}");
        }
        "ardupilot" => {
            profile.program = "wsl".into();
            profile.args = vec![format!(
                "cd {} && test -z \"$(pgrep -x arducopter)\" && exec {exe} --model + --defaults Tools/autotest/default_params/copter.parm",
                quote(&candidate.path)
            )];
            profile.probe = format!("test -x {exe}");
        }
        "gazebo" => {
            profile.program = "wsl".into();
            profile.probe = format!("test -x {exe}");
        }
        "qgc" => {
            profile.program = candidate.executable.clone();
            profile.args = vec!["-swrast".into()];
            profile.probe = candidate.executable.clone();
            profile.cwd = String::new();
        }
        "jmavsim" => {
            profile.program = if candidate.runtime.is_empty() {
                "java".into()
            } else {
                candidate.runtime.clone()
            };
            profile.cwd = std::path::Path::new(&candidate.executable)
                .parent()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let jar = profile.args.iter().position(|a| a == "-jar");
            if let Some(i) = jar {
                if i + 1 < profile.args.len() {
                    profile.args[i + 1] = candidate.executable.clone();
                }
            } else {
                profile.args = backend::defaults()
                    .profiles
                    .into_iter()
                    .find(|p| p.id == "jmavsim")
                    .unwrap()
                    .args;
                let i = profile.args.iter().position(|a| a == "-jar").unwrap();
                profile.args[i + 1] = candidate.executable.clone();
            }
            profile.probe = candidate.executable.clone();
        }
        _ => {
            s.setup.message = "Use manual integration settings for this tool.".into();
            return;
        }
    }
    if profile.program == "wsl" {
        config.distro = candidate.environment.clone();
    }
    if !s.headless {
        if let Err(e) = backend::save(&config) {
            s.setup.message = e;
            return;
        }
    }
    s.config = config;
    s.setup
        .checks
        .insert(candidate.tool.clone(), candidate.clone());
    s.setup.last_applied = Some(candidate.tool);
    s.setup.message="Launch configuration saved. No tool was updated or started. Connection/readiness is checked when you use it.".into();
}
pub(crate) fn use_job_result(s: &mut State) {
    let Some(job) = s
        .setup
        .jobs
        .iter()
        .find(|j| Some(&j.id) == s.setup.selected_job.as_ref())
    else {
        return;
    };
    let job_id = job.id.clone();
    if let Some(candidate) = job.result.clone() {
        let tool = candidate.tool.clone();
        if let Some(index) = s.setup.recipes.iter().position(|r| r.id == candidate.tool) {
            s.setup.selected = index;
        }
        if !candidate.environment.is_empty() {
            s.setup.distro = TextEdit::new(&candidate.environment);
        }
        s.setup.candidate = Some(candidate);
        adopt(s);
        if !s.headless && s.setup.last_applied.as_deref() == Some(&tool) {
            if let Err(e) = std::fs::write(
                root().join(job_id).join("configuration-applied"),
                "Configuration applied explicitly or by reviewed setup option.\n",
            ) {
                s.setup.message =
                    format!("Configuration saved, but setup completion could not be recorded: {e}");
            }
        }
    }
}

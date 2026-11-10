use super::*;
pub(super) fn can_reconfigure(s: &mut State) -> bool {
    if s.busy() {
        s.setup.message="Stop the session (owned tools, vehicle link, recordings, pipeline jobs) before changing its environment or launch configuration.".into();
        false
    } else {
        true
    }
}
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
/// The command and probe of the `mavsdk` profile for the (quoted) interpreter `exe`.
pub(crate) fn mavsdk_command(exe: &str) -> (Vec<String>, String) {
    (
        vec![format!("{exe} -c 'import mavsdk; print(mavsdk.__file__)'")],
        format!("test -x {exe}"),
    )
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
    if candidate.tool != "qgc"
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
        "ros-gz" => "rosgz",
        "foxglove-bridge" => "foxglove",
        other => other,
    };
    let mut config = s.config.clone();
    if !config.profiles.iter().any(|p| p.id == id)
        && ["ros-workspace", "onnx", "mavsdk"].contains(&id)
    {
        config.profiles.push(backend::Profile {
            id: id.into(),
            name: match id {
                "onnx" => "ONNX CPU runtime",
                "mavsdk" => "MAVSDK Python",
                _ => "PX4 ROS 2 workspace",
            }
            .into(),
            category: if id == "onnx" {
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
        s.setup.message =
            "Add a matching tool profile in Tools & config (Add a tool) first.".into();
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
            (profile.args, profile.probe) = mavsdk_command(&exe);
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
        "ardupilot" => {
            profile.program = "wsl".into();
            // The adopted build is the Copter binary: a system naming another vehicle is
            // refused with a reason instead of silently flying a copter.
            profile.args = vec![format!(
                "{{ [ \"${{DRONE_DECK_AP_VEHICLE:-ArduCopter}}\" = ArduCopter ] || {{ echo \"This ArduPilot profile runs the adopted Copter build only, not $DRONE_DECK_AP_VEHICLE. Edit its command in Tools & config.\" >&2; exit 2; }}; }} && cd {} && test -z \"$(pgrep -x arducopter)\" && exec {exe} --model + --defaults Tools/autotest/default_params/copter.parm",
                quote(&candidate.path)
            )];
            profile.probe = format!("test -x {exe}");
        }
        "gazebo" => {
            profile.program = "wsl".into();
            profile.probe = format!("test -x {exe}");
        }
        "ros-gz" | "foxglove-bridge" => {
            profile.program = "wsl".into();
            profile.probe = format!("test -f {exe}");
        }
        "qgc" => {
            profile.program = candidate.executable.clone();
            profile.args =
                crate::qgc_launch::with_gpu_safe(&[], crate::qgc_launch::gpu_safe(profile));
            profile.probe = candidate.executable.clone();
            profile.cwd = String::new();
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
            if let Err(e) = crate::store::write(
                &root().join(job_id).join("configuration-applied"),
                b"Configuration applied explicitly or by reviewed setup option.\n",
            ) {
                s.setup.message =
                    format!("Configuration saved, but setup completion could not be recorded: {e}");
            }
        }
    }
}

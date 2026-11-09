use super::*;
use std::ffi::OsStr;

fn profile() -> Profile {
    crate::backend::defaults()
        .profiles
        .into_iter()
        .find(|p| p.id == "qgc")
        .unwrap()
}
fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|a| a.to_string()).collect()
}
fn setting(command: &Command, name: &str) -> Option<Option<String>> {
    command
        .get_envs()
        .find(|(key, _)| *key == OsStr::new(name))
        .map(|(_, value)| value.map(|s| s.to_string_lossy().into_owned()))
}
#[test]
fn gpu_safe_mode_uses_cpu_ui_without_mutating_parent_environment() {
    let mut profile = profile();
    profile.args = with_gpu_safe(&profile.args, true);
    let parent = std::env::var_os("QT_QUICK_BACKEND");
    let mut command = Command::new(&profile.program);
    command.env("QSG_RHI_BACKEND", "vulkan");
    assert!(configure(&mut command, &profile));
    assert_eq!(
        setting(&command, "QT_QUICK_BACKEND"),
        Some(Some("software".into()))
    );
    assert_eq!(
        setting(&command, "QT_OPENGL"),
        Some(Some("software".into()))
    );
    assert_eq!(setting(&command, "QSG_RHI_BACKEND"), Some(None));
    assert_eq!(std::env::var_os("QT_QUICK_BACKEND"), parent);
    #[cfg(windows)]
    assert_eq!(
        command.get_current_dir(),
        Some(Path::new("C:\\Program Files\\QGroundControl"))
    );
}
#[test]
fn explicit_working_directory_and_normal_graphics_choice_are_respected() {
    let mut profile = profile();
    profile.args.clear();
    profile.cwd = "custom-working-directory".into();
    let mut command = Command::new(&profile.program);
    command
        .current_dir(&profile.cwd)
        .env("QSG_RHI_BACKEND", "d3d11");
    assert!(!configure(&mut command, &profile));
    assert_eq!(command.get_current_dir(), Some(Path::new(&profile.cwd)));
    assert_eq!(setting(&command, "QT_QUICK_BACKEND"), None);
    assert_eq!(
        setting(&command, "QSG_RHI_BACKEND"),
        Some(Some("d3d11".into()))
    );
}
#[test]
fn unrelated_profiles_and_wsl_commands_are_not_changed() {
    let mut profile = profile();
    profile.id = "another-tool".into();
    let mut command = Command::new("cmd.exe");
    assert!(!configure(&mut command, &profile));
    assert!(command.get_envs().next().is_none());
    assert!(command.get_current_dir().is_none());
    profile.id = "qgc".into();
    profile.program = "wsl".into();
    assert!(!configure(&mut command, &profile));
    assert!(command.get_envs().next().is_none());
}
#[test]
fn gpu_safe_toggle_adds_one_flag_and_removes_every_spelling() {
    let on = with_gpu_safe(&args(&["--log", "x"]), true);
    assert_eq!(on, args(&["--log", "x", "-swrast"]));
    assert_eq!(
        with_gpu_safe(&on, true),
        on,
        "switching on twice adds no second flag"
    );
    assert_eq!(
        with_gpu_safe(&args(&["-swrast", "--log", "--swrast", "x"]), false),
        args(&["--log", "x"])
    );
    assert!(with_gpu_safe(&[], false).is_empty());
    let mut profile = profile();
    profile.args = args(&["--swrast"]);
    assert!(
        gpu_safe(&profile),
        "an existing --swrast profile reads as on"
    );
    profile.args = args(&["-swrast"]);
    assert!(
        gpu_safe(&profile),
        "an existing -swrast profile reads as on"
    );
    profile.args.clear();
    assert!(!gpu_safe(&profile));
}
#[test]
fn toggle_saves_the_profile_updates_an_open_form_and_refuses_while_running() {
    let mut s = crate::State::new();
    let i = s
        .config
        .profiles
        .iter()
        .position(|p| p.id == "qgc")
        .unwrap();
    s.config.profiles[i].args = args(&["-swrast", "--clear-settings"]);
    s.selected = i;
    crate::profile_editor::open(&mut s);
    s.qgc.hint = Some("failure".into());
    set_gpu_safe(&mut s, false);
    assert_eq!(s.config.profiles[i].args, args(&["--clear-settings"]));
    let form = s.form.as_ref().expect("the form stays open");
    assert_eq!(form.fields[2].text, "--clear-settings");
    assert!(s.qgc.hint.is_some(), "switching off keeps the failure hint");
    set_gpu_safe(&mut s, true);
    assert_eq!(
        s.config.profiles[i].args,
        args(&["--clear-settings", "-swrast"])
    );
    assert_eq!(
        s.form.as_ref().unwrap().fields[2].text,
        "--clear-settings\n-swrast"
    );
    assert!(s.qgc.hint.is_none(), "switching on clears the failure hint");
    s.status.insert("qgc".into(), "Running".into());
    set_gpu_safe(&mut s, false);
    assert!(gpu_safe(&s.config.profiles[i]), "refused while running");
    assert!(s.notice.contains("Stop QGroundControl"));
}
#[test]
fn graphics_failure_lines_are_recognised() {
    for line in [
        "Failed to create RHI (backend 2)",
        "qt.rhi.general: Failed to create D3D11 device: 0x887a0004",
        "Failed to initialize graphics backend for OpenGL.",
        "QOpenGLContext::create: failed to create context",
        "qt.scenegraph.general: Could not create Direct3D 11 swapchain",
        "QRhiGles2: Failed to create temporary context",
        "Unable to initialize Vulkan instance",
        "wglCreateContextAttribsARB() failed",
        "eglInitialize failed: EGL_NOT_INITIALIZED",
        "DXGI_ERROR_DEVICE_REMOVED",
    ] {
        assert!(graphics_failure(line), "{line}");
    }
    for line in [
        "GPU-safe launch: Qt Quick CPU renderer; graphics overrides apply only to QGroundControl.",
        "qt.rhi.general: Created D3D11 device",
        "Vehicle link error: connection refused",
        "Failed to load parameter file",
        "Error: pitch angle out of range",
        "legacy mode enabled",
    ] {
        assert!(!graphics_failure(line), "{line}");
    }
}
#[test]
fn early_non_zero_exit_is_a_likely_graphics_failure() {
    let t0 = Instant::now();
    let mut w = Watch::default();
    assert!(w.status("Running", t0, false).is_none());
    let notice = w
        .status("Exited -1073741819", t0 + Duration::from_secs(3), false)
        .expect("early crash");
    assert!(notice.contains("GPU-safe mode"), "{notice}");
    assert!(notice.contains("exit code -1073741819"), "{notice}");
    assert!(w.hint.is_some());
    // A new start clears the hint; a late, clean or user-stopped exit is not a failure.
    for (status, after) in [("Exited 1", 25), ("Exited 0", 2), ("Stopped", 2)] {
        assert!(w.status("Running", t0, false).is_none());
        assert!(w.hint.is_none());
        assert!(
            w.status(status, t0 + Duration::from_secs(after), false)
                .is_none(),
            "{status} after {after} s"
        );
    }
    // An exit without a recorded start (e.g. a second exit event) is ignored.
    assert!(w.status("Exited 1", t0, false).is_none());
}
#[test]
fn graphics_log_reports_once_per_run_and_respects_gpu_safe_mode() {
    let t0 = Instant::now();
    let mut w = Watch::default();
    w.status("Running", t0, false);
    assert!(w.line("Loading map tiles", false).is_none());
    let notice = w.line("Failed to create RHI (backend 2)", false).unwrap();
    assert!(notice.contains("turn on GPU-safe mode"), "{notice}");
    assert!(w.line("Failed to create RHI (backend 2)", false).is_none());
    assert!(
        w.status("Exited 1", t0 + Duration::from_secs(2), false)
            .is_none(),
        "already reported for this run"
    );
    w.status("Running", t0, true);
    let notice = w
        .status("Exited 3", t0 + Duration::from_secs(1), true)
        .unwrap();
    assert!(notice.contains("even in GPU-safe mode"), "{notice}");
    assert!(!notice.contains("turn on GPU-safe mode"), "{notice}");
}
#[test]
fn observe_sets_the_notice_only_for_qgc_events() {
    let mut s = crate::State::new();
    observe(&mut s, &Event::Status("qgc".into(), "Running".into()));
    observe(
        &mut s,
        &Event::Log("sih".into(), "Failed to create RHI".into()),
    );
    assert!(s.qgc.hint.is_none());
    observe(
        &mut s,
        &Event::Log("qgc".into(), "Failed to create RHI".into()),
    );
    assert!(s.qgc.hint.is_some());
    assert!(s.notice.contains("GPU-safe mode"));
}
#[test]
fn tools_row_shows_the_toggle_and_failure_hint_without_a_download_button() {
    use rune::{Env, Runtime};
    fn settle(rt: &mut Runtime<crate::State>) {
        for _ in 0..10 {
            rt.frame(0.1);
        }
    }
    fn tap(rt: &mut Runtime<crate::State>, key: &str) {
        let target = key.to_string();
        rt.update(move |_, tx| tx.scroll_into_view(&target));
        settle(rt);
        let (x, y) = rt
            .find(key)
            .unwrap_or_else(|| panic!("{key} is not on screen"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.2);
    }
    for size in [(1440., 900.), (390., 844.)] {
        let mut s = crate::State::new();
        s.navigate(crate::ui::Page::Tools);
        s.ui.tools_all = true;
        s.qgc.hint = Some("QGroundControl closed 2 s after starting (exit code 1).".into());
        let mut rt = Runtime::new(
            s,
            crate::view,
            size,
            Env {
                system_dark: true,
                scale: 1.,
            },
        );
        rt.frame(0.);
        assert!(rt.find("qgc-row-hint").is_some(), "row hint at {size:?}");
        tap(&mut rt, "configure-qgc");
        assert!(
            rt.find("qgc-graphics-hint").is_some(),
            "detail hint at {size:?}"
        );
        assert!(rt.find("download-package").is_none());
        let i = rt
            .state
            .config
            .profiles
            .iter()
            .position(|p| p.id == "qgc")
            .unwrap();
        assert!(!gpu_safe(&rt.state.config.profiles[i]), "off by default");
        tap(&mut rt, "qgc-gpu-safe");
        assert_eq!(rt.state.config.profiles[i].args, args(&["-swrast"]));
        assert!(rt.state.qgc.hint.is_none());
        settle(&mut rt);
        assert!(rt.find("qgc-row-hint").is_none());
        let (r, _) = rt.presented("qgc-gpu-safe").expect("toggle stays rendered");
        assert!(
            r[0] >= -1. && r[0] + r[2] <= size.0 + 1.,
            "toggle fits {size:?}: {r:?}"
        );
        tap(&mut rt, "qgc-gpu-safe");
        assert!(rt.state.config.profiles[i].args.is_empty());
    }
}

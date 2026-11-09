use super::*;
use std::ffi::OsStr;

fn profile() -> Profile {
    crate::backend::defaults()
        .profiles
        .into_iter()
        .find(|p| p.id == "qgc")
        .unwrap()
}
fn setting(command: &Command, name: &str) -> Option<Option<String>> {
    command
        .get_envs()
        .find(|(key, _)| *key == OsStr::new(name))
        .map(|(_, value)| value.map(|s| s.to_string_lossy().into_owned()))
}
#[test]
fn gpu_safe_mode_uses_cpu_ui_without_mutating_parent_environment() {
    let profile = profile();
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

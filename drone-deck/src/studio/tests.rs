use super::*;
use rune::{Env, Runtime};
#[test]
fn studio_and_credentials_are_reachable_in_headless_ui_without_network() {
    let mut rt = Runtime::new(
        State::new(),
        crate::view,
        (1216., 800.),
        Env {
            scale: 1.,
            system_dark: false,
        },
    );
    rt.update(|s, _| s.navigate(4));
    rt.frame(0.1);
    assert!(rt.find("model-repo").is_some());
    rt.update(|s, _| s.studio.tab = 1);
    rt.frame(0.1);
    assert!(rt.find("evaluation-csv").is_some());
    rt.update(|s, _| s.studio.tab = 2);
    rt.frame(0.1);
    assert!(rt.find("create-project").is_some());
    rt.update(|s, _| s.navigate(6));
    rt.frame(0.1);
    assert!(rt.find("hf-token").is_some());
    request(&mut rt.state, "start_model");
    assert!(!rt.state.studio.pending);
}
#[test]
fn windows_models_map_to_selected_wsl_mount_without_shell_interpolation() {
    assert_eq!(
        windows_to_linux("C:\\Users\\Example\\model cache"),
        "/mnt/c/Users/Example/model cache"
    );
    assert_eq!(windows_to_linux("C:"), "C:");
}
#[test]
fn new_workspace_panels_fit_small_and_wide_windows() {
    let mut rt = Runtime::new(
        State::new(),
        crate::view,
        (1216., 800.),
        Env {
            scale: 1.,
            system_dark: false,
        },
    );
    for (w, h) in [(360., 800.), (640., 360.), (1216., 800.)] {
        rt.set_viewport((w, h), 1.);
        for variant in 0..5 {
            rt.update(|s, _| {
                s.navigate(if variant == 4 { 2 } else { 4 });
                s.studio.tab = variant.min(2);
                s.hardware.mode = 1;
            });
            for _ in 0..10 {
                rt.frame(0.1);
            }
            for key in ["app-root", "main-scroll", "topbar", "statusbar"] {
                let (r, _) = rt.presented(key).unwrap();
                assert!(
                    r[0] >= -1. && r[0] + r[2] <= w + 1. && r[1] >= -1. && r[1] + r[3] <= h + 1.,
                    "{key} at {w}x{h}: {r:?}"
                );
            }
        }
    }
}

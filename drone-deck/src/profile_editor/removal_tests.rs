use super::*;
use rune::{Env, Runtime};

fn state() -> State {
    let mut s = State::new();
    super::super::create(&mut s, "Simulation");
    s.form.as_mut().unwrap().fields[0] = TextEdit::new("Custom visualizer");
    super::super::save(&mut s);
    super::super::open(&mut s);
    s
}

#[test]
fn removal_requires_confirmation_and_rechecks_running_and_staged_work() {
    let mut s = state();
    let count = s.config.profiles.len();
    let id = s.config.profiles[s.selected].id.clone();
    remove(&mut s);
    assert_eq!(s.config.profiles.len(), count);
    request(&mut s);
    for status in ["Starting", "Running"] {
        s.status.insert(id.clone(), status.into());
        remove_with(&mut s, |_| panic!("must not save a running tool"));
        assert_eq!(s.config.profiles.len(), count);
        assert!(s.notice.contains("Stop this tool"));
    }
    s.status.clear();
    s.session = Some(session::Launch::new(["custom-1"]));
    remove_with(&mut s, |_| panic!("must not save a staged tool"));
    assert_eq!(s.config.profiles.len(), count);
}

#[test]
fn failed_save_keeps_profile_and_success_cleans_only_its_runtime_entries() {
    let mut s = state();
    let id = s.config.profiles[s.selected].id.clone();
    let count = s.config.profiles.len();
    s.detected.insert(id.clone(), true);
    s.status.insert(id.clone(), "Exited 0".into());
    s.status.insert("qgc".into(), "Running".into());
    s.readiness.insert(id.clone(), "Stopped".into());
    s.systems.claim(&id);
    s.log("Keep this historical log".into());
    request(&mut s);
    remove_with(&mut s, |_| Err("disk unavailable".into()));
    assert_eq!(s.config.profiles.len(), count);
    assert!(s.detected.contains_key(&id));
    assert!(s.form.is_some());
    assert!(s.notice.contains("disk unavailable"));
    remove_with(&mut s, |config| {
        let text = serde_json::to_string(config).unwrap();
        let restored: Config = serde_json::from_str(&text).unwrap();
        assert!(!restored.profiles.iter().any(|p| p.id == id));
        Ok(())
    });
    assert_eq!(s.config.profiles.len(), count - 1);
    assert!(s.selected < s.config.profiles.len());
    assert!(!s.detected.contains_key(&id));
    assert!(!s.status.contains_key(&id));
    assert!(!s.readiness.contains_key(&id));
    assert!(s.systems.owner(&id).is_none());
    assert_eq!(s.status.get("qgc").map(String::as_str), Some("Running"));
    assert!(
        s.logs
            .iter()
            .any(|l| l.contains("Keep this historical log"))
    );
    assert!(s.form.is_none() && s.ui.expanded.is_none());
}

#[test]
fn built_in_profiles_cannot_be_removed() {
    let mut s = State::new();
    super::super::open(&mut s);
    let count = s.config.profiles.len();
    request(&mut s);
    s.form.as_mut().unwrap().confirm_remove = true;
    remove_with(&mut s, |_| panic!("must not save removal of a built-in"));
    assert_eq!(s.config.profiles.len(), count);
}

fn tap(rt: &mut Runtime<State>, key: &str) {
    let key_owned = key.to_owned();
    rt.update(move |_, tx| tx.scroll_into_view(&key_owned));
    for _ in 0..10 {
        rt.frame(0.1);
    }
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.2);
}

#[test]
fn removal_buttons_work_at_desktop_and_phone_widths() {
    for size in [(1366., 768.), (390., 844.)] {
        let mut rt = Runtime::new(
            state(),
            crate::view,
            size,
            Env {
                system_dark: true,
                scale: 1.,
            },
        );
        rt.frame(0.);
        tap(&mut rt, "remove-tool");
        let (actions, _) = rt.presented("profile-actions").unwrap();
        for key in ["save-form", "remove-tool", "cancel-form"] {
            let (rect, _) = rt.presented(key).unwrap();
            assert!(
                rect[0] >= actions[0] - 1.
                    && rect[1] >= actions[1] - 1.
                    && rect[0] + rect[2] <= actions[0] + actions[2] + 1.
                    && rect[1] + rect[3] <= actions[1] + actions[3] + 1.,
                "{key} must be in the same action group at {size:?}"
            );
        }
        assert!(rt.state.form.as_ref().unwrap().confirm_remove);
        tap(&mut rt, "cancel-remove-tool");
        assert!(!rt.state.form.as_ref().unwrap().confirm_remove);
        tap(&mut rt, "remove-tool");
        tap(&mut rt, "confirm-remove-tool");
        assert!(!rt.state.config.profiles.iter().any(|p| p.id == "custom-1"));
        assert!(rt.state.form.is_none());
    }
}

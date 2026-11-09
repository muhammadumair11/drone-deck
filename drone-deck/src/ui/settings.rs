use super::*;
use serde::{Deserialize, Serialize};

pub(super) const DEFAULT_ZOOM: u16 = 100;
#[derive(Serialize, Deserialize)]
struct Preferences {
    zoom_percent: u16,
}
fn preference_path() -> std::path::PathBuf {
    backend::root().join("ui-preferences.json")
}
pub(crate) fn restore(s: &mut State) {
    if s.headless {
        return;
    }
    if let Some(p) = crate::store::read_json::<Preferences>(&preference_path()) {
        s.ui.zoom_percent = p.zoom_percent.clamp(50, 150);
    }
}
pub(crate) fn set_zoom(s: &mut State, value: i32) {
    s.ui.zoom_percent = value.clamp(50, 150) as u16;
    s.ui.reset_scroll = true;
    s.notice = format!(
        "UI zoom: {}%. Ctrl+− / Ctrl++ to change, Ctrl+0 to reset.",
        s.ui.zoom_percent
    );
    if !s.headless {
        let result = crate::store::write_json(
            &preference_path(),
            &Preferences {
                zoom_percent: s.ui.zoom_percent,
            },
        );
        if let Err(e) = result {
            s.notice = format!("Zoom changed, but preferences could not be saved: {e}");
        }
    }
}
pub(crate) fn shortcuts(s: &mut State, event: &rune::KeyEvent, _: &mut Tx) -> bool {
    let primary = event.mods.cmd || event.mods.ctrl;
    let character = match &event.key {
        rune::Key::Command(c) => Some(*c),
        rune::Key::Text(t) if primary && t.len() == 1 => t.chars().next(),
        _ => None,
    };
    match character {
        Some('+') | Some('=') => {
            set_zoom(s, s.ui.zoom_percent as i32 + 5);
            true
        }
        Some('-') | Some('_') => {
            set_zoom(s, s.ui.zoom_percent as i32 - 5);
            true
        }
        Some('0') => {
            set_zoom(s, 100);
            true
        }
        Some(',') => {
            s.navigate(Page::Settings);
            true
        }
        _ => false,
    }
}
fn chip(key: &str, name: &str, description: &str, action: impl Fn(&mut State) + 'static) -> El {
    el().key(key)
        .role("button")
        .label(description)
        .focusable(true)
        .help(description)
        .cursor(rune::Cursor::Pointer)
        .min_h(28.)
        .min_w(28.)
        .px(8.)
        .center()
        .radius(14.)
        .border(1., theme::rule())
        .hover(theme::soft())
        .focus_ring(2., 2., theme::fg())
        .on_press(action)
        .child(theme::meta_sized(name, 10., theme::fg()))
}
/// The status-bar zoom control: − / percentage / +.
pub(super) fn zoom_controls(s: &State) -> El {
    row()
        .key("zoom-controls")
        .gap(4.)
        .center_y()
        .child(chip("zoom-out", "−", "Zoom out (Ctrl+−)", |s| {
            set_zoom(s, s.ui.zoom_percent as i32 - 5)
        }))
        .child(chip(
            "zoom-reset",
            &format!("{}%", s.ui.zoom_percent),
            "Reset zoom to 100% (Ctrl+0)",
            |s| set_zoom(s, 100),
        ))
        .child(chip("zoom-in", "+", "Zoom in (Ctrl++)", |s| {
            set_zoom(s, s.ui.zoom_percent as i32 + 5)
        }))
}

use super::*;
use serde::{Deserialize, Serialize};

pub(super) const DEFAULT_ZOOM: u16 = 85;
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
    if let Ok(bytes) = std::fs::read(preference_path()) {
        if let Ok(p) = serde_json::from_slice::<Preferences>(&bytes) {
            s.ui.zoom_percent = p.zoom_percent.clamp(50, 150);
        }
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
        let result = std::fs::create_dir_all(backend::root()).and_then(|_| {
            std::fs::write(
                preference_path(),
                serde_json::to_vec_pretty(&Preferences {
                    zoom_percent: s.ui.zoom_percent,
                })
                .unwrap(),
            )
        });
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
            s.navigate(6);
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
        .min_h(28.)
        .padding(6.)
        .radius(4.)
        .border(1., line())
        .fill(bg())
        .focus_ring(2., 1., accent())
        .on_press(action)
        .child(text(name).font(TextStyle::new(12., 600.)).color(ink()))
}
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
pub(super) fn page(s: &State) -> El {
    col().w_full().min_w(0.).gap(16.)
        .child(card().child(title("Settings",30.).wrap()).child(label("Application display and tool configuration.",14.,muted()).wrap()))
        .child(button("settings-setup","Environment & installation setup",true,|s|s.navigate(8)))
        .child(crate::studio::settings(s))
        .child(card().key("display-settings").child(heading("Display & zoom","SAVED AUTOMATICALLY"))
            .child(title(format!("{}%",s.ui.zoom_percent),32.))
            .child(label("Zoom changes the entire interface and reflows the layout to fit your window. The compact default is 85%; the supported range is 50–150%.",14.,muted()).wrap())
            .child(row().gap(8.).flex_wrap().child(button("settings-zoom-out","Zoom out",false,|s|set_zoom(s,s.ui.zoom_percent as i32-5)))
                .child(button("settings-zoom-in","Zoom in",false,|s|set_zoom(s,s.ui.zoom_percent as i32+5)))
                .child(button("settings-zoom-reset","Reset 100%",false,|s|set_zoom(s,100)))
                .child(button("settings-zoom-compact","Compact 85%",true,|s|set_zoom(s,85))))
            .child(label("Keyboard: Ctrl+− to zoom out · Ctrl++ (or Ctrl+=) to zoom in · Ctrl+0 to reset · Ctrl+, for Settings.",13.,muted()).wrap()))
        .child(card().child(heading("Workspace panels","LAYOUT"))
            .child(row().gap(8.).flex_wrap()
                .child(button("settings-inspector",if s.ui.inspector.unwrap_or(true){"Hide integration inspector"}else{"Show integration inspector"},false,|s|s.ui.inspector=Some(!s.ui.inspector.unwrap_or(true))))
                .child(button("settings-console",if s.ui.console.unwrap_or(true){"Hide process console"}else{"Show process console"},false,|s|s.ui.console=Some(!s.ui.console.unwrap_or(true))))))
        .child(card().child(heading("Tool settings","SIMULATORS / ROS / MAVSDK / AI"))
            .child(label("For installation paths, commands and Ubuntu configuration, open Integrations, select a tool's Details, then choose Configure manually.",14.,muted()).wrap())
            .child(button("settings-integrations","Open integration settings",true,|s|s.navigate(5))))
}

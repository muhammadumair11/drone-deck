//! The management shell from `Design/drone-workspace.html`: a compact rail, a top bar with
//! the active system, one scrolling page and a status bar. Guided setup has its own surface.
use super::*;
pub(crate) mod kit;
mod pages;
mod settings;
pub(crate) mod theme;
use kit::*;
pub(crate) use settings::{restore as restore_preferences, set_zoom, shortcuts as zoom_shortcuts};
use std::collections::HashSet;
use theme::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Page {
    System,
    Tools,
    Data,
    Activity,
    Settings,
    Setup,
}
/// Rail order: key slug, number, label.
pub(crate) const RAIL: [(Page, &str, &str, &str); 4] = [
    (Page::System, "system", "01", "System"),
    (Page::Tools, "tools", "02", "Tools & config"),
    (Page::Data, "data", "03", "Data & models"),
    (Page::Activity, "activity", "04", "Activity"),
];

pub(crate) struct UiState {
    /// Compact layouts: the rail opens as a drawer.
    pub menu: bool,
    /// The ACTIVE SYSTEM list is open.
    pub system_menu: bool,
    pub search: TextEdit,
    pub reset_scroll: bool,
    pub zoom_percent: u16,
    /// Open disclosures, by key.
    pub open: HashSet<String>,
    /// Tools & config: filter index and whether to list every registered tool.
    pub tools_filter: usize,
    pub tools_all: bool,
    /// Tools & config: the expanded tool row (profile id).
    pub expanded: Option<String>,
    /// Data & models tab: 0 recordings, 1 models, 2 datasets & evaluation, 3 projects.
    pub data_tab: usize,
}
impl Default for UiState {
    fn default() -> Self {
        Self {
            menu: false,
            system_menu: false,
            search: TextEdit::new(""),
            reset_scroll: false,
            zoom_percent: settings::DEFAULT_ZOOM,
            open: HashSet::new(),
            tools_filter: 0,
            tools_all: false,
            expanded: None,
            data_tab: 0,
        }
    }
}

fn owned_processes(s: &State) -> usize {
    s.status
        .values()
        .filter(|v| v.as_str() == "Running" || v.as_str() == "Starting")
        .count()
}

/// Owned processes plus recording or replay, which do not appear in `status`.
fn session_label(s: &State, processes: usize) -> String {
    let mut parts = Vec::new();
    if processes > 0 {
        parts.push(format!(
            "{processes} owned process{}",
            if processes == 1 { "" } else { "es" }
        ));
    }
    if s.recorder.recording {
        parts.push("recording".to_string());
    }
    if s.recorder.playing {
        parts.push("replay".to_string());
    }
    if parts.is_empty() {
        if s.session.is_some() {
            "Workspace starting".into()
        } else {
            "No active session".into()
        }
    } else {
        parts.join(" / ")
    }
}

fn rail(s: &State, width: f32, drawer: bool, height: f32) -> El {
    let mut nav = col().key("rail-nav").role("tablist").w_full().gap(6.);
    for (page, slug, num, name) in RAIL {
        let current = s.page == page;
        nav = nav.child(
            row()
                .key(format!("rail-{slug}"))
                .role("tab")
                .label(name)
                .focusable(true)
                .focus_selected(current)
                .cursor(rune::Cursor::Pointer)
                .w_full()
                .min_h(44.)
                .px(8.)
                .py(10.)
                .gap(14.)
                .center_y()
                .fill(if current { soft() } else { Color::TRANSPARENT })
                .hover(Color::TRANSPARENT)
                .focus_ring(2., 2., fg())
                .on_press_tx(move |s: &mut State, tx| {
                    s.navigate(page);
                    s.ui.menu = false;
                    tx.scroll_to_instant("main-scroll", 0., 0.);
                })
                .child(meta(num))
                .child(
                    caps(name, 11., 500., 0.06, if current { fg() } else { muted() })
                        .group_color(fg())
                        .grow(1.)
                        .wrap(),
                ),
        );
    }
    let processes = owned_processes(s);
    let active = crate::systems::active_work(s);
    let env = if s.systems.active == systems::PHYSICAL {
        "Windows native".to_string()
    } else if s.wsl_ready() {
        format!("{} / WSL 2", s.config.distro)
    } else if s.setup.host.is_none() {
        "Environment not checked".into()
    } else {
        "WSL environment not ready".into()
    };
    let mut foot = col()
        .key("railfoot")
        .w_full()
        .gap(4.)
        .pt(32.)
        .child(meta_sized("Windows desktop", 9., muted()))
        .child(meta_sized("Local first", 9., muted()))
        .child(el().h(10.))
        .child(meta_sized(env, 9., muted()).wrap())
        .child(meta_sized(
            session_label(s, processes),
            9.,
            if active { fg() } else { muted() },
        ));
    if active {
        foot = foot.child(ghost("stop-all", "Stop session", true, |s| {
            s.confirm = Some("stop".into())
        }));
    }
    foot = foot
        .child(ghost("open-settings", "Settings", true, |s| {
            s.ui.menu = false;
            s.navigate(Page::Settings)
        }))
        .child(meta_sized("Drone Deck v0.8", 9., muted()));
    let logo = row()
        .key("app-logo")
        .role("button")
        .label("Drone Deck")
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .min_h(44.)
        .center_y()
        .focus_ring(2., 2., fg())
        .on_press(|s: &mut State| {
            s.ui.menu = false;
            s.navigate(Page::System)
        })
        .child(caps("Drone Deck", 19., 700., 0.12, fg()).wrap());
    let mut top = col().w_full().gap(8.).child(logo);
    if drawer {
        top = top.child(ghost("close-menu", "Close navigation", true, |s| {
            s.ui.menu = false
        }));
    }
    let body = col()
        .w_full()
        .grow(1.)
        .min_w(0.)
        .gap(8.)
        .child(top)
        .child(el().h(24.))
        .child(meta("Manage"))
        .child(nav)
        .child(
            ghost("open-guided-setup", "Guided setup ↗", true, |s| {
                s.ui.menu = false;
                crate::guided::open(s)
            })
            .mt(20.),
        )
        .child(spacer())
        .child(foot);
    col()
        .key("sidebar")
        .w(width)
        .h_full()
        .min_h(0.)
        .fill(bg())
        .border_sides([0., 1., 0., 0.], soft())
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            col()
                .w_full()
                .min_h(height)
                .px(if width < 180. { 16. } else { 22. })
                .py(if drawer { 20. } else { 32. })
                .child(body),
        )
}

fn system_select(s: &State, compact: bool) -> El {
    let kind = s.systems.active;
    row()
        .key("system-select")
        .role("button")
        .label("Active system")
        .a11y_value(systems::KINDS[kind])
        .expanded(s.ui.system_menu)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .min_w(if compact { 0. } else { 200. })
        .grow(if compact { 1. } else { 0. })
        .min_h(44.)
        .px(12.)
        .py(10.)
        .gap(12.)
        .center_y()
        .justify_between()
        .fill(bg())
        .border(1., rule())
        .radius(4.)
        .hover(soft())
        .focus_ring(2., 0., fg())
        .on_press(|s: &mut State| s.ui.system_menu = !s.ui.system_menu)
        .child(caps(systems::KINDS[kind], 11., 500., 0.045, fg()).truncate())
        .child(meta_sized(
            if s.ui.system_menu { "▴" } else { "▾" },
            11.,
            muted(),
        ))
}

fn system_menu(s: &State) -> El {
    let mut list = col()
        .key("system-list")
        .role("listbox")
        .label("Active system")
        .anchored("system-select", rune::Side::Bottom, rune::Align::Start, 4.)
        .modal(true, |s: &mut State| s.ui.system_menu = false)
        .w(280.)
        .fill(bg())
        .border(1., rule())
        .radius(4.)
        .padding(4.)
        .gap(2.);
    for (i, kind) in systems::KINDS.iter().enumerate() {
        let current = i == s.systems.active;
        let saved = &s.systems.records[i];
        let detail = match (saved.name.trim(), saved.prepared) {
            ("", false) => "Preparation required".to_string(),
            ("", true) => "Prepared".to_string(),
            (n, true) => format!("{n} / prepared"),
            (n, false) => format!("{n} / preparation required"),
        };
        list = list.child(
            col()
                .key(format!("system-option-{i}"))
                .role("option")
                .label(*kind)
                .focusable(true)
                .focus_selected(current)
                .cursor(rune::Cursor::Pointer)
                .w_full()
                .px(12.)
                .py(10.)
                .gap(4.)
                .radius(2.)
                .fill(if current { soft() } else { Color::TRANSPARENT })
                .hover(soft())
                .focus_ring(2., 0., fg())
                .on_press(move |s: &mut State| {
                    s.select_system(i);
                })
                .child(caps(*kind, 11., 500., 0.045, fg()))
                .child(meta_sized(detail, 9., muted())),
        );
    }
    list
}

fn topbar(s: &State, vp: &Viewport) -> El {
    let bar = row()
        .key("topbar")
        .w_full()
        .min_w(0.)
        .px(vp.gutter())
        .fill(bg());
    if vp.compact() {
        // The drawer carries the logo, environment and settings; keep one row here.
        return bar
            .py(10.)
            .gap(12.)
            .center_y()
            .child(ghost("open-menu", "Menu", true, |s| s.ui.menu = true))
            .child(system_select(s, true));
    }
    let env = if s.systems.active == systems::PHYSICAL {
        "Windows native / no WSL required".to_string()
    } else {
        format!(
            "Windows + {} / WSL{}",
            s.config.distro,
            if s.wsl_ready() { " 2" } else { " / not ready" }
        )
    };
    let context = row()
        .key("system-context")
        .gap(18.)
        .center_y()
        .child(caps("Active system", 9., 500., 0.08, muted()))
        .child(system_select(s, false));
    bar.py(22.)
        .gaps(10., 24.)
        .center_y()
        .flex_wrap()
        .child(context)
        .child(spacer())
        .child(meta(env).key("environment").wrap())
}

fn statusbar(s: &State, vp: &Viewport) -> El {
    row()
        .key("statusbar")
        .w_full()
        .min_w(0.)
        .px(if vp.compact() { 12. } else { vp.gutter() })
        .py(6.)
        .gap(12.)
        .center_y()
        .fill(bg())
        .border_sides([1., 0., 0., 0.], soft())
        .child(
            meta_sized(s.notice.clone(), 10., fg())
                .key("feedback")
                .role("status")
                .grow(1.)
                .truncate()
                .help(s.notice.clone()),
        )
        .child(settings::zoom_controls(s))
}

fn foot(vp: &Viewport) -> El {
    let f = if vp.compact() {
        col().gap(8.)
    } else {
        row().gap(24.).justify_between()
    };
    f.key("management-footer")
        .w_full()
        .min_w(0.)
        .py(24.)
        .child(meta_sized("Stop session is not a vehicle command", 9., muted()).wrap())
        .child(meta_sized("Configuration ≠ connection ≠ data health", 9., muted()).wrap())
}

/// Confirms stopping owned processes (`"stop"`) or downloading a profile's package URL.
fn confirm_dialog(action: &str, w: f32, h: f32) -> [El; 2] {
    let stop = action == "stop";
    let (eyebrow, title, copy, verb) = if stop {
        (
            "Owned processes".to_string(),
            "Stop owned processes?",
            "This closes managed tools, simulators, recordings and replay started by Drone Deck. It does not land or disarm a vehicle, and it does not cancel installation jobs.".to_string(),
            "Stop session",
        )
    } else {
        (
            "Package download".to_string(),
            "Download this package?",
            format!("{action}

The file is saved to workspace/downloads with a safe file name. It is never opened or run; install it yourself after checking the publisher."),
            "Download",
        )
    };
    let dw = (w - 32.).min(560.);
    let dh = (h - 32.).min(280.);
    [
        el().key("confirm-scrim")
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .fill(Color::BLACK.alpha(0.7)),
        col()
            .key("confirmation")
            .role("dialog")
            .label("Confirm action")
            .absolute((w - dw) / 2., (h - dh) / 2.)
            .w(dw)
            .h(dh)
            .min_h(0.)
            .padding(24.)
            .gap(16.)
            .fill(bg())
            .border(1., rule())
            .radius(4.)
            .modal(true, |s: &mut State| s.confirm = None)
            .child(meta(eyebrow))
            .child(h2(title, 22.))
            .child(
                col()
                    .grow(1.)
                    .min_h(0.)
                    .w_full()
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(if stop { p(copy, 12.) } else { code(copy, 12., muted()).wrap_anywhere() }),
            )
            .child(
                row()
                    .gap(12.)
                    .flex_wrap()
                    .child(pill("confirm-action", verb, true, |s| match s.confirm.take() {
                        Some(a) if a == "stop" => s.stop_all(),
                        Some(url) => {
                            s.send(Request::Download(url.clone()));
                            s.notice = "Downloading package; completion appears in Activity → Process output.".into();
                            crate::activity::record(s, format!("Requested package download: {url}"));
                        }
                        None => {}
                    }))
                    .child(ghost("cancel-action", "Cancel", true, |s| s.confirm = None)),
            ),
    ]
}

pub(super) fn view(s: &State, cx: &mut Cx<State>) -> El {
    let [physical_w, physical_h] = cx.layout_size("window-frame").unwrap_or([1280., 800.]);
    let zoom = s.ui.zoom_percent as f32 / 100.;
    let vp = Viewport {
        w: physical_w / zoom,
        h: physical_h / zoom,
    };
    let (w, h) = (vp.w, vp.h);
    let mut root = row()
        .key("app-root")
        .w(w)
        .h(h)
        .min_w(0.)
        .min_h(0.)
        .fill(bg())
        .hover_motion_within(rune::tok::motion::CSS_ENTER, rune::tok::motion::CSS_EXIT);
    let stage = if s.page == Page::Setup {
        col()
            .key("app-stage")
            .grow(1.)
            .min_w(0.)
            .min_h(0.)
            .h_full()
            .child(crate::guided::header(s, &vp))
            .child(
                col()
                    .key("main-scroll")
                    .w_full()
                    .grow(1.)
                    .min_h(0.)
                    .min_w(0.)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(crate::guided::page(s, &vp)),
            )
            .child(statusbar(s, &vp))
    } else {
        if !vp.compact() {
            root = root.child(rail(s, vp.rail_width(), false, h));
        }
        col()
            .key("app-stage")
            .grow(1.)
            .min_w(0.)
            .min_h(0.)
            .h_full()
            .child(topbar(s, &vp))
            .child(
                col()
                    .key("main-scroll")
                    .w_full()
                    .grow(1.)
                    .min_h(0.)
                    .min_w(0.)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(
                        col()
                            .key("page-content")
                            .w_full()
                            .min_w(0.)
                            .px(vp.gutter())
                            .child(pages::page(s, &vp))
                            .child(foot(&vp)),
                    ),
            )
            .child(statusbar(s, &vp))
    };
    root = root.child(stage);
    if s.page != Page::Setup && vp.compact() && s.ui.menu {
        root = root
            .child(
                el().key("menu-scrim")
                    .absolute(0., 0.)
                    .w_full()
                    .h_full()
                    .fill(Color::BLACK.alpha(0.6))
                    .on_press(|s: &mut State| s.ui.menu = false),
            )
            .child(
                rail(s, (w * 0.86).min(300.), true, h)
                    .key("nav-drawer")
                    .absolute(0., 0.)
                    .h(h)
                    .modal(true, |s: &mut State| s.ui.menu = false),
            );
    }
    if s.page != Page::Setup && s.ui.system_menu {
        root = root.child(system_menu(s));
    }
    if let Some(action) = &s.confirm {
        for e in confirm_dialog(action, w, h) {
            root = root.child(e);
        }
    }
    el().key("window-frame").w_full().h_full().fill(bg()).child(
        root.absolute(0., 0.)
            .origin(0., 0.)
            .scale(zoom)
            .instant_changes(),
    )
}

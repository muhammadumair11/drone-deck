use super::*;
mod components;
mod pages;
mod settings;
pub(crate) use components::*;
pub(crate) use settings::{restore as restore_preferences, shortcuts as zoom_shortcuts};

pub(super) struct UiState {
    pub menu: bool,
    pub inspector: Option<bool>,
    pub console: Option<bool>,
    pub search: TextEdit,
    pub reset_scroll: bool,
    pub zoom_percent: u16,
}
impl Default for UiState {
    fn default() -> Self {
        Self {
            menu: false,
            inspector: None,
            console: None,
            search: TextEdit::new(""),
            reset_scroll: false,
            zoom_percent: settings::DEFAULT_ZOOM,
        }
    }
}
const PAGES: [(&str, &str); 9] = [
    ("01", "Workspace"),
    ("02", "Simulators"),
    ("03", "Flight operations"),
    ("04", "Development"),
    ("05", "AI & data"),
    ("06", "Integrations"),
    ("07", "Settings"),
    ("08", "Recordings"),
    ("09", "Setup"),
];
static LOGO: rune::Icon = rune::Icon {
    name: "drone-deck-logo",
    view: 256.,
    body: include_str!("../../assets/monogram.svg"),
};
fn sidebar(s: &State, width: f32, drawer: bool, short: bool) -> El {
    let brand = row()
        .gap(12.)
        .center_y()
        .child(
            el().size(40., 40.)
                .key("app-logo")
                .label("Drone Deck logo")
                .child(
                    el().size(37.5, 37.5)
                        .absolute(1.25, 1.25)
                        .radius(8.125)
                        .fill(c(17, 17, 17)),
                )
                .child(
                    rune::icon(&LOGO, 40.)
                        .absolute(0., 0.)
                        .color(c(255, 255, 255)),
                ),
        )
        .child(
            col()
                .grow(1.)
                .min_w(0.)
                .gap(4.)
                .child(title("Drone Deck", 20.))
                .child(label("AUTONOMY WORKSPACE", 9., muted()).wrap()),
        );
    let mut nav = col()
        .key("sidebar")
        .w(width)
        .h_full()
        .min_h(0.)
        .padding(16.)
        .gap(24.)
        .fill(surface())
        .border(1., line())
        .child(brand);
    if drawer {
        nav = nav.child(button("close-menu", "Close navigation", false, |s| {
            s.ui.menu = false
        }));
    }
    let mut items = col().w_full().gap(4.);
    for (i, (num, name)) in PAGES.iter().enumerate() {
        items = items.child(
            row()
                .key(format!("nav-{i}"))
                .role("button")
                .label(*name)
                .focusable(true)
                .w_full()
                .min_h(48.)
                .padding(12.)
                .gap(12.)
                .radius(8.)
                .fill(if i == s.page { ink() } else { surface() })
                .focus_ring(2., 1., accent())
                .on_press_tx(move |s: &mut State, tx| {
                    s.navigate(i);
                    s.ui.menu = false;
                    tx.scroll_to_instant("main-scroll", 0., 0.);
                })
                .child(label(*num, 11., if i == s.page { bg() } else { muted() }))
                .child(
                    text(*name)
                        .font(TextStyle::new(14., 600.))
                        .color(if i == s.page { bg() } else { muted() })
                        .wrap(),
                ),
        );
    }
    let nav = nav.child(
        col()
            .grow(1.)
            .min_h(0.)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(items),
    );
    if short {
        return nav;
    }
    nav.child(
        card()
            .padding(14.)
            .gap(9.)
            .child(label("LOCAL ENVIRONMENT", 10., muted()))
            .child(title("Windows + WSL", 14.))
            .child(
                label(
                    &s.config.distro,
                    13.,
                    if s.wsl_ready() {
                        c(19, 122, 44)
                    } else {
                        muted()
                    },
                )
                .wrap(),
            )
            .child(
                label(
                    if s.wsl_ready() {
                        "Configured WSL 2 environment"
                    } else {
                        "Environment setup / check required"
                    },
                    12.,
                    muted(),
                )
                .wrap(),
            ),
    )
    .child(label("RUNE NATIVE / v0.7", 10., muted()))
}
fn stats(s: &State, width: f32) -> El {
    let count = s
        .status
        .values()
        .filter(|v| v.as_str() == "Running")
        .count();
    let count = format!("{count:02}");
    let data = [
        ("PROCESSES", count.as_str(), "Managed by this application"),
        (
            "VEHICLE LINK",
            s.metrics
                .get("link")
                .map(String::as_str)
                .unwrap_or("Offline"),
            "MAVSDK · read only",
        ),
        (
            "ALTITUDE",
            s.metrics.get("altitude").map(String::as_str).unwrap_or("—"),
            "Relative to takeoff",
        ),
        (
            "FLIGHT MODE",
            s.metrics.get("mode").map(String::as_str).unwrap_or("—"),
            "Reported by PX4",
        ),
    ];
    let columns = if width < 420. {
        1
    } else if width < 900. {
        2
    } else {
        4
    };
    let mut stack = col().key("stats-grid").w_full().min_w(0.).gap(12.);
    for group in data.chunks(columns) {
        let mut band = row().w_full().min_w(0.).gap(12.);
        for (k, v, sub) in group {
            band = band.child(metric(k, v, sub));
        }
        stack = stack.child(band);
    }
    stack
}
fn inspector(s: &State) -> El {
    let p = &s.config.profiles[s.selected.min(s.config.profiles.len() - 1)];
    card().key("inspector").child(row().gap(8.).justify_between().center_y().child(label("INTEGRATION INSPECTOR",11.,accent()).wrap()).child(button("close-inspector","Hide",false,|s|s.ui.inspector=Some(false))))
        .child(title(&p.name,26.).wrap()).child(label(&p.description,14.,muted()).wrap())
        .child(el().h(1.).w_full().fill(line())).child(label("COMMAND",10.,muted()))
        .child(col().w_full().min_w(0.).padding(12.).radius(4.).fill(code_bg()).child(mono(format!("{} {}",p.program,p.args.join(" ")),13.,c(246,245,244)).wrap_anywhere()))
        .child(label("WORKING DIRECTORY",10.,muted())).child(label(if !p.cwd.is_empty(){&p.cwd}else if p.program=="wsl"{"Selected WSL environment"}else if p.id=="qgc"{"QGroundControl installation directory"}else{"Application working directory"},13.,ink()).wrap_anywhere())
        .child(row().gap(8.).flex_wrap().child(button("copy-command","Copy command",false,|_|{}).on_press_tx(|s:&mut State,tx|{let p=&s.config.profiles[s.selected];tx.copy(format!("{} {}",p.program,p.args.join(" ")));s.notice="Command copied.".into();}))
            .child(button("open-project","Open folder",false,|s|{let p=&s.config.profiles[s.selected];s.open(if p.cwd.is_empty(){backend::root().to_string_lossy().into_owned()}else{p.cwd.clone()});})))
        .child(label("SETUP OPTIONS",10.,muted()))
        .child(button("detect-inspector","1   Detect installation",false,|s|s.send(Request::Detect(s.config.clone()))).w_full())
        .child(button("docs","2   Guided setup / docs",false,|s|s.open(s.config.profiles[s.selected].docs.clone())).w_full())
        .child(button("manual-config","3   Configure manually",false,profile_editor::open).w_full())
        .child(button("download","Open setup / install options",false,|s|{
            let id=s.config.profiles[s.selected].id.clone();
            let tool=match id.as_str(){"sih"=>"px4","telemetry"=>"mavsdk",other=>other};
            if let Some(index)=s.setup.recipes.iter().position(|r|r.id==tool){crate::setup::choose(s,index);s.navigate(8);}
            else{s.notice="Custom profile: configure its command and use the publisher's setup instructions. Package downloads alone do not install a tool.".into();}
        }).w_full())
}
fn console(s: &State, narrow: bool) -> El {
    let mut lines = col().w_full().min_w(0.).gap(6.);
    let selected = &s.config.profiles[s.selected].id;
    let mut count = 0;
    for line in s
        .logs
        .iter()
        .filter(|l| !s.log_filter || l.starts_with(&format!("[{selected}]")))
        .rev()
        .take(100)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        lines = lines.child(
            mono(
                line,
                12.,
                if line.contains("ERROR") || line.contains("Failed") {
                    c(255, 170, 160)
                } else {
                    c(246, 245, 244)
                },
            )
            .wrap_anywhere(),
        );
        count += 1;
    }
    if count == 0 {
        lines = lines.child(
            label(
                "No matching process output yet.\nStart a session or run a configured tool.",
                13.,
                c(190, 186, 181),
            )
            .wrap(),
        );
    }
    let actions = row()
        .gap(8.)
        .flex_wrap()
        .child(button(
            "filter-logs",
            if s.log_filter {
                "Selected tool"
            } else {
                "All tools"
            },
            false,
            |s| s.log_filter = !s.log_filter,
        ))
        .child(button("export-logs", "Export", false, |s| {
            if !s.headless {
                let path = backend::root().join("session.log");
                match std::fs::write(&path, s.logs.iter().cloned().collect::<Vec<_>>().join("\n")) {
                    Ok(_) => s.notice = format!("Exported {}", path.display()),
                    Err(e) => s.notice = e.to_string(),
                }
            }
        }))
        .child(button("clear-logs", "Clear", false, |s| s.logs.clear()));
    let name = row()
        .gap(10.)
        .center_y()
        .flex_wrap()
        .child(title("Process console", 20.))
        .child(badge("Live output", true));
    let top = if narrow {
        col().gap(12.).child(name).child(actions)
    } else {
        row()
            .justify_between()
            .gap(16.)
            .center_y()
            .flex_wrap()
            .child(name)
            .child(actions)
    };
    card().key("log-panel").child(top).child(
        col()
            .key("log-scroll")
            .w_full()
            .min_w(0.)
            .h(190.)
            .padding(16.)
            .radius(8.)
            .fill(code_bg())
            .scroll(ScrollSpec { stick_bottom: true })
            .child(lines),
    )
}
pub(super) fn view(s: &State, cx: &mut Cx<State>) -> El {
    let [physical_w, physical_h] = cx.layout_size("window-frame").unwrap_or([1280., 800.]);
    let zoom = s.ui.zoom_percent as f32 / 100.;
    let (w, h) = (physical_w / zoom, physical_h / zoom);
    let mobile = w < 900.;
    let sidebar_width = if w > 1200. { 256. } else { 220. };
    let padding = if w < 420. {
        12.
    } else {
        (w * 0.022).clamp(16., 36.)
    };
    let content_width = (w - if mobile { 0. } else { sidebar_width } - padding * 2.).max(280.);
    let wide = w > 1200.;
    let show_inspector = ![6, 7, 8].contains(&s.page) && s.ui.inspector.unwrap_or(true);
    let show_console = s.ui.console.unwrap_or(true);
    let mut actions = row()
        .gap(8.)
        .flex_wrap()
        .child(button("detect-tools", "Detect tools", false, |s| {
            s.send(Request::Detect(s.config.clone()));
            s.notice = "Scanning Windows and Ubuntu tools…".into();
        }))
        .child(button("stop-all", "Stop session", false, |s| {
            s.confirm = Some("stop".into())
        }))
        .child(button("open-settings", "Settings", false, |s| {
            s.navigate(6)
        }));
    if ![6, 7, 8].contains(&s.page) {
        actions = actions
            .child(button(
                "toggle-inspector",
                if show_inspector {
                    "Hide details"
                } else {
                    "Show details"
                },
                false,
                move |s| s.ui.inspector = Some(!show_inspector),
            ))
            .child(button(
                "toggle-console",
                if show_console {
                    "Hide console"
                } else {
                    "Show console"
                },
                false,
                move |s| s.ui.console = Some(!show_console),
            ));
    }
    let mut names = row().min_w(0.).grow(1.).gap(12.).center_y();
    if mobile {
        names = names.child(button("open-menu", "Menu", false, |s| s.ui.menu = true));
    }
    names = names.child(
        col()
            .min_w(0.)
            .gap(6.)
            .child(label("DRONE PROJECTS / LOCAL LAB", 10., accent()).wrap())
            .child(title(PAGES[s.page].1, if mobile { 26. } else { 34. }).wrap()),
    );
    let header = if w < 1000. {
        col().gap(12.).child(names).child(actions)
    } else {
        row()
            .gap(16.)
            .center_y()
            .justify_between()
            .flex_wrap()
            .child(names)
            .child(actions)
    };
    let page_width = if wide && show_inspector {
        (content_width - 316.).max(280.)
    } else {
        content_width
    };
    let page = pages::page(s, page_width);
    let page_with_inspector = if wide && show_inspector {
        row()
            .w_full()
            .min_w(0.)
            .gap(16.)
            .items_start()
            .child(col().grow(1.).min_w(0.).child(page))
            .child(col().w(300.).child(inspector(s)))
    } else {
        let mut stack = col().w_full().min_w(0.).gap(16.).child(page);
        if show_inspector {
            stack = stack.child(inspector(s));
        }
        stack
    };
    let mut content = col()
        .key("page-content")
        .w_full()
        .min_w(0.)
        .padding(padding)
        .gap(16.);
    if ![6, 7, 8].contains(&s.page)
        && !(s.page == 2 && s.hardware.mode != 0)
        && !s.editing
        && s.form.is_none()
    {
        content = content.child(stats(s, content_width));
    }
    if [1, 3, 4, 5].contains(&s.page) && !s.editing && s.form.is_none() {
        content = content.child(
            field(&s.ui.search, "tool-search")
                .on_edit(|s: &mut State| &mut s.ui.search)
                .placeholder("Filter tools by name or description…", muted()),
        );
    }
    content = content.child(page_with_inspector);
    if show_console && ![6, 8].contains(&s.page) {
        content = content.child(console(s, content_width < 650.));
    }
    let stage = col()
        .key("app-stage")
        .grow(1.)
        .min_w(0.)
        .min_h(0.)
        .h_full()
        .child(
            col()
                .key("topbar")
                .max_h((h * 0.4).max(60.))
                .min_h(0.)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .w_full()
                .min_w(0.)
                .padding(if mobile { 12. } else { 20. })
                .fill(bg())
                .border(1., line())
                .child(header),
        )
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
                .child(content),
        )
        .child(
            col()
                .key("statusbar")
                .w_full()
                .min_w(0.)
                .padding(6.)
                .fill(surface())
                .border(1., line())
                .child(
                    row()
                        .w_full()
                        .min_w(0.)
                        .gap(8.)
                        .center_y()
                        .child(
                            label(&s.notice, 11., muted())
                                .grow(1.)
                                .truncate()
                                .help(s.notice.clone()),
                        )
                        .child(settings::zoom_controls(s)),
                ),
        );
    let mut root = row()
        .key("app-root")
        .w(w)
        .h(h)
        .min_w(0.)
        .min_h(0.)
        .fill(bg());
    if !mobile {
        root = root.child(sidebar(s, sidebar_width, false, h < 700.));
    }
    root = root.child(stage);
    if mobile && s.ui.menu {
        root = root
            .child(
                el().key("menu-scrim")
                    .absolute(0., 0.)
                    .w_full()
                    .h_full()
                    .fill(ink().alpha(0.35))
                    .on_press(|s: &mut State| s.ui.menu = false),
            )
            .child(
                sidebar(s, (w * 0.86).min(320.), true, h < 700.)
                    .key("nav-drawer")
                    .absolute(0., 0.)
                    .modal(true, |s: &mut State| s.ui.menu = false),
            );
    }
    if let Some(action) = &s.confirm {
        let stop = action == "stop";
        let dw = (w - 32.).min(560.);
        let dh = (h - 32.).min(260.);
        root=root.child(el().key("confirm-scrim").absolute(0.,0.).w_full().h_full().fill(ink().alpha(0.35)))
            .child(card().key("confirmation").role("dialog").label("Confirm action").absolute((w-dw)/2.,(h-dh)/2.).w(dw).h(dh).min_h(0.).modal(true,|s:&mut State|s.confirm=None)
            .child(title(if stop{"Stop owned processes?"}else{"Download this package?"},22.).wrap())
            .child(col().grow(1.).min_h(0.).w_full().scroll(ScrollSpec{stick_bottom:false}).child(label(if stop{"This closes managed tools and simulators. It does not send a landing command."}else{action},13.,muted()).wrap_anywhere()))
            .child(row().gap(8.).flex_wrap().child(button("confirm-action","Confirm",true,|s|{if let Some(a)=s.confirm.take(){if a=="stop"{s.stop_all();}else{s.send(Request::Download(a));s.notice="Downloading package; completion appears in the console.".into();}}})).child(button("cancel-action","Cancel",false,|s|s.confirm=None))));
    }
    el().key("window-frame").w_full().h_full().fill(bg()).child(
        root.absolute(0., 0.)
            .origin(0., 0.)
            .scale(zoom)
            .instant_changes(),
    )
}

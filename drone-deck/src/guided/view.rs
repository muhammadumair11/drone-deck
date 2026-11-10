//! Guided setup surface: its own top bar, the chapter journey, a shared chapter frame and
//! the first two chapters (Intention, The system).
use super::*;
use crate::ui::kit::*;
use crate::ui::theme::*;
use rune::Color;

/// Gutter and breakpoints of `Design/system-setup.html` (920px and 600px).
pub(super) struct Frame {
    pub w: f32,
}
impl Frame {
    pub fn narrow(&self) -> bool {
        self.w <= 600.
    }
    pub fn medium(&self) -> bool {
        self.w <= 920.
    }
    pub fn gutter(&self) -> f32 {
        if self.narrow() {
            22.
        } else if self.medium() {
            24.
        } else {
            40.
        }
    }
}

/// Guided setup's own top bar (key `topbar`): back to management, system name, start over.
pub(crate) fn header(s: &State, vp: &ui::theme::Viewport) -> El {
    let f = Frame { w: vp.w };
    let back = row()
        .key("back-to-management")
        .role("button")
        .label("Drone Deck / Workspace")
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .min_h(44.)
        .center_y()
        .hover_group()
        .focus_ring(2., 4., fg())
        .on_press(|s: &mut State| {
            let to = match s.guided.came_from {
                ui::Page::Setup => ui::Page::System,
                page => page,
            };
            s.navigate(to)
        })
        .child(
            caps(
                "Drone Deck / Workspace",
                if f.narrow() { 13. } else { 19. },
                700.,
                if f.narrow() { 0.1 } else { 0.16 },
                fg(),
            )
            .group_color(fg_hover())
            .wrap(),
        );
    let label = s.guided.name.text.trim();
    let mut bar = row()
        .key("topbar")
        .w_full()
        .min_w(0.)
        .px(if f.medium() { 24. } else { 38. })
        .py(if f.narrow() {
            14.
        } else if f.medium() {
            22.
        } else {
            28.
        })
        .gaps(6., 24.)
        .center_y()
        .justify_between()
        .flex_wrap()
        .fill(bg())
        .child(back);
    if !f.narrow() {
        bar = bar.child(
            meta(if label.is_empty() {
                "System preparation"
            } else {
                label
            })
            .key("project-label")
            .truncate()
            .max_w(320.),
        );
    }
    bar.child(ghost("new-system", "Start over ↗", true, restart))
}

fn journey(s: &State, f: &Frame) -> El {
    let mut list = row()
        .key("chapter-navigation")
        .role("tablist")
        .label("System chapters")
        .w_full()
        .max_w(720.)
        .min_w(0.)
        .px(if f.narrow() { 22. } else { 24. })
        .mt(if f.narrow() { 12. } else { 24. });
    for (i, name) in CHAPTERS.iter().enumerate() {
        let current = i == s.guided.chapter;
        let enabled = i <= s.guided.chapter;
        let number = format!("0{} /", i + 1);
        let color = if current { fg() } else { muted() };
        let size = if f.narrow() { 9. } else { 10. };
        let words = if f.narrow() {
            col()
                .gap(6.)
                .child(caps(number, size, 500., 0.08, color).group_color(fg()))
                .child(
                    caps(*name, size, 500., 0.08, color)
                        .group_color(fg())
                        .wrap(),
                )
        } else {
            col().child(
                caps(format!("{number} {name}"), size, 500., 0.08, color)
                    .group_color(fg())
                    .wrap(),
            )
        };
        let mut b = row()
            .key(format!("chapter-nav-{i}"))
            .role("tab")
            .label(format!("{} {name}", i + 1))
            .focus_selected(current)
            .grow(1.)
            .w(0.)
            .min_w(0.)
            .min_h(44.)
            .px(if f.narrow() { 3. } else { 8. })
            .py(12.)
            .border_sides([1., 0., 0., 0.], if current { fg() } else { soft() })
            .child(words);
        b = if enabled {
            b.focusable(true)
                .cursor(rune::Cursor::Pointer)
                .hover(soft())
                .focus_ring(2., 2., fg())
                .on_press(move |s: &mut State| go(s, i))
        } else {
            b.disabled(true).dim(0.6)
        };
        list = list.child(b);
    }
    row().w_full().min_w(0.).justify_center().child(list)
}

/// A chapter: eyebrow, intro (H1 + lead), body and the action bar (`chapter-action`).
#[allow(clippy::too_many_arguments)]
pub(super) fn frame(
    s: &State,
    f: &Frame,
    eyebrow: &str,
    title: &str,
    lead: &str,
    content: El,
    note: &str,
    action: El,
) -> El {
    let h1_size = if f.narrow() {
        36.
    } else {
        (f.w * 0.04).clamp(36., 56.)
    };
    let heading = h1(title, h1_size).key("chapter-title").max_w(h1_size * 14.);
    let lead = p(lead, 14.).max_w(620.);
    let intro = if f.medium() {
        col().w_full().min_w(0.).gap(24.).child(heading).child(lead)
    } else {
        row()
            .w_full()
            .min_w(0.)
            .gap(72.)
            .items_end()
            .child(col().grow(1.4).w(0.).min_w(0.).child(heading))
            .child(col().grow(1.).w(0.).min_w(0.).child(lead))
    };
    let note = meta(note).wrap();
    let bar = if f.narrow() {
        col()
            .gap(16.)
            .items_stretch()
            .child(note)
            .child(action.w_full())
    } else {
        row()
            .gap(24.)
            .center_y()
            .justify_between()
            .child(note.grow(1.).min_w(0.))
            .child(action)
    };
    let top = if f.w >= 1600. {
        90.
    } else if f.narrow() {
        30.
    } else if f.medium() {
        40.
    } else {
        32.
    };
    col()
        .key(format!("chapter-{}", s.guided.chapter))
        .w_full()
        .min_w(0.)
        .pt(top)
        .pb(24.)
        .child(meta(eyebrow).mb(18.).wrap())
        .child(intro.mb(if f.w >= 1600. { 64. } else { 32. }))
        .child(content)
        .child(bar.key("chapter-action").w_full().min_w(0.).mt(32.))
}

/// The main chapter action (key `next`): a large pill.
pub(super) fn next(name: &str, enabled: bool, on: impl Fn(&mut State) + 'static) -> El {
    pill_sized("next", name, enabled, true, on)
}

fn intent_row(s: &State, i: usize, f: &Frame) -> El {
    let story = &STORIES[i];
    let selected = s.guided.choice == i;
    let locked = s.setup.active();
    let circle = el()
        .size(18., 18.)
        .center()
        .radius(9.)
        .border(1., if selected { fg() } else { rule() })
        .a11y_hidden()
        .child(
            el().size(8., 8.)
                .radius(4.)
                .fill(if selected { fg() } else { Color::TRANSPARENT }),
        );
    let name = caps(
        story.name,
        if f.medium() { 19. } else { 22. },
        700.,
        0.02,
        fg(),
    )
    .wrap();
    let desc = col()
        .w_full()
        .min_w(0.)
        .gap(4.)
        .child(p(story.desc, 11.))
        .child(meta(story.tag).wrap());
    let number = meta(format!("0{}", i + 1));
    let mut r = row()
        .key(format!("intent-{i}"))
        .role("radio")
        .label(story.name)
        .checked(selected)
        .w_full()
        .min_w(0.)
        .min_h(if f.w >= 1600. { 120. } else { 88. })
        .py(if f.narrow() { 20. } else { 18. })
        .px(if f.narrow() { 8. } else { 16. })
        .center_y()
        .border_sides([1., 0., if i == 3 { 1. } else { 0. }, 0.], rule())
        .fill(if selected { soft() } else { Color::TRANSPARENT });
    r = if f.narrow() {
        r.gap(14.)
            .child(number.w(24.))
            .child(col().grow(1.).min_w(0.).gap(8.).child(name).child(desc))
            .child(circle)
    } else {
        let gap = if f.medium() { 16. } else { 24. };
        r.gap(gap)
            .child(number.w(if f.medium() { 36. } else { 52. }))
            .child(col().grow(1.).w(0.).min_w(0.).child(name))
            .child(
                col()
                    .grow(if f.medium() { 1. } else { 1.1 })
                    .w(0.)
                    .min_w(0.)
                    .child(desc),
            )
            .child(circle)
    };
    if locked && !selected {
        return r.disabled(true).dim(0.45);
    }
    r.focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(fg().alpha(0.06))
        .focus_ring(2., 2., fg())
        .on_press(move |s: &mut State| choose(s, i))
}

fn choose(s: &mut State, i: usize) {
    if !s.setup.active() {
        set_choice(s, i);
    }
}

/// Why the system choice is locked, naming what the running installation belongs to.
fn locked_note(s: &State) -> String {
    let after = "Other systems can be chosen after it finishes or is cancelled.";
    if s.setup.batch.is_none() {
        return format!("An installation job is running. {after}");
    }
    match batch_system(s) {
        Some(kind) => format!(
            "An installation queue is running for {}. {after}",
            STORIES[kind].name
        ),
        None => format!(
            "An installation queue is running for {}. {after}",
            setup::workflow::PRESETS[s.setup.workflow.min(setup::workflow::PRESETS.len() - 1)].0
        ),
    }
}

/// Which saved system this preparation creates or updates.
fn scope_note(s: &State) -> String {
    let base = s.guided.base.as_deref().and_then(|id| s.systems.get(id));
    match (base, target(s)) {
        (Some(b), Some(t)) if b.id == t.id => format!(
            "Preparing {}. Choosing another outcome prepares a new system; {} stays as it is.",
            t.label(),
            t.label()
        ),
        (_, Some(t)) => format!("Preparing {}.", t.label()),
        (Some(b), None) => format!(
            "Creates a new system from this template when you name it. {} stays as it is.",
            b.label()
        ),
        (None, None) => "Creates a new system from this template when you name it.".into(),
    }
}

fn intention(s: &State, f: &Frame) -> El {
    let mut list = col()
        .key("intent-list")
        .role("radiogroup")
        .label("System starting point")
        .w_full()
        .min_w(0.);
    for i in 0..4 {
        list = list.child(intent_row(s, i, f));
    }
    let mut content = col()
        .w_full()
        .min_w(0.)
        .gap(12.)
        .child(list)
        .child(notice(scope_note(s)).key("intent-scope"));
    if s.setup.active() {
        content = content.child(notice(locked_note(s)));
    }
    frame(
        s,
        f,
        "01 / Start with an outcome",
        "What would you like to try?",
        "You bring the idea. Drone Deck brings the right tools together, helps prepare your computer, and guides you to a first result.",
        content,
        "No tool knowledge required. Nothing installs yet.",
        next("Shape this system →", true, |s| go(s, 1)),
    )
}

fn connector(vertical: bool, width: f32) -> El {
    if vertical {
        let mut line = col().w(1.).h(28.).gap(3.).ml(10.);
        for _ in 0..5 {
            line = line.child(el().w(1.).h(3.).fill(rule()));
        }
        return line.a11y_hidden();
    }
    let mut line = row().grow(1.).h(1.).gap(3.);
    for _ in 0..((width - 12.) / 6.) as usize {
        line = line.child(el().w(3.).h(1.).fill(rule()));
    }
    row()
        .w(width)
        .gap(4.)
        .center_y()
        .self_center()
        .a11y_hidden()
        .child(line)
        .child(meta_sized("›", 16., muted()))
}

fn blueprint(s: &State, f: &Frame) -> El {
    let story = &STORIES[s.guided.choice];
    let vertical = f.narrow();
    let mut chain = if vertical {
        col().w_full().min_w(0.)
    } else {
        row()
            .w_full()
            .min_w(0.)
            .gap(if f.medium() { 16. } else { 20. })
    };
    chain = chain
        .key("system-story")
        .mt(if vertical { 24. } else { 48. })
        .mb(30.);
    for (i, (name, role, desc, target)) in story.steps.iter().enumerate() {
        if i > 0 {
            chain = chain.child(connector(vertical, if f.medium() { 20. } else { 50. }));
        }
        let mut stage = col()
            .key(format!("system-stage-{i}"))
            .min_w(0.)
            .py(if vertical { 10. } else { 20. })
            .gap(8.)
            .child(meta(format!("0{} / {role}", i + 1)).wrap())
            .child(h3(*name, 23.).my(4.))
            .child(p(*desc, 12.))
            .child(meta(*target).wrap());
        stage = if vertical {
            stage.w_full()
        } else {
            stage.grow(1.).w(0.)
        };
        chain = chain.child(stage);
    }
    chain
}

fn valid_path(text: &str) -> bool {
    let b = text.as_bytes();
    text.starts_with('/')
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\')
}

fn recipe_controls(s: &State, f: &Frame) -> El {
    let g = &s.guided;
    let target: El = if g.choice == systems::PHYSICAL {
        body("Windows local / no WSL required", 12., fg())
    } else {
        let installed: Vec<_> = s
            .setup
            .host
            .as_ref()
            .map(|h| h.distros.iter().filter(|d| d.version == 2).collect())
            .unwrap_or_default();
        if installed.is_empty() {
            p(
                if s.setup.host.is_none() {
                    format!(
                        "{} / WSL 2. Not checked yet: Preparation checks this computer and offers to install it.",
                        s.config.distro
                    )
                } else {
                    "No WSL 2 distribution is installed. Preparation offers a reviewed installation."
                        .to_string()
                },
                12.,
            )
        } else {
            let mut list = col().w_full().min_w(0.);
            for d in installed {
                let name = d.name.clone();
                list = list.child(crate::setup::option_row(
                    format!("environment-{}", d.name),
                    &d.name,
                    &format!(
                        "WSL 2 / {}",
                        if d.name == s.config.distro {
                            "in use"
                        } else {
                            "installed"
                        }
                    ),
                    s.setup.distro.text.trim().eq_ignore_ascii_case(&d.name),
                    !s.setup.active(),
                    move |s| crate::setup::select_distro(s, &name),
                ));
            }
            list
        }
    };
    let tool = s
        .setup
        .recipes
        .iter()
        .find(|r| r.id == primary_tool(s))
        .map(|r| r.name.as_str())
        .unwrap_or("tool");
    let text = g.path.text.trim();
    let (message, tone) = if text.is_empty() {
        (
            "An existing path needs compatibility checks before adoption.",
            muted(),
        )
    } else if valid_path(text) {
        (
            "Path format accepted; compatibility has not been checked.",
            muted(),
        )
    } else {
        (
            "Use an absolute Windows or Linux path.",
            if g.path_error { danger() } else { muted() },
        )
    };
    let path = col()
        .w_full()
        .min_w(0.)
        .gap(10.)
        .child(labeled(
            &format!("Existing {tool} location (optional)"),
            field(&g.path, "override", "Absolute executable or source path")
                .on_edit(|s: &mut State| &mut s.guided.path),
        ))
        .child(body(message, 11., tone).key("pathmessage").role("status"));
    let target = labeled("Execution target", target);
    let grid = if f.medium() {
        col().w_full().min_w(0.).gap(24.).child(target).child(path)
    } else {
        row()
            .w_full()
            .min_w(0.)
            .gap(32.)
            .items_start()
            .child(col().grow(1.).w(0.).min_w(0.).child(target))
            .child(col().grow(1.).w(0.).min_w(0.).child(path))
    };
    col()
        .w_full()
        .min_w(0.)
        .gap(16.)
        .child(p(
            "This is a proposed relationship, not a discovered live network. The selected environment and every connection need separate verification.",
            12.,
        ).max_w(760.))
        .child(grid)
}

fn advance_system(s: &mut State) {
    let name = match systems::clean_name(&s.guided.name.text) {
        Ok(name) => name,
        Err(e) => {
            s.guided.name_error = e;
            return;
        }
    };
    s.guided.name_error.clear();
    let path = s.guided.path.text.trim().to_string();
    if !path.is_empty() && !valid_path(&path) {
        s.guided.path_error = true;
        s.ui.open.insert("technical-recipe".into());
        return;
    }
    s.guided.path_error = false;
    let tool = primary_tool(s).to_string();
    if path.is_empty() {
        s.setup.overrides.remove(&tool);
    } else {
        s.setup.overrides.insert(tool, path);
    }
    let kind = s.guided.choice;
    let id = ensure_target(s);
    if s.systems.get(&id).is_some_and(|x| x.name != name) {
        let _ = s.systems.rename(&id, &name);
        if let Err(e) = s.systems.save(s.headless) {
            s.notice = format!("The system name could not be saved: {e}");
        }
    }
    if kind != systems::PHYSICAL
        && !s
            .setup
            .distro
            .text
            .trim()
            .eq_ignore_ascii_case(&s.config.distro)
        && s.setup.can_use_distro()
    {
        crate::setup::use_distro(s);
        if !s
            .setup
            .distro
            .text
            .trim()
            .eq_ignore_ascii_case(&s.config.distro)
        {
            s.notice = s.setup.message.clone();
        }
    }
    go(s, 2);
}

fn the_system(s: &State, f: &Frame) -> El {
    let story = &STORIES[s.guided.choice];
    let name = col()
        .w_full()
        .max_w(440.)
        .min_w(0.)
        .gap(10.)
        .mt(18.)
        .child(labeled(
            "Give your system a name",
            field(&s.guided.name, "name", "My first system")
                .on_edit(|s: &mut State| &mut s.guided.name),
        ))
        .child(
            body(s.guided.name_error.clone(), 11., danger())
                .key("nameerror")
                .role("status"),
        )
        .child(notice(scope_note(s)).key("system-scope"));
    let content = col()
        .w_full()
        .min_w(0.)
        .child(blueprint(s, f))
        .child(notice(story.extra).max_w(860.))
        .child(
            disclosure(
                s,
                "technical-recipe",
                "Inspect or adapt the technical recipe",
                || recipe_controls(s, f),
            )
            .mt(16.),
        )
        .child(name);
    frame(
        s,
        f,
        "02 / A small, complete system",
        story.title,
        story.result,
        content,
        "A plan you can understand before a tool is installed.",
        next("Prepare my computer →", true, advance_system),
    )
}

fn footer(s: &State, f: &Frame) -> El {
    let r = if f.narrow() {
        col().gap(8.)
    } else {
        row().gap(20.).justify_between()
    };
    r.key("footer")
        .w_full()
        .min_w(0.)
        .py(32.)
        .child(meta_sized("Windows desktop / local first", 9., muted()))
        .child(
            meta_sized(
                if s.headless {
                    "Headless preview / no processes start and nothing installs"
                } else {
                    "Changes need your review / missions and vehicle setup stay in QGroundControl"
                },
                9.,
                muted(),
            )
            .wrap(),
        )
}

pub(crate) fn page(s: &State, vp: &ui::theme::Viewport) -> El {
    let f = Frame { w: vp.w };
    let chapter = match s.guided.chapter {
        0 => intention(s, &f),
        1 => the_system(s, &f),
        2 => prep::view(s, &f),
        _ => run::view(s, &f),
    };
    let container = col()
        .key("guided-content")
        .w_full()
        .max_w(1140.)
        .min_w(0.)
        .px(f.gutter())
        .child(chapter)
        .child(footer(s, &f));
    col()
        .key("guided-page")
        .w_full()
        .min_w(0.)
        .items_center()
        .child(journey(s, &f))
        .child(container)
}

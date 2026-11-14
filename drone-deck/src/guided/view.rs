//! Guided setup surface: its own top bar, the chapter journey, a shared chapter frame, the
//! starting-point rows of the Vehicle chapter and the page that holds a chapter.
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

pub(super) fn intent_row(s: &State, i: usize, f: &Frame) -> El {
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
pub(super) fn locked_note(s: &State) -> String {
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
            s.setup.workflow.name
        ),
    }
}

/// Which saved system this preparation creates or updates.
pub(super) fn scope_note(s: &State) -> String {
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
        0 => steps::vehicle(s, &f),
        1 => steps::world(s, &f),
        2 => steps::listeners(s, &f),
        REVIEW => prep::view(s, &f),
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

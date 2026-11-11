//! Components from `Design/*.html`, built on the tokens in `theme`.
use super::theme::*;
use crate::State;
use rune::{Color, Cursor, El, TextEdit, TextStyle, col, el, row, text, text_input};

fn hover_ring(radius: f32) -> El {
    el().pin(Some(0.), Some(0.), Some(0.), Some(0.))
        .radius(radius)
        .border(1., fg())
        .group_opacity(0., 1.)
        .a11y_hidden()
}

/// `.btn`: pill, translucent fill, 1px rule; the rule and text brighten on hover.
pub(crate) fn pill(
    key: impl Into<String>,
    name: &str,
    enabled: bool,
    on: impl Fn(&mut State) + 'static,
) -> El {
    pill_sized(key, name, enabled, false, on)
}
/// The guided-setup `.btn` is larger (48px, 11px label).
pub(crate) fn pill_sized(
    key: impl Into<String>,
    name: &str,
    enabled: bool,
    large: bool,
    on: impl Fn(&mut State) + 'static,
) -> El {
    let (h, px, size) = if large {
        (48., 25., 11.)
    } else {
        (44., 22., 10.)
    };
    let b = row()
        .key(key)
        .role("button")
        .label(name)
        .min_h(h)
        .px(px)
        .py(10.)
        .gap(12.)
        .center()
        .radius(32.)
        .fill(soft())
        .border(1., rule())
        .child(caps(name, size, 500., 0.08, fg()).group_color(fg_hover()));
    if !enabled {
        return b.disabled(true).dim(0.4);
    }
    b.focusable(true)
        .cursor(Cursor::Pointer)
        .hover(fg().alpha(0.06))
        .press(fg().alpha(0.1))
        .press_scale(0.98)
        .focus_ring(2., 4., fg())
        .on_press(on)
        .child(hover_ring(32.))
}
/// `.btn-ghost`: text only; underlines on hover.
/// A **Browse…** button that opens Windows' Open dialog and hands the chosen file to `set`.
/// `filters` are (name, extensions) pairs; "All files" is always added.
pub(crate) fn browse(
    key: impl Into<String>,
    title: &'static str,
    filters: &'static [(&'static str, &'static [&'static str])],
    enabled: bool,
    set: fn(&mut State, std::path::PathBuf),
) -> El {
    ghost(key, "Browse…", enabled, |_| {}).on_press_tx(move |_: &mut State, tx| {
        let mut options = rune::file_dialog::FileDialogOptions {
            title: title.into(),
            ..Default::default()
        };
        for (name, extensions) in filters {
            options = options.filter(*name, extensions);
        }
        options = options.filter("All files", &["*"]);
        tx.open_file(options, move |s: &mut State, answer, _| match answer {
            Ok(Some(path)) => set(s, path),
            Ok(None) => {}
            Err(e) => {
                s.notice = format!("Could not open the file dialog: {e}. Type the path instead.")
            }
        });
    })
}
pub(crate) fn ghost(
    key: impl Into<String>,
    name: &str,
    enabled: bool,
    on: impl Fn(&mut State) + 'static,
) -> El {
    let label = col()
        .gap(2.)
        .child(caps(name, 10., 500., 0.08, fg()).group_color(fg_hover()))
        .child(
            el().h(1.)
                .w_full()
                .fill(fg_hover())
                .group_opacity(0., 1.)
                .a11y_hidden(),
        );
    let b = row()
        .key(key)
        .role("button")
        .label(name)
        .min_h(44.)
        .px(4.)
        .center_y()
        .child(label);
    if !enabled {
        return b.disabled(true).dim(0.4);
    }
    b.focusable(true)
        .cursor(Cursor::Pointer)
        .hover_group()
        .focus_ring(2., 4., fg())
        .on_press(on)
}
/// `.smalllink`: a small underlined text action.
pub(crate) fn small_link(
    key: impl Into<String>,
    name: &str,
    on: impl Fn(&mut State) + 'static,
) -> El {
    col()
        .key(key)
        .role("button")
        .label(name)
        .focusable(true)
        .cursor(Cursor::Pointer)
        .py(8.)
        .gap(1.)
        .hover(soft())
        .focus_ring(2., 4., fg())
        .on_press(on)
        .child(caps(name, 10., 400., 0.08, fg()))
        .child(el().h(1.).w_full().fill(fg()).a11y_hidden())
}

/// `.tabs`: underlined tab buttons. `on` receives the tab index.
pub(crate) fn tabs<F>(prefix: &str, items: &[&str], selected: usize, on: F) -> El
where
    F: Fn(&mut State, usize) + Clone + 'static,
{
    let mut strip = row()
        .key(format!("{prefix}-tabs"))
        .role("tablist")
        .gaps(4., 28.)
        .flex_wrap()
        .my(18.);
    for (i, name) in items.iter().enumerate() {
        let on = on.clone();
        let current = i == selected;
        strip = strip.child(
            row()
                .key(format!("{prefix}-{i}"))
                .role("tab")
                .label(*name)
                .focusable(true)
                .focus_selected(current)
                .cursor(Cursor::Pointer)
                .min_h(44.)
                .py(10.)
                .center_y()
                .border_sides(
                    [0., 0., 1., 0.],
                    if current { fg() } else { Color::TRANSPARENT },
                )
                .hover(soft())
                .focus_ring(2., 4., fg())
                .on_press(move |s: &mut State| on(s, i))
                .child(
                    caps(*name, 10., 500., 0.08, if current { fg() } else { muted() })
                        .group_color(fg()),
                ),
        );
    }
    strip
}

/// `.input`: black field, 1px rule, 4px radius, 2px outline on focus.
/// Attach `.on_edit(|s: &mut State| &mut s.…)` at the call site.
pub(crate) fn field(edit: &TextEdit, key: &str, placeholder: &str) -> El {
    text_input(edit, TextStyle::new(12., 400.).family(SANS))
        .key(key)
        .single_line()
        .placeholder(placeholder, fg().alpha(0.4))
        .caret(fg(), fg().alpha(0.25))
        .w_full()
        .min_w(0.)
        .min_h(44.)
        .padding(12.)
        .radius(4.)
        .fill(bg())
        .border(1., rule())
        .color(fg())
        .focus_within_ring(2., 0., fg())
}
/// A multi-line field (arguments, JSON) that grows from `min` to `max` lines, then scrolls
/// inside its rule instead of spilling over the content below.
pub(crate) fn area(
    edit: &TextEdit,
    key: &str,
    lines: (usize, usize),
    on_edit: Option<impl for<'a> Fn(&'a mut State) -> &'a mut TextEdit + 'static>,
) -> El {
    let mut input = text_input(edit, TextStyle::new(12., 400.).family(MONO))
        .key(key)
        .w_full()
        .min_w(0.)
        .padding(12.)
        .color(fg())
        .caret(fg(), fg().alpha(0.25));
    input = match on_edit {
        Some(f) => input.on_edit(f),
        None => input.read_only(true).dim(0.5),
    };
    rune::auto_grow(input, lines.0, lines.1)
        .key(format!("{key}-scroll"))
        .radius(4.)
        .fill(bg())
        .border(1., rule())
        .focus_within_ring(2., 0., fg())
}
/// `.field`: a 10px muted label over its control.
pub(crate) fn labeled(name: &str, control: El) -> El {
    col()
        .w_full()
        .min_w(0.)
        .gap(8.)
        .child(caps(name, 10., 400., 0.08, muted()))
        .child(control)
}

/// A checkbox with a label; the box fills with ink when checked.
pub(crate) fn checkbox(
    key: &str,
    name: &str,
    checked: bool,
    on: impl Fn(&mut State) + 'static,
) -> El {
    row()
        .key(key)
        .role("checkbox")
        .label(name)
        .checked(checked)
        .focusable(true)
        .cursor(Cursor::Pointer)
        .min_h(44.)
        .gap(12.)
        .center_y()
        .focus_ring(2., 4., fg())
        .on_press(on)
        .child(
            el().size(16., 16.)
                .center()
                .radius(2.)
                .border(1., if checked { fg() } else { rule() })
                .fill(if checked { fg() } else { Color::TRANSPARENT })
                .a11y_hidden()
                .child(
                    text("✓")
                        .font(TextStyle::new(12., 700.))
                        .color(bg())
                        .opacity(if checked { 1. } else { 0. }),
                ),
        )
        .child(body(name, 11., fg()).grow(1.).a11y_hidden())
}

/// `details`/`summary`: a `+`/`-` disclosure. Open state lives in `s.ui.open`.
pub(crate) fn disclosure(s: &State, key: &str, title: &str, content: impl FnOnce() -> El) -> El {
    let open = s.ui.open.contains(key);
    let id = key.to_string();
    let summary = row()
        .key(format!("{key}-summary"))
        .role("button")
        .label(title)
        .expanded(open)
        .focusable(true)
        .cursor(Cursor::Pointer)
        .min_h(44.)
        .gap(12.)
        .center_y()
        .hover(soft())
        .focus_ring(2., 4., fg())
        .on_press(move |s: &mut State| {
            if !s.ui.open.remove(&id) {
                s.ui.open.insert(id.clone());
            }
        })
        .child(
            text(if open { "-" } else { "+" })
                .font(TextStyle::new(18., 300.))
                .color(fg())
                .w(12.)
                .a11y_hidden(),
        )
        .child(caps(title, 11., 500., 0.06, fg()).grow(1.).wrap());
    let mut block = col().key(key).w_full().min_w(0.).my(8.).child(summary);
    if open {
        block = block.child(col().w_full().min_w(0.).gap(12.).pb(8.).child(content()));
    }
    block
}

/// A status dot paired with text elsewhere, never colour alone.
pub(crate) fn dot(color: Color) -> El {
    el().size(7., 7.).radius(3.5).fill(color).a11y_hidden()
}

/// `.notice`: 11px muted copy, line-height 1.8.
pub(crate) fn notice(t: impl Into<String>) -> El {
    p(t, 11.).w_full()
}

/// A row with a 1px rule on top (`.toolrow` uses the soft rule, `.log-row` the stronger one).
pub(crate) fn ruled(strong: bool) -> El {
    row()
        .w_full()
        .min_w(0.)
        .border_sides([1., 0., 0., 0.], if strong { rule() } else { soft() })
}

/// `.log-row`: meta | title + description | pull. Stacks below 720px.
pub(crate) fn log_row(key: impl Into<String>, lead: El, main: El, pull: El, compact: bool) -> El {
    if compact {
        return ruled(true)
            .key(key)
            .gap(14.)
            .py(20.)
            .items_start()
            .child(lead.w(56.))
            .child(col().grow(1.).min_w(0.).gap(8.).child(main).child(pull));
    }
    ruled(true)
        .key(key)
        .gap(24.)
        .py(20.)
        .items_start()
        .child(lead.w(100.))
        .child(col().grow(1.).min_w(0.).child(main))
        .child(col().w(120.).items_end().child(pull))
}
/// Title + description block for a log row.
pub(crate) fn row_text(title: impl Into<String>, desc: impl Into<String>) -> El {
    col()
        .w_full()
        .min_w(0.)
        .gap(4.)
        .child(h3(title, 15.))
        .child(p(desc, 11.))
}

/// `.empty`: a heading and an explanation, no decoration.
pub(crate) fn empty(key: &str, title: &str, t: &str) -> El {
    col()
        .key(key)
        .w_full()
        .min_w(0.)
        .py(32.)
        .gap(10.)
        .child(h3(title, 16.))
        .child(p(t, 12.).max_w(760.))
}

/// `.mast`: eyebrow, H1, description and an optional action aligned to the end.
pub(crate) fn mast(
    eyebrow: &str,
    title: &str,
    desc: &str,
    action: Option<El>,
    compact: bool,
) -> El {
    let words = col()
        .grow(1.)
        .min_w(0.)
        .gap(10.)
        .child(meta(eyebrow).wrap())
        .child(h1(title, if compact { 32. } else { 40. }).key("page-title"))
        .child(p(desc, 11.).max_w(760.));
    let mut m = if compact {
        col().gap(16.)
    } else {
        row().gap(24.).items_end()
    };
    m = m
        .key("mast")
        .w_full()
        .min_w(0.)
        .mt(16.)
        .mb(22.)
        .child(words);
    if let Some(a) = action {
        m = m.child(a);
    }
    m
}

/// `.helpstrip`: a heading or note beside one action.
pub(crate) fn helpstrip(key: &str, title: Option<&str>, t: &str, action: El, compact: bool) -> El {
    let mut words = col().grow(1.).min_w(0.).gap(6.);
    if let Some(title) = title {
        words = words.child(h2(title, 20.));
    }
    words = words.child(p(t, 11.).max_w(820.));
    let strip = if compact {
        col().gap(12.).items_start()
    } else {
        row().gap(24.).center_y()
    };
    strip
        .key(key)
        .w_full()
        .min_w(0.)
        .pt(10.)
        .pb(18.)
        .child(words)
        .child(action)
}

/// `.maphead` / `.runtimeheader`: a section title with metadata or an action on the right.
pub(crate) fn section_head(title: &str, right: El, compact: bool) -> El {
    let head = if compact {
        col().gap(8.).items_start()
    } else {
        row().gap(24.).center_y().justify_between()
    };
    // Every section opens with a full-width rule and space above it, so where one section
    // ends and the next begins is visible at a glance (row rules inside use `soft`).
    head.w_full()
        .min_w(0.)
        .mt(20.)
        .pt(20.)
        .mb(12.)
        .border_sides([1., 0., 0., 0.], section_rule())
        .child(h2(title, 19.))
        .child(right)
}
/// A sub-section heading inside a section (`h3` with a lighter rule than `section_head`).
pub(crate) fn subsection_head(title: &str, right: El) -> El {
    row()
        .w_full()
        .min_w(0.)
        .gaps(6., 24.)
        .flex_wrap()
        .justify_between()
        .mt(12.)
        .pt(16.)
        .pb(8.)
        .border_sides([1., 0., 0., 0.], divider())
        .child(h3(title, 15.))
        .child(right)
}

/// `.asset`: two columns that stack below 720px.
pub(crate) fn asset(left: El, right: El, compact: bool) -> El {
    if compact {
        return col()
            .w_full()
            .min_w(0.)
            .gap(20.)
            .py(24.)
            .child(left)
            .child(right);
    }
    // Side-by-side halves are separate things: a vertical rule between them.
    row()
        .w_full()
        .min_w(0.)
        .py(24.)
        .items_stretch()
        .child(col().grow(1.).min_w(0.).gap(12.).pr(20.).child(left))
        .child(
            col()
                .grow(1.)
                .min_w(0.)
                .gap(12.)
                .pl(20.)
                .border_sides([0., 0., 0., 1.], divider())
                .child(right),
        )
}

/// `.station`: one stage of a topology chain.
pub(crate) fn station(
    key: impl Into<String>,
    target: &str,
    name: &str,
    desc: &str,
    state: &str,
    state_color: Color,
    large: bool,
) -> El {
    col()
        .key(key)
        .grow(1.)
        .min_w(0.)
        .py(if large { 20. } else { 14. })
        .gap(8.)
        .child(meta(target).wrap())
        .child({
            // A long single word ("QGroundControl") cannot wrap at a space: draw it smaller,
            // and break it anywhere as a last resort rather than clipping it at the page edge.
            let size = if large { 24. } else { 20. };
            let size = if name.chars().count() > 12 {
                size * 0.8
            } else {
                size
            };
            h3(name, size).wrap_anywhere()
        })
        .child(p(desc, 11.).max_w(420.))
        .child(
            row()
                .w_full()
                .min_w(0.)
                .gap(8.)
                .center_y()
                .child(dot(state_color))
                .child(meta_sized(state, 10., fg()).wrap().grow(1.)),
        )
}

/// `.wire`: a dashed connector with an arrow and a small label. A solid wire means traffic
/// was observed on that link.
pub(crate) fn wire(label: &str, observed: bool, vertical: bool) -> El {
    let color = if observed { fg() } else { rule() };
    if vertical {
        let mut line = col().w(1.).h(24.).gap(if observed { 0. } else { 3. });
        for _ in 0..4 {
            line = line.child(el().w(1.).h(if observed { 9. } else { 3. }).fill(color));
        }
        return row()
            .ml(12.)
            .gap(14.)
            .center_y()
            .a11y_hidden()
            .child(line)
            .child(meta_sized(format!("v  {label}"), 8., muted()));
    }
    let mut line = row().w(60.).h(1.).gap(if observed { 0. } else { 3. });
    if observed {
        line = line.child(el().w(60.).h(1.).fill(color));
    } else {
        for _ in 0..10 {
            line = line.child(el().w(3.).h(1.).fill(color));
        }
    }
    col()
        .w(60.)
        .gap(6.)
        .center()
        .self_center()
        .a11y_hidden()
        .child(meta_sized(">", 12., color).self_end())
        .child(line)
        .child(meta_sized(label, 8., muted()).align(rune::Align::Center))
}

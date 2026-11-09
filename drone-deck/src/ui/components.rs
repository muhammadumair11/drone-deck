use super::super::*;
pub(crate) fn c(r: u8, g: u8, b: u8) -> Color {
    fn l(v: u8) -> f32 {
        let x = v as f32 / 255.;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    }
    Color {
        r: l(r),
        g: l(g),
        b: l(b),
        a: 1.,
    }
}
pub(crate) fn bg() -> Color {
    c(255, 255, 255)
}
pub(crate) fn panel() -> Color {
    bg()
}
pub(crate) fn surface() -> Color {
    c(246, 245, 244)
}
pub(crate) fn line() -> Color {
    c(228, 227, 225)
}
pub(crate) fn ink() -> Color {
    c(13, 13, 13)
}
pub(crate) fn muted() -> Color {
    c(97, 93, 89)
}
pub(crate) fn accent() -> Color {
    c(0, 117, 222)
}
pub(crate) fn code_bg() -> Color {
    c(36, 35, 33)
}
pub(crate) fn label(t: impl Into<String>, size: f32, color: Color) -> El {
    text(t)
        .font(TextStyle::new(
            if size <= 11. { size + 1. } else { size },
            400.,
        ))
        .color(color)
        .min_w(0.)
}
pub(crate) fn title(t: impl Into<String>, size: f32) -> El {
    text(t)
        .font(TextStyle::new(size, 650.))
        .color(ink())
        .min_w(0.)
}
pub(crate) fn mono(t: impl Into<String>, size: f32, color: Color) -> El {
    let mut style = TextStyle::new(size, 400.);
    style.family = rune::Family::Mono;
    text(t).font(style).color(color).min_w(0.)
}
pub(crate) fn button(
    key: impl Into<String>,
    name: &str,
    primary: bool,
    action: impl Fn(&mut State) + 'static,
) -> El {
    let danger = name == "Stop session" || name == "Stop";
    el().key(key)
        .role("button")
        .label(name)
        .focusable(true)
        .min_h(44.)
        .padding(10.)
        .radius(4.)
        .fill(if primary {
            accent()
        } else if danger {
            c(255, 240, 238)
        } else {
            c(236, 234, 232)
        })
        .border(1., if primary { accent() } else { line() })
        .hover(if primary {
            c(0, 91, 171)
        } else {
            c(225, 223, 220)
        })
        .focus_ring(2., 2., accent())
        .on_press(action)
        .child(
            text(name)
                .font(TextStyle::new(14., 650.))
                .color(if primary {
                    bg()
                } else if danger {
                    c(180, 35, 24)
                } else {
                    ink()
                })
                .wrap(),
        )
}
pub(crate) fn badge(t: &str, on: bool) -> El {
    el().padding(5.)
        .radius(12.)
        .fill(if on {
            c(232, 250, 240)
        } else {
            c(236, 234, 232)
        })
        .child(label(t, 11., if on { c(19, 122, 44) } else { muted() }).wrap())
}
pub(crate) fn card() -> El {
    col()
        .w_full()
        .min_w(0.)
        .padding(20.)
        .gap(14.)
        .radius(12.)
        .fill(panel())
        .border(1., line())
}
pub(crate) fn heading(t: &str, sub: &str) -> El {
    row()
        .w_full()
        .gap(10.)
        .flex_wrap()
        .center_y()
        .justify_between()
        .child(title(t, 20.).wrap())
        .child(label(sub, 10., muted()).wrap())
}
pub(crate) fn metric(k: &str, v: &str, sub: &str) -> El {
    card()
        .grow(1.)
        .min_w(0.)
        .child(label(k, 11., muted()).wrap())
        .child(title(v, 36.).w_full().truncate())
        .child(label(sub, 12., muted()).wrap())
}
pub(crate) fn field(edit: &TextEdit, key: &str) -> El {
    text_input(edit, TextStyle::new(13., 400.))
        .key(key)
        .w_full()
        .min_w(0.)
        .h(40.)
        .padding(10.)
        .radius(5.)
        .fill(surface())
        .border(1., line())
        .color(ink())
        .single_line()
}

pub(crate) fn tool_row(s: &State, i: usize, narrow: bool) -> El {
    let p = &s.config.profiles[i];
    let status = s.status.get(&p.id).map(String::as_str).unwrap_or("Stopped");
    let running = matches!(status, "Running" | "Starting");
    let available = match s.detected.get(&p.id) {
        Some(true) => "Path found",
        Some(false) => "Not detected",
        None => "Unchecked",
    };
    let info = row()
        .gap(12.)
        .center_y()
        .min_w(0.)
        .grow(1.)
        .child(el().size(8., 8.).radius(4.).fill(if running {
            c(19, 122, 44)
        } else {
            c(125, 120, 115)
        }))
        .child(
            col()
                .grow(1.)
                .min_w(0.)
                .gap(5.)
                .child(title(&p.name, 15.).wrap())
                .child(label(&p.description, 13., muted()).wrap()),
        );
    let actions = row()
        .gap(8.)
        .center_y()
        .flex_wrap()
        .child(badge(
            if running { status } else { available },
            running || available == "Path found",
        ))
        .child(
            button(format!("configure-{}", p.id), "Details", false, move |s| {
                s.selected = i;
                s.ui.inspector = Some(true);
            })
            .on_press_tx(move |s: &mut State, tx| {
                s.selected = i;
                s.ui.inspector = Some(true);
                tx.scroll_into_view("inspector");
            }),
        )
        .child(button(
            format!("start-{}", p.id),
            if running { "Stop" } else { "Run" },
            !running,
            move |s| {
                if running {
                    s.send(Request::Stop(s.config.profiles[i].id.clone()));
                } else {
                    s.start(i);
                }
            },
        ));
    let content = if narrow {
        col().gap(12.).child(info).child(actions)
    } else {
        row().gap(14.).center_y().child(info).child(actions)
    };
    col()
        .key(format!("tool-{}", p.id))
        .w_full()
        .min_w(0.)
        .padding(12.)
        .radius(8.)
        .fill(if s.selected == i {
            c(242, 249, 255)
        } else {
            bg()
        })
        .border(1., line())
        .child(content)
}

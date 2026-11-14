//! The comparison of a selected run with its flight: verdict, the two paths seen from above,
//! distance over time, events, commands and AI output.
use super::compare::{Report, seconds};
use crate::State;
use crate::ui::theme::Viewport;
use crate::ui::{kit::*, theme::*};
use rune::{El, LineCap, Shape, canvas, col, el, row};

const TRACK_HEIGHT: f32 = 260.;

/// Both paths from above (east right, north up), the same scale on both axes; the split is
/// marked.
fn track(r: &Report) -> El {
    let p = &r.path;
    let all = p.old.iter().chain(&p.new);
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for q in all {
        // [t, north (x), east (y)]
        x0 = x0.min(q[2]);
        x1 = x1.max(q[2]);
        y0 = y0.min(q[1]);
        y1 = y1.max(q[1]);
    }
    if x0 > x1 {
        return el();
    }
    let span = (x1 - x0).max(y1 - y0).max(2.) * 1.1;
    let (cx, cy) = ((x0 + x1) / 2., (y0 + y1) / 2.);
    let w = span as f32;
    let at = |q: &[f64; 3]| {
        [
            (q[2] - cx + span / 2.) as f32,
            (cy + span / 2. - q[1]) as f32,
        ]
    };
    let mut shape = Shape::new().view_box(w, w);
    for frac in [0., 0.5, 1.] {
        shape = shape
            .line(
                [0., w * frac],
                [w, w * frac],
                w * 0.002,
                LineCap::Butt,
                soft(),
            )
            .line(
                [w * frac, 0.],
                [w * frac, w],
                w * 0.002,
                LineCap::Butt,
                soft(),
            );
    }
    let stroke = w * 0.01;
    for (points, color) in [(&p.old, fg()), (&p.new, fg().alpha(0.45))] {
        if points.len() > 1 {
            shape = shape.stroke(
                points.iter().map(at).collect::<Vec<[f32; 2]>>(),
                stroke,
                color,
            );
        }
    }
    if let Some(t) = p.split_t
        && let Some(q) = p
            .old
            .iter()
            .min_by(|a, b| (a[0] - t).abs().total_cmp(&(b[0] - t).abs()))
    {
        let c = at(q);
        shape = shape.line(c, c, stroke * 3., LineCap::Round, danger());
    }
    let legend = row()
        .gaps(4., 16.)
        .flex_wrap()
        .child(swatch("recorded", fg()))
        .child(swatch("fresh", fg().alpha(0.45)))
        .child(meta_sized(format!("{:.0} m across", span), 9., muted()));
    col()
        .key("resim-track")
        .w_full()
        .min_w(0.)
        .gap(8.)
        .child(caps("Paths from above / north up", 11., 500., 0.08, fg()))
        .child(
            row().w_full().justify_center().child(
                canvas(shape)
                    .label("The recorded and fresh paths seen from above")
                    .w(TRACK_HEIGHT)
                    .h(TRACK_HEIGHT),
            ),
        )
        .child(legend)
}

fn swatch(label: &str, color: rune::Color) -> El {
    row()
        .gap(6.)
        .center_y()
        .child(el().w(14.).h(2.).fill(color).a11y_hidden())
        .child(meta_sized(label.to_string(), 9., muted()))
}

fn table_row(cells: &[String], strong: bool) -> El {
    let mut r = row().w_full().min_w(0.).gap(12.).py(4.);
    for (i, c) in cells.iter().enumerate() {
        let text = if strong {
            meta_sized(c.clone(), 9., muted())
        } else {
            code(c.clone(), 11., fg()).wrap_anywhere()
        };
        r = r.child(
            col()
                .min_w(0.)
                .grow(if i == 0 { 2. } else { 1. })
                .child(text),
        );
    }
    r
}

fn events(r: &Report) -> El {
    let e = &r.events;
    let mut c = col()
        .key("resim-events")
        .w_full()
        .min_w(0.)
        .child(subsection_head(
            "Events",
            meta(format!(
                "{} missing in the fresh flight / {} new",
                e.missing, e.extra
            )),
        ))
        .child(table_row(
            &[
                "Event".into(),
                "Recorded".into(),
                "Fresh".into(),
                "Shift".into(),
            ],
            true,
        ));
    for row in e.rows.iter().take(40) {
        c = c.child(table_row(
            &[
                format!("{}: {}", row.kind, row.value),
                seconds(row.old_t),
                seconds(row.new_t),
                row.shift
                    .map_or_else(|| "—".into(), |s| format!("{s:+.1} s")),
            ],
            false,
        ));
    }
    if e.rows.is_empty() {
        c = c.child(p(
            "No arming, mode, failsafe or landing changes in either flight.",
            11.,
        ));
    }
    c
}

fn commands(r: &Report) -> El {
    let mut c = col()
        .key("resim-commands")
        .w_full()
        .min_w(0.)
        .child(subsection_head(
            "Commands",
            meta(match r.takeover_t {
                Some(t) => format!("Your code took over at {t:.1} s"),
                None => "The recording steered throughout".into(),
            }),
        ))
        .child(table_row(
            &[
                "Topic".into(),
                "Recorded".into(),
                "From the recording".into(),
                "From your code".into(),
            ],
            true,
        ));
    for t in &r.commands.topics {
        c = c.child(table_row(
            &[
                t.topic.clone(),
                t.recorded.to_string(),
                t.from_recording.to_string(),
                t.from_your_code.to_string(),
            ],
            false,
        ));
    }
    c
}

fn ai(r: &Report) -> El {
    let mut c = col().key("resim-ai").w_full().min_w(0.);
    if r.ai.is_empty() {
        return c;
    }
    c = c.child(subsection_head(
        "AI output",
        meta("Agreement, not accuracy"),
    ));
    for a in &r.ai {
        let agree = if a.paired == 0 {
            "no messages to pair".to_string()
        } else {
            format!(
                "top label agrees on {} of {} paired messages ({:.0}%)",
                a.agree,
                a.paired,
                a.agree as f64 * 100. / a.paired as f64
            )
        };
        c = c.child(table_row(
            &[
                a.topic.clone(),
                format!("{} recorded / {} fresh", a.old, a.new),
                agree,
            ],
            false,
        ));
        for d in a.disagreements.iter().take(5) {
            c = c.child(
                body(
                    format!(
                        "  at {} s: recorded {}, fresh {}",
                        d["t"], d["old"], d["new"]
                    ),
                    10.,
                    muted(),
                )
                .wrap(),
            );
        }
    }
    c
}

pub(crate) fn view(s: &State, vp: &Viewport) -> El {
    let Some((name, result)) = &s.resim.report else {
        return col();
    };
    let head = row()
        .gaps(4., 16.)
        .flex_wrap()
        .child(ghost(
            "resim-compare-again",
            "Compare again",
            !s.resim.reporting,
            super::compare_again,
        ))
        .child(ghost("resim-report-close", "Close", true, |s| {
            s.resim.report = None
        }));
    let mut c = col()
        .key("resim-report")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .pt(16.)
        .child(section_head(
            &format!("Comparison / {name}"),
            head,
            vp.compact(),
        ));
    let r = match result {
        Ok(r) => r,
        Err(e) => return c.child(body(e.clone(), 11., warn()).wrap()),
    };
    let tone = match r.verdict.as_str() {
        "same" => success(),
        "changed" => warn(),
        _ => muted(),
    };
    c = c.child(h2(r.verdict_line(), 17.).key("resim-verdict").color(tone));
    for note in &r.notes {
        c = c.child(body(note.clone(), 11., muted()).wrap());
    }
    if r.path.compared > 0 {
        let marks: Vec<f64> = r.path.split_t.into_iter().chain(r.takeover_t).collect();
        c = c.child(track(r)).child(crate::ui::series_plot(
            &r.distance_plot(),
            r.duration(),
            &[],
            &marks,
        ));
    }
    c.child(events(r)).child(commands(r)).child(ai(r)).child(p(
        "Red marks where the course changed and where your code took over. Open both MCAPs in Foxglove from Recordings for the full picture.",
        11.,
    ))
}

//! Flight-log plots drawn with Rune shapes: one line per series over armed periods (a faint
//! wash) and failsafe marks (red), stretched to the element with a view box.
use super::*;
use crate::ulog::Plot;
use rune::{LineCap, Shape, canvas};

const HEIGHT: f32 = 132.;

fn label(v: f32, unit: &str) -> String {
    if v.abs() >= 100. {
        format!("{v:.0} {unit}")
    } else {
        format!("{v:.1} {unit}")
    }
}

/// `armed`: intervals (s) to wash; `marks`: failsafe times (s).
pub(super) fn plot(p: &Plot, duration: f32, armed: &[(f64, f64)], marks: &[f64]) -> El {
    let (lo, hi) = p.range().unwrap_or((0., 1.));
    let pad = ((hi - lo) * 0.08).max(0.5);
    let (lo, hi) = (lo - pad, hi + pad);
    let w = duration.max(1.);
    let h = hi - lo;
    let y = |v: f32| hi - v;
    let mut shape = Shape::new().view_box(w, h);
    for &(a, b) in armed {
        let (a, b) = (a as f32, (b as f32).max(a as f32 + w * 0.002));
        shape = shape.fill(vec![[a, 0.], [b, 0.], [b, h], [a, h]], fg().alpha(0.06));
    }
    for frac in [0., 0.5, 1.] {
        shape = shape.line([0., h * frac], [w, h * frac], 1., LineCap::Butt, soft());
    }
    for &t in marks {
        shape = shape.line([t as f32, 0.], [t as f32, h], 1., LineCap::Butt, danger());
    }
    let colors = [fg(), fg().alpha(0.45)];
    for (i, line) in p.lines.iter().enumerate() {
        let color = colors[i.min(1)];
        let mut run: Vec<[f32; 2]> = Vec::new();
        for pt in line.points.iter().chain(std::iter::once(&[0., f32::NAN])) {
            if pt[1].is_finite() {
                run.push([pt[0], y(pt[1])]);
            } else if !run.is_empty() {
                let pts = std::mem::take(&mut run);
                shape = if pts.len() == 1 {
                    shape.line(pts[0], pts[0], 2., LineCap::Round, color)
                } else {
                    shape.stroke(pts, 1.5, color)
                };
            }
        }
    }
    let mut legend = row().gaps(4., 16.).flex_wrap();
    for (i, line) in p.lines.iter().enumerate() {
        legend = legend.child(
            row()
                .gap(6.)
                .center_y()
                .child(el().w(14.).h(2.).fill(colors[i.min(1)]).a11y_hidden())
                .child(meta_sized(line.label, 9., muted())),
        );
    }
    let axis = col()
        .w(64.)
        .h(HEIGHT)
        .justify_between()
        .child(meta_sized(label(hi - pad, p.unit), 9., muted()))
        .child(meta_sized(label(lo + pad, p.unit), 9., muted()));
    col()
        .key(format!("plot-{}", p.id))
        .w_full()
        .min_w(0.)
        .gap(10.)
        .child(
            row()
                .w_full()
                .min_w(0.)
                .gaps(4., 16.)
                .flex_wrap()
                .justify_between()
                .child(caps(
                    format!("{} / {}", p.title, p.unit),
                    11.,
                    500.,
                    0.08,
                    fg(),
                ))
                .child(code(p.source, 10., muted())),
        )
        .child(
            row().w_full().min_w(0.).gap(8.).child(axis).child(
                canvas(shape)
                    .label(format!("{} plot", p.title))
                    .h(HEIGHT)
                    .grow(1.)
                    .min_w(0.),
            ),
        )
        .child(
            row()
                .w_full()
                .min_w(0.)
                .pl(72.)
                .gap(16.)
                .justify_between()
                .child(legend)
                .child(meta_sized(format!("0–{duration:.0} s"), 9., muted())),
        )
}

//! The attitude indicator (horizon, pitch ladder, roll scale) and the heading tape, drawn with
//! Rune shapes in the 0.8 language: black, one light ink, translucent rules.
use super::track::circle;
use crate::ui::theme::*;
use rune::{El, LineCap, Shape, canvas, el};

/// Degrees of pitch across the indicator's diameter.
const PITCH_SPAN: f32 = 90.;

/// Unit vectors on screen (y down) for a horizon rolled by `roll` degrees: `along` the horizon
/// and `up` towards the sky. Rolling right turns the horizon counter-clockwise.
pub(crate) fn horizon_axes(roll: f32) -> ([f32; 2], [f32; 2]) {
    let (s, c) = roll.to_radians().sin_cos();
    ([c, -s], [-s, -c])
}

fn dot2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// Keeps the part of convex polygon `poly` where `side(p) <= 0` (one Sutherland–Hodgman pass).
pub(crate) fn clip(poly: &[[f32; 2]], side: impl Fn([f32; 2]) -> f32) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    for i in 0..poly.len() {
        let (prev, cur) = (poly[(i + poly.len() - 1) % poly.len()], poly[i]);
        let (dp, dc) = (side(prev), side(cur));
        let cross = || {
            let t = dp / (dp - dc);
            [
                prev[0] + (cur[0] - prev[0]) * t,
                prev[1] + (cur[1] - prev[1]) * t,
            ]
        };
        if dc <= 0. {
            if dp > 0. {
                out.push(cross());
            }
            out.push(cur);
        } else if dp <= 0. {
            out.push(cross());
        }
    }
    out
}

/// The ground part of an indicator `size` points wide for this attitude.
pub(crate) fn ground(roll: f32, pitch: f32, size: f32) -> Vec<[f32; 2]> {
    let c = size / 2.;
    let r = c - 2.;
    let ppd = size / PITCH_SPAN;
    let (_, up) = horizon_axes(roll);
    let p0 = [c - up[0] * pitch * ppd, c - up[1] * pitch * ppd];
    clip(&circle([c, c], r, 72), |q| {
        dot2([q[0] - p0[0], q[1] - p0[1]], up)
    })
}

/// Attitude indicator `size`×`size` (key `vehicle-attitude`).
pub(crate) fn attitude(roll: Option<f32>, pitch: Option<f32>, size: f32) -> El {
    let c = size / 2.;
    let r = c - 2.;
    let mut shape = Shape::new();
    // ATTITUDE carries floats: a malformed NaN or infinity is treated as not reported.
    let (roll, pitch) = (
        roll.filter(|r| r.is_finite()),
        pitch.filter(|p| p.is_finite()),
    );
    let known = roll.is_some() && pitch.is_some();
    let (roll, pitch) = (roll.unwrap_or(0.), pitch.unwrap_or(0.).clamp(-80., 80.));
    let ppd = size / PITCH_SPAN;
    let (along, up) = horizon_axes(roll);
    if known {
        let ground = ground(roll, pitch, size);
        if ground.len() >= 3 {
            shape = shape.fill(ground, fg().alpha(0.14));
        }
        // Horizon and pitch ladder, clipped to the dial.
        for deg in [0., -20., -10., 10., 20.] {
            let offset = (deg - pitch) * ppd;
            if offset.abs() >= r - 6. {
                continue;
            }
            let p = [c + up[0] * offset, c + up[1] * offset];
            let half = if deg == 0. {
                (r * r - offset * offset).sqrt()
            } else if deg.abs() == 10. {
                size * 0.14
            } else {
                size * 0.09
            };
            let a = [p[0] - along[0] * half, p[1] - along[1] * half];
            let b = [p[0] + along[0] * half, p[1] + along[1] * half];
            let (width, color) = if deg == 0. { (1.5, fg()) } else { (1., rule()) };
            shape = shape.line(a, b, width, LineCap::Butt, color);
        }
    }
    shape = shape.stroke_with(circle([c, c], r, 72), true, 1., LineCap::Butt, rule());
    // Roll scale: ticks clockwise from the top of the dial.
    let at = |deg: f32, rad: f32| {
        let (s, co) = deg.to_radians().sin_cos();
        [c + rad * s, c - rad * co]
    };
    for deg in [-60f32, -45., -30., -20., -10., 0., 10., 20., 30., 45., 60.] {
        let long = deg.abs() % 30. == 0.;
        let inner = r - if long { 10. } else { 6. };
        shape = shape.line(at(deg, r), at(deg, inner), 1., LineCap::Butt, muted());
    }
    if known {
        // The sky pointer turns with the horizon.
        let a = -roll;
        shape = shape.fill(
            vec![at(a, r - 11.), at(a - 4., r - 19.), at(a + 4., r - 19.)],
            fg(),
        );
    }
    // The aircraft, fixed at the centre.
    let w = size * 0.2;
    shape = shape
        .line([c - w - 10., c], [c - 10., c], 2., LineCap::Butt, fg())
        .line([c - 10., c], [c - 5., c + 5.], 2., LineCap::Butt, fg())
        .line([c + 10., c], [c + w + 10., c], 2., LineCap::Butt, fg())
        .line([c + 10., c], [c + 5., c + 5.], 2., LineCap::Butt, fg())
        .fill(circle([c, c], 2., 12), fg());
    let dial = el()
        .key("vehicle-attitude")
        .role("image")
        .label(if known {
            format!("Attitude: roll {roll:.0}°, pitch {pitch:.0}°")
        } else {
            "Attitude not reported".into()
        })
        .size(size, size)
        .child(
            canvas(shape)
                .absolute(0., 0.)
                .size(size, size)
                .a11y_hidden(),
        );
    if known {
        return dial;
    }
    dial.child(
        meta_sized("Attitude not reported", 8., muted())
            .absolute(12., c + 14.)
            .w(size - 24.)
            .wrap(),
    )
}

/// Compass points labelled on the tape every 30°.
fn point_name(deg: i32) -> String {
    match deg.rem_euclid(360) {
        0 => "N".into(),
        90 => "E".into(),
        180 => "S".into(),
        270 => "W".into(),
        d => format!("{d:03}"),
    }
}

/// Heading tape `w` points wide showing ±45° around `heading` (key `vehicle-heading`).
pub(crate) fn heading_tape(heading: Option<f32>, w: f32) -> El {
    let h = 40.;
    let tape = el()
        .key("vehicle-heading")
        .role("image")
        .w(w)
        .h(h)
        .border_sides([1., 0., 0., 0.], soft());
    let Some(hdg) = heading else {
        return tape.label("Heading not reported").child(
            meta_sized("Heading not reported", 8., muted())
                .absolute(0., 14.)
                .a11y_hidden(),
        );
    };
    let ppd = w / 90.;
    let mut shape = Shape::new();
    let mut labels = Vec::new();
    let first = ((hdg - 45.) / 5.).ceil() as i32 * 5;
    let mut d = first;
    while (d as f32) <= hdg + 45. {
        let x = w / 2. + (d as f32 - hdg) * ppd;
        let len = if d % 30 == 0 {
            10.
        } else if d % 10 == 0 {
            6.
        } else {
            3.
        };
        shape = shape.line([x, 0.], [x, len], 1., LineCap::Butt, muted());
        if d % 30 == 0 && x > 10. && x < w - 10. {
            labels.push(
                meta_sized(point_name(d), 8., muted())
                    .absolute(x - 9., 16.)
                    .w(18.)
                    .align(rune::Align::Center)
                    .a11y_hidden(),
            );
        }
        d += 5;
    }
    shape = shape.fill(
        vec![[w / 2., 12.], [w / 2. - 4., 4.], [w / 2. + 4., 4.]],
        fg(),
    );
    tape.label(format!("Heading {:03.0}°", hdg.rem_euclid(360.)))
        .child(canvas(shape).absolute(0., 0.).w(w).h(h).a11y_hidden())
        .children(labels)
}

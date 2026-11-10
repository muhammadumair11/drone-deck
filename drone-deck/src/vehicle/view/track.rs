//! The local position trace: positions since the link came up, in metres east and north of
//! the first fix, drawn top-down with the home position (when the vehicle reports one), the
//! vehicle's heading and a scale bar. No map tiles.
use crate::mavlink::VehicleState;
use crate::ui::theme::*;
use rune::{El, LineCap, Shape, canvas, el};
use std::collections::VecDeque;

/// Points kept; the oldest go first.
pub(crate) const TRACK_POINTS: usize = 2000;
/// A new point needs this much movement (metres), so a hovering vehicle adds nothing.
const STEP_M: f32 = 0.25;
/// The smallest span drawn (metres), so a vehicle on the ground is not zoomed to centimetres.
const MIN_SPAN_M: f32 = 20.;
const EARTH_M: f64 = 6_371_000.;
/// A reported home further than this from the first fix (metres) is named, not drawn.
const HOME_RANGE_M: f32 = 5_000.;

#[derive(Clone, Debug, Default)]
pub(crate) struct Track {
    origin: Option<(f64, f64)>,
    points: VecDeque<[f32; 2]>,
}

impl Track {
    pub(crate) fn clear(&mut self) {
        self.origin = None;
        self.points.clear();
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
    pub(crate) fn len(&self) -> usize {
        self.points.len()
    }
    /// Metres east and north of the first fix; `None` before it.
    pub(crate) fn local(&self, lat: f64, lon: f64) -> Option<[f32; 2]> {
        let (lat0, lon0) = self.origin?;
        let east = (lon - lon0).to_radians() * EARTH_M * lat0.to_radians().cos();
        let north = (lat - lat0).to_radians() * EARTH_M;
        Some([east as f32, north as f32])
    }
    /// Adds a fix when it moved at least `STEP_M` from the last point. True when added.
    pub(crate) fn push_fix(&mut self, lat: f64, lon: f64) -> bool {
        if !lat.is_finite() || !lon.is_finite() || (lat == 0. && lon == 0.) {
            return false;
        }
        if self.origin.is_none() {
            self.origin = Some((lat, lon));
        }
        let Some(p) = self.local(lat, lon) else {
            return false;
        };
        if let Some(last) = self.points.back() {
            let (dx, dy) = (p[0] - last[0], p[1] - last[1]);
            if (dx * dx + dy * dy).sqrt() < STEP_M {
                return false;
            }
        }
        self.points.push_back(p);
        while self.points.len() > TRACK_POINTS {
            self.points.pop_front();
        }
        true
    }
    /// Folds a vehicle's position in (fresh heartbeats only; a stale value is not a fix).
    pub(crate) fn push(&mut self, v: &VehicleState, fresh: bool) -> bool {
        match (fresh, v.lat, v.lon) {
            (true, Some(lat), Some(lon)) => self.push_fix(lat, lon),
            _ => false,
        }
    }
}

/// Bounds of the drawing: centre and metres per point, so every point fits `w`×`h`.
pub(crate) struct Frame {
    mid: [f32; 2],
    per_pt: f32,
    w: f32,
    h: f32,
}

impl Frame {
    pub(crate) fn fit(points: &[[f32; 2]], w: f32, h: f32) -> Self {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in points {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        if points.is_empty() {
            (lo, hi) = ([0.; 2], [0.; 2]);
        }
        let span = |i: usize| (hi[i] - lo[i]).max(MIN_SPAN_M) * 1.25;
        let per_pt = (span(0) / w.max(1.)).max(span(1) / h.max(1.));
        Self {
            mid: [(lo[0] + hi[0]) / 2., (lo[1] + hi[1]) / 2.],
            per_pt,
            w,
            h,
        }
    }
    /// Screen point for a local position (north up).
    pub(crate) fn to_screen(&self, p: [f32; 2]) -> [f32; 2] {
        [
            self.w / 2. + (p[0] - self.mid[0]) / self.per_pt,
            self.h / 2. - (p[1] - self.mid[1]) / self.per_pt,
        ]
    }
    /// A round scale-bar length (metres) no longer than a third of the width.
    pub(crate) fn scale_bar(&self) -> f32 {
        let max = self.w / 3. * self.per_pt;
        let mut best = 1.;
        for decade in [1., 10., 100., 1000., 10_000.] {
            for step in [1., 2., 5.] {
                let len = step * decade;
                if len <= max {
                    best = len;
                }
            }
        }
        best
    }
}

fn label(t: impl Into<String>, x: f32, y: f32) -> El {
    meta_sized(t, 8., muted()).absolute(x, y).a11y_hidden()
}

/// The top-down trace, `w`×`h` points (key `vehicle-track`). A stale trace is drawn in a
/// darker opaque ink (a translucent stroke reads as dotted where segments overlap).
pub(crate) fn view(track: &Track, v: Option<&VehicleState>, fresh: bool, w: f32, h: f32) -> El {
    let frame_el = el()
        .key("vehicle-track")
        .role("image")
        .label("Position trace since the link came up")
        .w(w)
        .h(h)
        .border(1., soft());
    let current = v.and_then(|v| track.local(v.lat?, v.lon?));
    // 0,0 is an unset home, and a home further than HOME_RANGE_M would shrink the trace to a
    // dot: neither is drawn.
    let home = v
        .and_then(|v| v.home)
        .filter(|&(lat, lon, _)| lat.is_finite() && lon.is_finite() && (lat, lon) != (0., 0.))
        .and_then(|(lat, lon, _)| track.local(lat, lon))
        .filter(|p| p[0].hypot(p[1]) <= HOME_RANGE_M);
    if track.is_empty() || current.is_none() {
        return frame_el.child(
            meta_sized("No position fix since the link came up", 9., muted())
                .absolute(12., h / 2. - 6.)
                .w(w - 24.)
                .wrap(),
        );
    }
    let mut all: Vec<[f32; 2]> = track.points.iter().copied().collect();
    all.extend(current);
    all.extend(home);
    let f = Frame::fit(&all, w, h);
    let mut shape = Shape::new();
    let mark = if fresh { fg() } else { muted() };
    // Quarter grid, faint.
    for frac in [0.25, 0.5, 0.75] {
        shape = shape
            .line(
                [w * frac, 0.],
                [w * frac, h],
                1.,
                LineCap::Butt,
                fg().alpha(0.05),
            )
            .line(
                [0., h * frac],
                [w, h * frac],
                1.,
                LineCap::Butt,
                fg().alpha(0.05),
            );
    }
    let screen = thin(track.points.iter().map(|p| f.to_screen(*p)));
    if screen.len() > 1 {
        let ink = if fresh {
            c(176, 176, 184)
        } else {
            c(84, 84, 90)
        };
        shape = shape.stroke(screen.clone(), 2., ink);
    }
    let mut labels = Vec::new();
    if let Some(home) = home {
        let [x, y] = f.to_screen(home);
        let r = 5.;
        shape = shape.stroke_with(
            vec![
                [x - r, y - r],
                [x + r, y - r],
                [x + r, y + r],
                [x - r, y + r],
            ],
            true,
            1.5,
            LineCap::Butt,
            mark,
        );
        labels.push(label("Home", x + 8., y - 5.));
    } else if let Some(&start) = screen.first() {
        shape = shape.stroke_with(circle(start, 4., 16), true, 1., LineCap::Butt, muted());
        labels.push(label("Start", start[0] + 7., start[1] - 5.));
    }
    if let Some(p) = current {
        let [x, y] = f.to_screen(p);
        shape = shape.fill(circle([x, y], 4., 16), mark);
        if let Some(hdg) = v.and_then(|v| v.heading) {
            let a = hdg.to_radians();
            shape = shape.line(
                [x, y],
                [x + 16. * a.sin(), y - 16. * a.cos()],
                2.,
                LineCap::Round,
                mark,
            );
        }
    }
    let bar = f.scale_bar();
    let len = bar / f.per_pt;
    shape = shape
        .line(
            [12., h - 12.],
            [12. + len, h - 12.],
            2.,
            LineCap::Butt,
            mark,
        )
        .line([12., h - 16.], [12., h - 8.], 1., LineCap::Butt, mark)
        .line(
            [12. + len, h - 16.],
            [12. + len, h - 8.],
            1.,
            LineCap::Butt,
            mark,
        );
    labels.push(label(format!("{bar:.0} m"), 12., h - 30.));
    labels.push(label("N ↑", w - 30., 8.));
    labels.push(label(
        match (home, v.and_then(|v| v.home)) {
            (Some(_), _) => "Since link up",
            (None, Some(_)) => "Since link up / home outside the trace",
            (None, None) => "Since link up / home not reported",
        },
        12.,
        8.,
    ));
    frame_el
        .child(canvas(shape).absolute(0., 0.).w(w).h(h).a11y_hidden())
        .children(labels)
}

pub(crate) fn circle(c: [f32; 2], r: f32, n: usize) -> Vec<[f32; 2]> {
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// Drops screen points closer than 3 points to the last kept one (the last always stays), so
/// a long trace draws as few, clean segments.
pub(crate) fn thin(points: impl Iterator<Item = [f32; 2]>) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::new();
    let mut last = None;
    for p in points {
        last = Some(p);
        if out
            .last()
            .is_none_or(|q| (p[0] - q[0]).hypot(p[1] - q[1]) >= 3.)
        {
            out.push(p);
        }
    }
    if let Some(p) = last
        && out.last() != Some(&p)
    {
        out.push(p);
    }
    out
}

//! A re-simulation's comparison with the flight it replayed, as `scripts/agent/compare.py`
//! writes it (`compare.json`, read through `resim.report`). The app only renders it.
use crate::ulog::plots::{Line, Plot};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Path {
    pub verdict: String,
    pub compared: u64,
    pub split_t: Option<f64>,
    pub max_m: f64,
    pub max_t: f64,
    pub mean_m: f64,
    pub floor_m: f64,
    /// `[s, x, y]` of the recorded and fresh vehicle (north, east in metres).
    pub old: Vec<[f64; 3]>,
    pub new: Vec<[f64; 3]>,
    /// `[s, metres]`.
    pub distance: Vec<[f64; 2]>,
    pub allowed: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct EventRow {
    pub kind: String,
    pub value: String,
    pub old_t: Option<f64>,
    pub new_t: Option<f64>,
    pub shift: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Events {
    pub rows: Vec<EventRow>,
    pub missing: u64,
    pub extra: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct CommandRow {
    pub topic: String,
    pub recorded: u64,
    pub from_recording: u64,
    pub from_your_code: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Commands {
    pub topics: Vec<CommandRow>,
    pub steering: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Ai {
    pub topic: String,
    pub old: u64,
    pub new: u64,
    pub paired: u64,
    pub agree: u64,
    pub disagreements: Vec<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Report {
    pub verdict: String,
    /// Fraction (0.1 = 10%).
    pub tolerance: f64,
    pub path: Path,
    pub events: Events,
    pub commands: Commands,
    pub takeover_t: Option<f64>,
    pub ai: Vec<Ai>,
    pub notes: Vec<String>,
}

impl Report {
    /// The headline: same course, changed at t, or why there is no verdict.
    pub(crate) fn verdict_line(&self) -> String {
        let p = &self.path;
        let pct = self.tolerance * 100.;
        match self.verdict.as_str() {
            "same" => format!(
                "Same course: within {pct:.0}% (at least {:.1} m) all flight; at most {:.2} m apart, {:.2} m on average.",
                p.floor_m, p.max_m, p.mean_m
            ),
            "changed" => format!(
                "Changed at t = {:.1} s: beyond {pct:.0}% (at least {:.1} m); at most {:.2} m apart at {:.1} s.",
                p.split_t.unwrap_or(0.),
                p.floor_m,
                p.max_m,
                p.max_t
            ),
            _ => "No verdict: the flights could not be put side by side.".into(),
        }
    }

    /// Distance between the flights over time, with what the tolerance allowed.
    pub(crate) fn distance_plot(&self) -> Plot {
        let points = |v: &[[f64; 2]]| v.iter().map(|p| [p[0] as f32, p[1] as f32]).collect();
        Plot {
            id: "resim-distance",
            title: "Distance between the flights",
            unit: "m",
            source: "vehicle_local_position",
            lines: vec![
                Line {
                    label: "apart",
                    points: points(&self.path.distance),
                },
                Line {
                    label: "allowed",
                    points: points(&self.path.allowed),
                },
            ],
        }
    }

    /// The flight's length in seconds (for the plot axis).
    pub(crate) fn duration(&self) -> f32 {
        self.path
            .distance
            .last()
            .map(|p| p[0])
            .or_else(|| self.path.old.last().map(|p| p[0]))
            .unwrap_or(1.) as f32
    }
}

/// A seconds value or "—".
pub(crate) fn seconds(t: Option<f64>) -> String {
    t.map_or_else(|| "—".to_string(), |t| format!("{t:.1} s"))
}

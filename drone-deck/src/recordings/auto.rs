//! The `mcap` launch stage: a system with "Record when the session starts" records its saved
//! topics once they appear on its ROS domain, as the last stage of its launch. Stop session
//! stops the recording like any other (`State::stop_all`).
use super::send;
use crate::vehicle::{Stage, Step};
use crate::*;

/// How often the stage asks the recorder for the topics on the domain while it waits.
const ASK_EVERY: Duration = Duration::from_secs(2);

/// Where the automatic recording is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Auto {
    #[default]
    Idle,
    /// The recorder's dependencies are being checked.
    Checking,
    /// Waiting for the system's topics; the last time the topic list was asked for.
    Waiting(Option<Instant>),
    /// The record request is out.
    Starting,
}

/// What the recorder reports, as the stage sees it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Seen {
    pub ready: bool,
    pub pending: bool,
    pub recording: bool,
    /// A record request is out and not yet confirmed.
    pub unconfirmed: bool,
    /// One of the wanted topics is on the domain.
    pub topic: bool,
}

/// The stage's next move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Move {
    Wait,
    Ask(&'static str),
    Ready,
    Fail,
}

/// The next state and move from the current state and what the recorder reports.
pub(crate) fn decide(auto: Auto, seen: Seen, start: bool, now: Instant) -> (Auto, Move) {
    if seen.recording {
        return (Auto::Idle, Move::Ready);
    }
    if start {
        return if seen.ready {
            (Auto::Waiting(Some(now)), Move::Ask("topics"))
        } else {
            (Auto::Checking, Move::Ask("check"))
        };
    }
    if seen.pending {
        return (auto, Move::Wait);
    }
    match auto {
        Auto::Idle => (auto, Move::Wait),
        Auto::Checking if !seen.ready => (Auto::Idle, Move::Fail),
        Auto::Checking => (Auto::Waiting(None), Move::Wait),
        // A confirmed record request takes `unconfirmed`; one still unconfirmed was refused.
        Auto::Starting if seen.unconfirmed => (Auto::Idle, Move::Fail),
        Auto::Starting => (Auto::Idle, Move::Ready),
        Auto::Waiting(_) if seen.topic => (Auto::Starting, Move::Ask("record")),
        Auto::Waiting(asked) if asked.is_none_or(|t| now.duration_since(t) >= ASK_EVERY) => {
            (Auto::Waiting(Some(now)), Move::Ask("topics"))
        }
        Auto::Waiting(_) => (auto, Move::Wait),
    }
}

/// The topics the automatic recording takes: the system's saved selection, else the
/// autopilot's own topics (what a re-simulation needs).
pub(crate) fn wanted(s: &State) -> Vec<String> {
    let saved = &s.systems.current().consumers.ros.topics;
    if !saved.is_empty() {
        return saved.clone();
    }
    crate::resim::recipe::wanted_topics(s)
}

/// Starts or checks the stage (`vehicle::stage` dispatches `mcap` here).
pub(crate) fn stage(s: &mut State, step: Step) -> Stage {
    let wanted = wanted(s);
    let r = &s.recorder;
    let seen = Seen {
        ready: r.ready,
        pending: r.pending,
        recording: r.recording,
        unconfirmed: r.saving.is_some(),
        topic: r.discovered.iter().any(|t| wanted.contains(&t.name)),
    };
    let start = matches!(step, Step::Start);
    if start && s.headless {
        return Stage::Failed("Preview: recording processes disabled.".into());
    }
    let (auto, step) = decide(r.auto, seen, start, Instant::now());
    s.recorder.auto = auto;
    match step {
        Move::Wait => Stage::Waiting,
        Move::Ready => Stage::Ready,
        Move::Fail => Stage::Failed(format!(
            "automatic recording did not start: {}",
            s.recorder.message
        )),
        Move::Ask(action) => {
            if action == "record" {
                s.recorder.topics = TextEdit::new(wanted.join(" "));
            }
            send(s, action);
            if !s.recorder.pending {
                s.recorder.auto = Auto::Idle;
                return Stage::Failed(format!(
                    "automatic recording did not start: {}",
                    s.recorder.message
                ));
            }
            if action == "topics" {
                s.recorder.message = if wanted.is_empty() {
                    "Automatic recording: waiting for the autopilot's topics on the system's domain…".into()
                } else {
                    format!(
                        "Automatic recording: waiting for {} on the system's domain…",
                        wanted.join(", ")
                    )
                };
            }
            Stage::Waiting
        }
    }
}

#[cfg(test)]
#[path = "auto_tests.rs"]
mod tests;

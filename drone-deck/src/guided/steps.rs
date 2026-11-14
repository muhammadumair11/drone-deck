//! The first three chapters. Vehicle: a starting point, or the vehicle and autopilot piece by
//! piece, and the system's name. World: where a simulated autopilot flies. Listeners: what
//! the vehicle feeds. They edit `Guided::draft` with Settings → Systems' controls
//! (`ui::vehicle_form`); leaving Listeners saves the system and opens Review & install.
use super::view::{Frame, frame, intent_row, locked_note, next, scope_note};
use super::*;
use crate::ui::kit::*;
use crate::ui::theme::*;
use crate::ui::vehicle_form as form;

fn at(s: &mut State) -> &mut systems::Draft {
    &mut s.guided.draft
}

fn label(t: &str) -> El {
    caps(t, 10., 400., 0.08, muted())
}

/// The saved system the draft updates, or the chosen template for a new one.
fn base(s: &State) -> systems::System {
    target(s)
        .cloned()
        .unwrap_or_else(|| systems::model::template(s.guided.choice, "", ""))
}

/// Chapter 1: starting points (`intent-<i>`), the name (`name`), then the vehicle kind and
/// its autopilot, endpoint or recording.
pub(super) fn vehicle(s: &State, f: &Frame) -> El {
    let d = &s.guided.draft;
    let mut starts = col()
        .key("intent-list")
        .role("radiogroup")
        .label("System starting point")
        .w_full()
        .min_w(0.);
    for i in 0..4 {
        starts = starts.child(intent_row(s, i, f));
    }
    let name = col()
        .w_full()
        .max_w(440.)
        .min_w(0.)
        .gap(10.)
        .child(labeled(
            "Give your system a name",
            field(&s.guided.name, "name", "My first system")
                .on_edit(|s: &mut State| &mut s.guided.name),
        ))
        .child(
            body(s.guided.name_error.clone(), 11., danger())
                .key("nameerror")
                .role("status"),
        );
    let details = match d.vehicle {
        1 => form::physical(s, d, at),
        2 => form::replay(d, at),
        _ => col()
            .w_full()
            .min_w(0.)
            .gap(8.)
            .child(label("Autopilot"))
            .child(form::autopilot_tabs(d, at)),
    };
    let mut content = col()
        .w_full()
        .min_w(0.)
        .gap(12.)
        .child(label("Start from"))
        .child(starts)
        .child(notice(scope_note(s)).key("intent-scope"))
        .child(name.mt(18.))
        .child(label("Or choose the pieces").mt(18.))
        .child(form::vehicle_tabs(d, at))
        .child(details);
    if s.setup.active() {
        content = content.child(notice(locked_note(s)));
    }
    let world = d.vehicle == 0;
    frame(
        s,
        f,
        "01 / The vehicle",
        "What will fly?",
        "Start from a ready-made system, or choose the pieces: a simulated autopilot, a real drone, or a recording to replay. Everything stays changeable.",
        content,
        "No tool knowledge required. Nothing installs yet.",
        next(
            if world {
                "Choose the world →"
            } else {
                "Choose who listens →"
            },
            true,
            move |s| {
                if check_name(s) {
                    go(s, if s.guided.draft.vehicle == 0 { 1 } else { 2 });
                }
            },
        ),
    )
}

/// The entered name is usable; otherwise its reason is shown under the field.
fn check_name(s: &mut State) -> bool {
    match systems::clean_name(&s.guided.name.text) {
        Ok(_) => {
            s.guided.name_error.clear();
            true
        }
        Err(e) => {
            s.guided.name_error = e;
            go(s, 0);
            false
        }
    }
}

/// Chapter 2: the world rows (`system-world-<id>`) and their details. A world the
/// autopilot cannot fly in is listed, disabled, with the reason.
pub(super) fn world(s: &State, f: &Frame) -> El {
    let d = &s.guided.draft;
    let content = if d.vehicle == 0 {
        let saved = target(s);
        col()
            .w_full()
            .min_w(0.)
            .gap(12.)
            .child(label(&format!(
                "Worlds for {}",
                systems::adapter::autopilot_name(d.autopilot)
            )))
            .child(form::worlds(d, saved, at, f.narrow()))
    } else {
        col().child(
            notice("A real drone flies in the real world, and a recording replays what was captured: there is no simulated world to choose.")
                .key("world-not-needed"),
        )
    };
    frame(
        s,
        f,
        "02 / The world",
        "Where does it fly?",
        "The autopilot's own physics is the lightest start. A 3D simulator adds cameras, terrain and a view. Worlds Drone Deck cannot install itself come with step-by-step guides.",
        content,
        "Simulators you install yourself are checked in Review & install.",
        next("Choose who listens →", true, |s| go(s, 2)),
    )
}

/// Chapter 3: the consumers the vehicle can feed, a line for the system as chosen, and the
/// reason the draft cannot be saved (`draft-error`).
pub(super) fn listeners(s: &State, f: &Frame) -> El {
    let d = &s.guided.draft;
    let summary = match d.apply(&base(s)) {
        Ok(sys) => sys.summary(),
        Err(e) => e,
    };
    // The starting point's note on how its parts connect, while its vehicle is unchanged.
    let start = systems::Draft::of(&systems::model::template(s.guided.choice, "", ""));
    let unchanged =
        (start.vehicle, start.autopilot, &start.world) == (d.vehicle, d.autopilot, &d.world);
    let mut content = col()
        .w_full()
        .min_w(0.)
        .gap(16.)
        .child(
            meta(format!("Your system / {summary}"))
                .key("draft-summary")
                .wrap(),
        )
        .child(form::consumers(d, at, f.narrow()));
    if unchanged {
        content = content.child(notice(STORIES[s.guided.choice].extra).max_w(860.));
    }
    let content = content.child(
        body(d.error.clone(), 11., danger())
            .key("draft-error")
            .role("status")
            .w_full(),
    );
    frame(
        s,
        f,
        "03 / The listeners",
        "Who listens to it?",
        "A ground station, your scripts, ROS 2, a recorder, a model: pick what the vehicle should feed. Only what fits the vehicle you chose is offered.",
        content,
        "Saving the system installs nothing; the next chapter shows what it needs.",
        next("Review and install →", true, advance),
    )
}

/// Leaves Listeners: applies the draft to the system it updates (creating it under the
/// entered name when it is new), saves it, and opens Review & install.
fn advance(s: &mut State) {
    if !check_name(s) {
        return;
    }
    let name = s.guided.name.text.trim().to_string();
    s.guided.draft.name = TextEdit::new(&name);
    let base = base(s);
    let next = match s.guided.draft.apply(&base) {
        Ok(next) => next,
        Err(e) => {
            s.guided.draft.error = e;
            return;
        }
    };
    if let Some(t) = target(s)
        && t.vehicle != next.vehicle
        && systems::owns_work(s, &t.id)
    {
        s.guided.draft.error = format!(
            "{} has running work. Stop its session before changing the vehicle; the listeners can change now.",
            t.label()
        );
        return;
    }
    s.guided.draft.error.clear();
    let id = ensure_target(s);
    let Some(sys) = s.systems.get_mut(&id) else {
        return;
    };
    sys.name = next.name;
    sys.vehicle = next.vehicle;
    sys.consumers = next.consumers;
    let kind = sys.kind();
    s.systems.changed();
    if let Err(e) = s.systems.save(s.headless) {
        s.notice = format!("The system could not be saved: {e}");
    }
    // Review & install and First run follow what the system is now.
    s.guided.choice = kind;
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
    }
    go(s, REVIEW);
}

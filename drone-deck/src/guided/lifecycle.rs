//! Draft identity, review invalidation and transactional persistence for guided setup.
use super::*;
use serde_json::json;

pub(crate) fn target(s: &State) -> Option<&systems::System> {
    s.guided
        .existing
        .as_deref()
        .and_then(|id| s.systems.get(id))
}

pub(super) fn base(s: &State) -> systems::System {
    target(s)
        .cloned()
        .unwrap_or_else(|| systems::model::starter(systems::SIMULATION, "", ""))
}

pub(super) fn candidate(s: &State) -> Result<systems::System, String> {
    if s.guided.existing.is_some() && target(s).is_none() {
        return Err("That system no longer exists. Start a new setup.".into());
    }
    let d = &s.guided.draft;
    if d.vehicle == 0 {
        systems::adapter::world_for(d.autopilot, &d.world)?;
    }
    d.apply(&base(s))
}

fn signature(sys: &systems::System, name: bool) -> String {
    let mut value = serde_json::to_value(sys).unwrap();
    let obj = value.as_object_mut().unwrap();
    for key in ["prepared", "template", "id"] {
        obj.remove(key);
    }
    if !name {
        obj.remove("name");
    }
    value.to_string()
}

/// Raw fields also detect invalid edits, without treating a rename as a configuration change.
fn fingerprint(s: &State) -> String {
    let d = &s.guided.draft;
    json!([
        d.vehicle,
        format!("{:?}", d.autopilot),
        d.world,
        format!("{:?}", d.hint),
        d.params
            .iter()
            .map(|(k, v)| (*k, v.text.as_str()))
            .collect::<Vec<_>>(),
        d.endpoint.text,
        d.recording.text,
        d.qgc,
        d.live_view,
        d.ros_agent,
        d.recorder,
        d.domain.text,
        d.inference,
        d.project.text,
        d.foxglove,
        d.scripts,
        d.scripts_udp.text,
        s.config.distro,
        s.setup.distro.text
    ])
    .to_string()
}

pub(crate) fn check_context(s: &State) -> String {
    let overrides: std::collections::BTreeMap<_, _> = s.setup.overrides.iter().collect();
    format!("{}{:?}", fingerprint(s), overrides)
}

pub(crate) fn saved(s: &State) -> bool {
    target(s).is_some()
        && candidate(s).is_ok_and(|c| signature(&c, true) == s.guided.saved_fingerprint)
}

fn reset_evidence(g: &mut Guided) {
    g.consent = false;
    g.applied = false;
    g.check = Check::Idle;
    g.task = 0;
    g.awaiting = false;
    g.done = false;
    g.evidence.clear();
    g.launched = None;
    g.prior_runs.clear();
    g.run_recording = None;
    g.playback_seen = false;
    g.last_query = None;
    g.queries = 0;
}

pub(super) fn reconcile(s: &mut State) -> bool {
    let next = fingerprint(s);
    if next == s.guided.fingerprint {
        return false;
    }
    if !s.guided.fingerprint.is_empty() {
        if s.setup.pending && s.guided.check == Check::Tools {
            s.setup.discard_detect = true;
        }
        reset_evidence(&mut s.guided);
        s.setup.queue.clear();
    }
    s.guided.fingerprint = next;
    true
}

fn for_system(s: &State, id: &str) -> Guided {
    let sys = s.systems.get(id).expect("known saved system");
    Guided {
        draft: systems::Draft::of(sys),
        existing: Some(id.into()),
        saved_fingerprint: signature(sys, true),
        opened_for: s.systems.active_id().into(),
        ..Default::default()
    }
}

/// New checkpoints carry identity; legacy checkpoints attach only to a unique match.
pub(crate) fn batch_system(s: &State) -> Option<String> {
    let b = s.setup.batch.as_ref()?;
    if let Some(id) = b.system.as_ref() {
        return s.systems.get(id).map(|sys| sys.id.clone());
    }
    let matches: Vec<_> = s
        .systems
        .saved()
        .filter(|sys| presets::for_system(sys) == s.setup.workflow)
        .collect();
    (matches.len() == 1).then(|| matches[0].id.clone())
}

pub(super) fn follow_batch(s: &mut State) {
    if s.setup.batch.is_none() {
        return;
    }
    let owner = batch_system(s);
    if let Some(id) = owner.as_ref() {
        if s.guided.existing.as_deref() != Some(&id) {
            let from = s.guided.came_from;
            s.guided = for_system(s, id);
            s.guided.came_from = from;
        }
    }
    s.guided.queue_unbound = owner.is_none();
    s.guided.fingerprint = fingerprint(s);
    s.guided.chapter = REVIEW;
    s.guided.applied = true;
}

pub(crate) fn open(s: &mut State) {
    let from = if s.page == ui::Page::Setup {
        s.guided.came_from
    } else {
        s.page
    };
    if s.guided.opened_for != s.systems.active_id() {
        s.guided = for_system(s, s.systems.active_id());
    }
    reconcile(s);
    follow_batch(s);
    s.guided.came_from = from;
    s.navigate(ui::Page::Setup);
}

pub(crate) fn new_system(s: &mut State) {
    let from = if s.page == ui::Page::Setup {
        s.guided.came_from
    } else {
        s.page
    };
    s.guided = Guided {
        opened_for: s.systems.active_id().into(),
        came_from: from,
        ..Default::default()
    };
    let first = s.systems.suggest_name("My system");
    s.guided.draft.name = TextEdit::new(&first);
    s.guided.suggested = first;
    reconcile(s);
    follow_batch(s);
    s.navigate(ui::Page::Setup);
}

pub(crate) fn restart(s: &mut State) {
    if s.setup.active() {
        return;
    }
    let from = s.guided.came_from;
    if let Some(id) = s
        .guided
        .existing
        .clone()
        .filter(|id| s.systems.get(id).is_some())
    {
        s.guided = for_system(s, &id);
        reconcile(s);
    } else {
        new_system(s);
    }
    s.guided.came_from = from;
    s.ui.reset_scroll = true;
}

/// Save once, rollback in memory on a write failure, and never create a template on disk.
pub(super) fn save_draft(s: &mut State) {
    save_with(s, |systems, headless| systems.save(headless));
}

pub(super) fn save_with(
    s: &mut State,
    persist: impl FnOnce(&systems::Systems, bool) -> Result<(), String>,
) {
    reconcile(s);
    if s.setup.active() {
        s.guided.prep_error = "Finish the active installation before saving changes.".into();
        return;
    }
    let mut next = match candidate(s) {
        Ok(c) => c,
        Err(e) => {
            s.guided.prep_error = e;
            return;
        }
    };
    let previous = target(s).cloned();
    if previous
        .as_ref()
        .is_some_and(|old| old.vehicle != next.vehicle && systems::owns_work(s, &old.id))
    {
        s.guided.prep_error = "Stop this system's session before changing its vehicle.".into();
        return;
    }
    let configured_changed = previous
        .as_ref()
        .is_none_or(|old| signature(old, false) != signature(&next, false));
    if configured_changed {
        next.prepared = false;
    }
    let old_list = s.systems.list.clone();
    let id = if let Some(old) = &previous {
        old.id.clone()
    } else {
        s.systems.add(next.clone())
    };
    next.id = id.clone();
    *s.systems.get_mut(&id).unwrap() = next.clone();
    s.systems.changed();
    if let Err(e) = persist(&s.systems, s.headless) {
        s.systems.list = old_list;
        s.systems.changed();
        s.guided.prep_error = format!("Could not save the system: {e}");
        return;
    }
    s.guided.existing = Some(id.clone());
    s.guided.saved_fingerprint = signature(&next, true);
    s.guided.prep_error.clear();
    use_preset(s);
    if s.systems.active_id() == id {
        systems::follow_active(s);
    }
    activity::record(s, format!("Saved system {}", next.label()));
}

pub(crate) fn ensure_target(s: &mut State) -> String {
    target(s).map(|t| t.id.clone()).unwrap_or_default()
}

pub(crate) fn mark_prepared(s: &mut State) -> bool {
    if s.guided.queue_unbound
        || !saved(s)
        || preset(s) != s.setup.workflow
        || !setup::workflow::verified_all(s)
    {
        s.guided.prep_error = "Save the system and verify its tools before continuing.".into();
        return false;
    }
    let id = ensure_target(s);
    let old = s.systems.get(&id).unwrap().prepared;
    s.systems.get_mut(&id).unwrap().prepared = true;
    if let Err(e) = s.systems.save(s.headless) {
        s.systems.get_mut(&id).unwrap().prepared = old;
        s.guided.prep_error = format!("Could not save preparation: {e}");
        return false;
    }
    let active = s.select_system(&id);
    s.guided.opened_for = s.systems.active_id().into();
    active
}

pub(crate) fn open_workspace(s: &mut State) {
    if mark_prepared(s) {
        s.navigate(ui::Page::System);
    }
}

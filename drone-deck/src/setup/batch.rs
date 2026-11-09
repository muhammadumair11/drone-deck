//! The reviewed workflow queue: installs missing tools one job at a time and configures
//! each after verification. Its checkpoint survives app restarts.
use super::*;
use crate::ui::kit::*;
use crate::ui::theme::*;
#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Batch {
    remaining: Vec<usize>,
    waiting: Option<String>,
    distro: String,
}
impl Batch {
    /// Recipe indices still to install.
    pub(crate) fn remaining(&self) -> &[usize] {
        &self.remaining
    }
    /// The job id being waited on, once its job has started.
    pub(crate) fn waiting(&self) -> Option<&str> {
        self.waiting
            .as_deref()
            .filter(|id| !id.starts_with("starting:"))
    }
}
pub(super) fn restore(s: &mut State) {
    if s.headless {
        return;
    }
    let Ok(bytes) = std::fs::read(backend::root().join("setup/workflow-state.json")) else {
        return;
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        s.setup.batch_message =
            "Previous workflow state could not be read. Recheck the workflow before continuing."
                .into();
        return;
    };
    if value["batch"].is_null() {
        return;
    }
    if value["app_version"].as_str() != Some(env!("CARGO_PKG_VERSION")) {
        s.setup.batch_message =
            "A previous workflow used another app version. Recheck/review it before continuing."
                .into();
        return;
    }
    if let Ok(batch) = serde_json::from_value::<Batch>(value["batch"].clone()) {
        let valid_wait = batch.waiting.as_ref().is_none_or(|id| {
            id.starts_with("starting:") || id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
        if valid_wait
            && batch
                .remaining
                .iter()
                .all(|i| *i < s.setup.recipes.len() && s.setup.recipes[*i].automatic)
        {
            s.setup.workflow =
                (value["workflow"].as_u64().unwrap_or(0) as usize).min(workflow::PRESETS.len() - 1);
            s.setup.batch = Some(batch);
            s.setup.batch_message =
                "Resuming the previously reviewed workflow; job state is checked first.".into();
        }
    }
}
pub(super) fn persist(s: &mut State) -> bool {
    if s.headless || (s.setup.batch.is_none() && s.setup.batch_message.is_empty()) {
        return false;
    }
    let value=json!({"app_version":env!("CARGO_PKG_VERSION"),"batch":s.setup.batch,"workflow":s.setup.workflow,"message":s.setup.batch_message}).to_string();
    if value == s.setup.checkpoint_cache {
        return false;
    }
    let path = backend::root().join("setup/workflow-state.json");
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, &value)?;
        std::fs::rename(temporary, path)
    })();
    s.setup.checkpoint_cache = value;
    if let Err(error) = result {
        s.setup.batch_message =
            format!("Workflow active, but its queue could not be saved: {error}");
        return true;
    }
    false
}
/// Adopts every verified tool of the preset and queues the rest for installation.
pub(crate) fn begin(s: &mut State) {
    if s.headless {
        s.setup.message = "Headless preview: installation is disabled.".into();
        return;
    }
    if s.busy() || s.setup.pending {
        s.setup.message =
            "Stop running tools, recordings and installations before applying the plan.".into();
        return;
    }
    if workflow::needs_wsl(s) && (!s.wsl_ready() || s.setup.distro.text.trim() != s.config.distro) {
        s.setup.message = "Select a working WSL 2 environment before applying the plan.".into();
        return;
    }
    let ids = workflow::PRESETS[s.setup.workflow].1;
    if !workflow::checked_all(s) {
        s.setup.message =
            "Check the entire workflow in the selected environment before installation.".into();
        return;
    }
    let candidates: Vec<_> = ids
        .iter()
        .filter_map(|id| s.setup.checks.get(*id).filter(|c| c.compatible).cloned())
        .collect();
    for candidate in candidates {
        let index = s
            .setup
            .recipes
            .iter()
            .position(|r| r.id == candidate.tool)
            .unwrap();
        choose(s, index);
        s.setup.candidate = Some(candidate);
        profiles::adopt(s);
        if s.setup.last_applied.is_none() {
            return;
        }
    }
    let mut remaining = Vec::new();
    for id in ids {
        if !s.setup.checks.get(*id).is_some_and(|c| c.compatible) {
            remaining.push(s.setup.recipes.iter().position(|r| r.id == *id).unwrap());
        }
    }
    s.setup.batch = Some(Batch {
        remaining,
        waiting: None,
        distro: s.setup.distro.text.clone(),
    });
    s.setup.configure_after = true;
    s.setup.batch_message="Workflow setup started. Components install sequentially and are configured after verification.".into();
}
pub(super) fn started(s: &mut State, id: &str) {
    if let Some(batch) = s.setup.batch.as_mut() {
        batch.waiting = Some(id.into());
    }
}
pub(super) fn poll(s: &mut State) -> bool {
    if s.setup.pending {
        return false;
    }
    let Some(mut batch) = s.setup.batch.take() else {
        return false;
    };
    if batch.distro != s.setup.distro.text {
        s.setup.batch_message =
            "Workflow paused: selected environment changed. Check it again before continuing."
                .into();
        return true;
    }
    if let Some(id) = &batch.waiting {
        if id.starts_with("starting:") {
            s.setup.batch_message="Workflow paused after an interrupted job handoff. Review the persisted installation jobs, then recheck the workflow.".into();
            return true;
        }
        let Some(job) = s.setup.jobs.iter().find(|j| &j.id == id) else {
            if root().join(id).join("state.json").exists() {
                s.setup.batch = Some(batch);
                return false;
            }
            s.setup.batch_message="Workflow paused: a job record is missing. Recheck installed components before continuing.".into();
            return true;
        };
        if job.alive
            || ["Queued", "Running", "Downloading", "Installing", "Checking"]
                .contains(&job.status.as_str())
        {
            s.setup.batch = Some(batch);
            return false;
        }
        if !["Ready", "Existing installation"].contains(&job.status.as_str()) {
            s.setup.batch_message = format!(
                "Workflow paused at {}: {}. Completed components were preserved; review this job, then check the workflow again.",
                job.tool, job.message
            );
            return true;
        }
        if job.configure_after && !s.setup.applied_jobs.contains(id) {
            s.setup.batch_message =
                "Workflow waiting for verified launch configuration to be saved.".into();
            s.setup.batch = Some(batch);
            return false;
        }
        batch.waiting = None;
    }
    if batch.remaining.is_empty() {
        s.setup.batch_message="Workflow tools installed/configured. Launch and connection checks are next; flight readiness is separate.".into();
        return true;
    }
    if s.busy() {
        s.setup.batch_message =
            "Workflow waiting for active tools/recordings to stop before changing dependencies."
                .into();
        s.setup.batch = Some(batch);
        return false;
    }
    let index = batch.remaining.remove(0);
    batch.waiting = Some(format!("starting:{}", s.setup.recipes[index].id));
    choose(s, index);
    s.setup.configure_after = true;
    s.setup.batch_message = format!(
        "Installing {}. {} components remain.",
        s.setup.recipes[index].name,
        batch.remaining.len()
    );
    s.setup.batch = Some(batch);
    request(s, "start");
    true
}
/// Stops the queue; the running job, if any, is asked to stop at its next safe step.
pub(crate) fn cancel(s: &mut State) {
    if let Some(batch) = s.setup.batch.take() {
        if let Some(id) = batch.waiting.filter(|id| !id.starts_with("starting:")) {
            s.setup.selected_job = Some(id);
            request(s, "cancel");
        }
        s.setup.batch_message="Workflow queue cancelled. The active transaction finishes its safe step; completed tools are preserved.".into();
        activity::record(s, "Cancelled the workflow installation queue");
    }
}

/// The running queue: what is installing, step progress, what remains and the job's log.
pub(crate) fn progress(s: &State, compact: bool) -> El {
    let r = &s.setup;
    let Some(batch) = &r.batch else {
        return el();
    };
    let job = batch
        .waiting()
        .and_then(|id| r.jobs.iter().find(|j| j.id == id));
    let mut block = col()
        .key("workflow-progress")
        .w_full()
        .min_w(0.)
        .gap(12.)
        .pt(18.)
        .border_sides([1., 0., 0., 0.], fg())
        .child(
            meta_sized(r.batch_message.clone(), 10., fg())
                .wrap()
                .role("status"),
        );
    match job {
        Some(job) => {
            let fraction = if job.total > 0 {
                (job.step as f32 / job.total as f32).clamp(0., 1.)
            } else {
                0.
            };
            block = block
                .child(
                    row()
                        .w_full()
                        .min_w(0.)
                        .gaps(6., 24.)
                        .center_y()
                        .flex_wrap()
                        .child(h3(&job.tool, 19.).grow(1.))
                        .child(meta_sized(job.status.clone(), 10., fg()))
                        .child(meta(format!("Step {}/{}", job.step, job.total))),
                )
                .child(
                    row()
                        .key("workflow-progress-bar")
                        .role("progressbar")
                        .label("Installation step progress")
                        .a11y_value(format!("{}/{}", job.step, job.total))
                        .w_full()
                        .h(5.)
                        .fill(soft())
                        .child(el().w_frac(fraction).h(5.).fill(fg())),
                )
                .child(p(&job.message, 12.))
                .child(super::view::job_log(job, if compact { 180. } else { 240. }));
        }
        None => {
            block = block.child(p("Starting the next installation job…", 12.));
        }
    }
    let names: Vec<_> = batch
        .remaining()
        .iter()
        .map(|i| r.recipes[*i].name.as_str())
        .collect();
    block
        .child(
            meta(if names.is_empty() {
                "Nothing else queued".to_string()
            } else {
                format!("Then: {}", names.join(" / "))
            })
            .wrap(),
        )
        .child(ghost(
            "cancel-workflow-setup",
            "Cancel after current step",
            !r.pending,
            cancel,
        ))
}

/// Headless fixture: a queue part-way through installing the preset's missing tools.
pub(crate) fn preview(s: &mut State) {
    let ids = workflow::ids(s);
    let indices: Vec<usize> = ids
        .iter()
        .filter_map(|id| s.setup.recipe_index(id))
        .collect();
    let Some((&first, rest)) = indices.split_first() else {
        return;
    };
    let id = "20260101T120000-0123abcd".to_string();
    s.setup.jobs = vec![Job {
        id: id.clone(),
        tool: s.setup.recipes[first].name.clone(),
        status: "Installing".into(),
        message: "Headless preview fixture: no installer runs.".into(),
        step: 3,
        total: 7,
        alive: true,
        log_tail: "[preview] fixture log; no command was executed".into(),
        ..Default::default()
    }];
    s.setup.selected_job = Some(id.clone());
    s.setup.batch = Some(Batch {
        remaining: rest.to_vec(),
        waiting: Some(id),
        distro: s.setup.distro.text.clone(),
    });
    s.setup.batch_message = format!(
        "Installing {}. {} components remain.",
        s.setup.recipes[first].name,
        rest.len()
    );
}

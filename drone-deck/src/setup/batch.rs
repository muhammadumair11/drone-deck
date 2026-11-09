use super::*;
use crate::ui::*;
#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Batch {
    remaining: Vec<usize>,
    waiting: Option<String>,
    distro: String,
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
pub(super) fn begin(s: &mut State) {
    if s.headless {
        s.setup.message = "Preview: workflow installation disabled.".into();
        return;
    }
    if s.busy()
        || s.setup.pending
        || !s.wsl_ready()
        || s.setup.distro.text.trim() != s.config.distro
    {
        s.setup.message = "Finish active work and select a working Linux environment first.".into();
        return;
    }
    let ids = workflow::PRESETS[s.setup.workflow].1;
    if !ids.iter().all(|id| {
        s.setup
            .checks
            .get(*id)
            .is_some_and(|c| c.environment.is_empty() || c.environment == s.setup.distro.text)
    }) {
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
    s.setup.batch_review = false;
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
fn cancel(s: &mut State) {
    if let Some(batch) = s.setup.batch.take() {
        if let Some(id) = batch.waiting {
            s.setup.selected_job = Some(id);
            request(s, "cancel");
        }
        s.setup.batch_message="Workflow queue cancelled. The active transaction finishes its safe step; completed tools are preserved.".into();
    }
}
pub(super) fn view(s: &State) -> El {
    let mut body = col().w_full().min_w(0.).gap(10.);
    if !s.setup.batch_message.is_empty() {
        body = body.child(label(&s.setup.batch_message, 13., ink()).wrap());
    }
    if s.setup.batch.is_some() {
        return body.child(
            button(
                "cancel-workflow-setup",
                "Cancel workflow after current step",
                false,
                cancel,
            )
            .disabled(s.setup.pending),
        );
    }
    let ids = workflow::PRESETS[s.setup.workflow].1;
    body = body.child(
        button(
            "review-workflow-setup",
            "Review complete workflow setup",
            true,
            |s| s.setup.batch_review = true,
        )
        .disabled(
            !ids.iter().all(|id| s.setup.checks.contains_key(*id)) || s.setup.pending || s.busy(),
        ),
    );
    if s.setup.batch_review {
        let mut review=card().key("workflow-install-review").child(heading("Review complete setup","REUSE / INSTALL / CONFIGURE"))
   .child(label("Verified existing components will be reused and their launch paths applied. Missing/incompatible tools get managed installations; shared prerequisites may be installed. Jobs run sequentially. This does not start a simulator, download a model, or fly a vehicle.",13.,ink()).wrap());
        for id in ids {
            let recipe = s.setup.recipes.iter().find(|r| r.id == *id).unwrap();
            let compatible = s.setup.checks.get(*id).is_some_and(|c| c.compatible);
            review = review.child(
                label(
                    format!(
                        "{} · {}\n{}\n{}",
                        recipe.name,
                        if compatible {
                            "reuse verified installation"
                        } else {
                            "install reviewed recipe"
                        },
                        recipe.destination,
                        recipe.privileges
                    ),
                    13.,
                    ink(),
                )
                .wrap(),
            );
        }
        body = body.child(
            review
                .child(button(
                    "confirm-workflow-setup",
                    "Set up this reviewed workflow",
                    true,
                    begin,
                ))
                .child(button("back-workflow-setup", "Back", false, |s| {
                    s.setup.batch_review = false
                })),
        );
    }
    body
}

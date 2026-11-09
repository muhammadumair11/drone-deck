use super::*;
use crate::ui::*;
fn action(key: &str, label: &str, command: &'static str, s: &State) -> El {
    button(key, label, true, move |s| request(s, command)).disabled(s.studio.pending)
}
pub(crate) fn settings(s: &State) -> El {
    card().key("huggingface-settings").child(heading("Hugging Face","WINDOWS CREDENTIAL MANAGER"))
 .child(label(if s.studio.stored{"An access token is stored for this Windows user."}else{"Check status or save a read/fine-grained access token. Public model downloads also work without one."},13.,muted()).wrap())
 .child(text_input(&s.studio.token,TextStyle::new(13.,400.)).key("hf-token").secure().single_line().on_edit(|s:&mut State|&mut s.studio.token).w_full().h(40.).padding(8.).fill(surface()).border(1.,line()))
 .child(row().gap(8.).flex_wrap().child(action("hf-status","Check saved token","credential_status",s)).child(action("hf-save","Save token","save_token",s)).child(action("hf-validate","Validate access","validate_token",s)).child(action("hf-forget","Remove saved token","forget_token",s)))
 .child(label("Removing the saved token does not revoke it on Hugging Face. Manage access and license requests on the model's official page.",12.,muted()).wrap())
 .child(label(&s.studio.message,13.,ink()).wrap())
}
pub(crate) fn page(s: &State) -> El {
    let r = &s.studio;
    let mut tabs = row().gap(8.).flex_wrap();
    for (index, name) in ["Models", "Datasets & evaluation", "Projects"]
        .iter()
        .enumerate()
    {
        tabs = tabs.child(button(
            format!("studio-tab-{index}"),
            *name,
            r.tab == index,
            move |s| s.studio.tab = index,
        ));
    }
    col()
        .w_full()
        .min_w(0.)
        .gap(16.)
        .child(
            card()
                .child(heading(
                    "Autonomy workspace",
                    "MODELS / DATA / REPRODUCIBLE RUNS",
                ))
                .child(tabs)
                .child(label(&r.message, 13., ink()).wrap()),
        )
        .child(match r.tab {
            0 => models(s),
            1 => datasets(s),
            _ => projects(s),
        })
}
fn models(s: &State) -> El {
    let r = &s.studio;
    let mut body = col().w_full().min_w(0.).gap(16.).child(
        card()
            .child(heading(
                "Download a model revision",
                "REVIEW BEFORE DOWNLOADING",
            ))
            .child(label("Repository · organization/model", 12., muted()))
            .child(field(&r.repo, "model-repo").on_edit(|s: &mut State| {
                s.studio.preview = None;
                &mut s.studio.repo
            }))
            .child(label("Revision · tag, branch, or commit", 12., muted()))
            .child(
                field(&r.revision, "model-revision").on_edit(|s: &mut State| {
                    s.studio.preview = None;
                    &mut s.studio.revision
                }),
            )
            .child(label(
                "Selected file patterns · comma separated",
                12.,
                muted(),
            ))
            .child(
                field(&r.patterns, "model-patterns").on_edit(|s: &mut State| {
                    s.studio.preview = None;
                    &mut s.studio.patterns
                }),
            )
            .child(
                row()
                    .gap(8.)
                    .flex_wrap()
                    .child(action(
                        "inspect-model",
                        "Inspect revision and size",
                        "inspect_model",
                        s,
                    ))
                    .child(button("model-settings", "Token settings", false, |s| {
                        s.navigate(6)
                    })),
            ),
    );
    if let Some(preview) = &r.preview {
        let files = preview["files"].as_array().cloned().unwrap_or_default();
        let mut review = card()
            .key("model-review")
            .child(heading(
                "Review model download",
                "IMMUTABLE REVISION / VERIFIED FILES",
            ))
            .child(
                label(
                    format!(
                        "{}\nCommit: {}\nLicense: {} · Gated: {}\n{} files · {:.1} MiB\nCache: {}",
                        preview["repo"].as_str().unwrap_or(""),
                        preview["sha"].as_str().unwrap_or(""),
                        preview["license"].as_str().unwrap_or("Not specified"),
                        preview["gated"],
                        files.len(),
                        preview["bytes"].as_f64().unwrap_or(0.) / 1048576.,
                        preview["destination"].as_str().unwrap_or("")
                    ),
                    13.,
                    ink(),
                )
                .wrap_anywhere(),
            );
        for file in files.iter().take(30) {
            review = review.child(
                label(
                    format!(
                        "{} · {} bytes",
                        file["name"].as_str().unwrap_or(""),
                        file["size"]
                    ),
                    12.,
                    muted(),
                )
                .wrap(),
            );
        }
        body=body.child(review.child(label("Review the model's license/access terms on Hugging Face. Files are downloaded and verified; repository code is not executed. An inference runtime and task-specific inputs are separate.",13.,muted()).wrap()).child(action("download-reviewed-model","Download reviewed files","start_model",s)));
    }
    let mut jobs = card().child(heading("Model downloads", "PERSISTENT PROGRESS / RETRY"));
    for job in &r.jobs {
        let id = job["id"].as_str().unwrap_or("").to_string();
        jobs = jobs.child(button(
            format!("model-job-{id}"),
            &format!(
                "{} · {} · {} / {} bytes",
                job["repo"].as_str().unwrap_or(""),
                job["status"].as_str().unwrap_or(""),
                job["progress"]["bytes"].as_u64().unwrap_or(0),
                job["progress"]["total"].as_u64().unwrap_or(0)
            ),
            r.selected_job.as_ref() == Some(&id),
            move |s| s.studio.selected_job = Some(id.clone()),
        ));
        if r.selected_job.as_deref() == job["id"].as_str() {
            jobs = jobs
                .child(label(job["message"].as_str().unwrap_or(""), 12., muted()).wrap())
                .child(
                    row()
                        .gap(8.)
                        .flex_wrap()
                        .child(action("cancel-model", "Cancel download", "cancel_model", s))
                        .child(action(
                            "retry-model",
                            "Retry pinned download",
                            "retry_model",
                            s,
                        ))
                        .disabled(r.pending),
                );
        }
    }
    let mut library = card()
        .child(heading("Local model library", "OFFLINE ASSETS"))
        .child(
            row()
                .gap(8.)
                .flex_wrap()
                .child(action(
                    "refresh-models",
                    "Refresh local models",
                    "models",
                    s,
                ))
                .child(action(
                    "verify-model",
                    "Verify selected model offline",
                    "verify_model",
                    s,
                )),
        );
    for model in &r.models {
        let path = model["path"].as_str().unwrap_or("").to_string();
        library = library.child(button(
            format!(
                "model-{}-{}",
                model["repo"].as_str().unwrap_or(""),
                model["sha"].as_str().unwrap_or("")
            ),
            &format!(
                "{} · {}",
                model["repo"].as_str().unwrap_or(""),
                model["sha"].as_str().unwrap_or("")
            ),
            r.model_path == path,
            move |s| s.studio.model_path = path.clone(),
        ));
    }
    body.child(jobs).child(library)
}
fn datasets(s: &State) -> El {
    let r = &s.studio;
    let mut body = col().w_full().min_w(0.).gap(16.).child(
        card()
            .child(heading("Recording datasets", "IMMUTABLE PROVENANCE"))
            .child(action(
                "refresh-data",
                "Load recordings, datasets and reports",
                "data_library",
                s,
            ))
            .child(label("Dataset name", 12., muted()))
            .child(field(&r.name, "dataset-name").on_edit(|s: &mut State| &mut s.studio.name))
            .child(label(
                "Finalized recording session identifier",
                12.,
                muted(),
            ))
            .child(
                field(&r.session, "dataset-session").on_edit(|s: &mut State| &mut s.studio.session),
            )
            .child(action(
                "create-dataset",
                "Create version from recording",
                "dataset_create",
                s,
            )),
    );
    if let Some(sessions) = r.library["sessions"].as_array() {
        for session in sessions {
            let id = session["id"].as_str().unwrap_or("").to_string();
            body = body.child(button(
                format!("source-{id}"),
                &format!("Use recording: {id}"),
                r.session.text == id,
                move |s| s.studio.session = TextEdit::new(&id),
            ));
        }
    }
    if let Some(datasets) = r.library["datasets"].as_array() {
        for dataset in datasets {
            let selected = dataset.clone();
            body = body.child(button(
                format!("dataset-{}", dataset["version"].as_str().unwrap_or("")),
                &format!(
                    "{} · {}",
                    dataset["name"].as_str().unwrap_or(""),
                    dataset["version"].as_str().unwrap_or("")
                ),
                r.dataset
                    .as_ref()
                    .is_some_and(|d| d["version"] == dataset["version"]),
                move |s| s.studio.dataset = Some(selected.clone()),
            ));
        }
    }
    body=body.child(card().child(heading("Evaluate supplied predictions","OFFLINE / NO FLIGHT COMMANDS"))
  .child(label("CSV inside WSL: sample_id,target,prediction; optional latency_ms. Scores use your exported ground truth and predictions; this does not infer camera labels automatically.",13.,muted()).wrap())
  .child(field(&r.csv,"evaluation-csv").on_edit(|s:&mut State|&mut s.studio.csv))
  .child(row().gap(8.).flex_wrap().child(button("task-classification","Classification",r.task.text=="classification",|s|s.studio.task=TextEdit::new("classification"))).child(button("task-regression","Regression",r.task.text=="regression",|s|s.studio.task=TextEdit::new("regression"))))
  .child(row().gap(8.).flex_wrap().child(action("verify-dataset","Verify dataset offline","dataset_verify",s)).child(action("run-evaluation","Evaluate CSV and save report","evaluate",s))));
    if let Some(reports) = r.library["reports"].as_array() {
        for report in reports {
            body = body.child(
                card()
                    .child(title(
                        format!(
                            "{} · {} samples",
                            report["task"].as_str().unwrap_or(""),
                            report["samples"]
                        ),
                        14.,
                    ))
                    .child(
                        mono(
                            serde_json::to_string_pretty(&report["metrics"]).unwrap_or_default(),
                            12.,
                            ink(),
                        )
                        .wrap(),
                    )
                    .child(label(report["manifest"].as_str().unwrap_or(""), 11., muted()).wrap()),
            );
        }
    }
    body
}
pub(crate) fn projects(s: &State) -> El {
    let r = &s.studio;
    let mut choices = row().gap(8.).flex_wrap();
    for (index, label) in [
        "ONNX inference",
        "ROS 2 subscriber",
        "MAVSDK monitor",
        "PyTorch CPU training",
    ]
    .iter()
    .enumerate()
    {
        choices = choices.child(button(
            format!("project-kind-{index}"),
            *label,
            r.kind == index,
            move |s| s.studio.kind = index,
        ));
    }
    let mut body=card().key("project-workspace").child(heading("Create an autonomy project","WSL / USER-OWNED SOURCE"))
 .child(field(&r.project_name,"project-name").on_edit(|s:&mut State|&mut s.studio.project_name)).child(choices)
 .child(label(format!("Selected model: {}\nSelected dataset: {}",if r.model_path.is_empty(){"None"}else{&r.model_path},r.dataset.as_ref().and_then(|d|d["name"].as_str()).unwrap_or("None")),12.,muted()).wrap_anywhere())
 .child(label("Creates editable source and a manifest without installing or starting code. ONNX uses prepared NPZ tensors; CPU training uses features/targets and exports ONNX. ROS and MAVSDK starters only monitor. These are starting points for your own autonomy code.",13.,muted()).wrap())
 .child(row().gap(8.).flex_wrap().child(action("create-project","Create project","project_create",s)).child(action("refresh-projects","Refresh projects","data_library",s)).child(button("project-setup","Install project prerequisites",false,|s|s.navigate(8))));
    body = body
        .child(label(
            "Run inputs: Linux model path for ONNX; NPZ input path for inference or CPU training",
            12.,
            muted(),
        ))
        .child(
            field(&r.model_file, "project-model-file")
                .on_edit(|s: &mut State| &mut s.studio.model_file),
        )
        .child(field(&r.inputs, "project-inputs").on_edit(|s: &mut State| &mut s.studio.inputs));
    if let Some(projects) = r.library["projects"].as_array() {
        for project in projects {
            let selected = project.clone();
            let id = format!("project-{}", project["name"].as_str().unwrap_or(""));
            let running = s
                .status
                .get(&id)
                .is_some_and(|v| v == "Running" || v == "Starting");
            body = body
                .child(
                    label(
                        format!(
                            "{} · {}\n{}",
                            project["name"].as_str().unwrap_or(""),
                            project["kind"].as_str().unwrap_or(""),
                            project["path"].as_str().unwrap_or("")
                        ),
                        13.,
                        ink(),
                    )
                    .wrap_anywhere(),
                )
                .child(button(
                    format!("run-{id}"),
                    if running {
                        "Stop project"
                    } else {
                        "Run project"
                    },
                    true,
                    move |s| super::projects::run(s, selected.clone()),
                ));
        }
    }
    body
}

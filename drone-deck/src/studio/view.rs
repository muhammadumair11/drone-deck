//! Data & models tabs backed by Studio: models, datasets & evaluation, projects, plus the
//! Hugging Face access block that Settings also embeds.
use super::*;
use crate::ui::{Page, kit::*, theme::*};
mod library;
mod runs;

/// Sends a Studio request and records it in Activity once the request is in flight.
pub(super) fn act(s: &mut State, command: &str, record: impl Into<String>) {
    request(s, command);
    if s.studio.pending {
        activity::record(s, record);
    }
}

/// A selectable list row: the whole row is one button.
pub(super) fn choose(row: El, name: &str, selected: bool, on: impl Fn(&mut State) + 'static) -> El {
    row.role("button")
        .label(name)
        .focus_selected(selected)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .fill(if selected {
            soft()
        } else {
            rune::Color::TRANSPARENT
        })
        .hover(soft())
        .focus_ring(2., 0., fg())
        .on_press(on)
}

/// The pull column of a selectable row.
pub(super) fn pick_label(selected: bool) -> El {
    meta_sized(
        if selected { "Selected" } else { "Select" },
        10.,
        if selected { fg() } else { muted() },
    )
}

/// The lead column of a log row; 9px on compact so words like "Downloading" fit its 56px.
pub(super) fn lead(t: impl Into<String>, tone: rune::Color, compact: bool) -> El {
    meta_sized(t, if compact { 9. } else { 10. }, tone).wrap()
}

/// A mono label / value pair for manifests and reviews.
pub(super) fn kv(label: &str, value: impl Into<String>, compact: bool) -> El {
    let value = code(value, 12., fg()).wrap_anywhere().selectable(true);
    if compact {
        return col()
            .w_full()
            .min_w(0.)
            .gap(2.)
            .child(meta(label))
            .child(value);
    }
    row()
        .w_full()
        .min_w(0.)
        .gap(16.)
        .items_start()
        .child(meta(label).w(120.).pt(2.))
        .child(col().grow(1.).min_w(0.).child(value))
}

/// The shared Studio status line (`#feedback` in the design).
pub(super) fn status(s: &State) -> El {
    let r = &s.studio;
    row()
        .key("studio-status")
        .role("status")
        .label(r.message.clone())
        .w_full()
        .min_w(0.)
        .gap(14.)
        .py(10.)
        .items_start()
        .child(
            meta_sized(
                if r.pending { "Working" } else { "Status" },
                9.,
                if r.pending { fg() } else { muted() },
            )
            .w(64.)
            .pt(4.),
        )
        .child(body(r.message.clone(), 11., fg()).grow(1.))
}

/// The Data & models tabs Studio owns (`ui.data_tab` 1–3).
pub(crate) fn page(s: &State, vp: &Viewport) -> El {
    let content = match s.ui.data_tab {
        1 => models(s, vp),
        2 => library::datasets(s, vp),
        _ => runs::projects(s, vp),
    };
    col()
        .key("studio-tab")
        .w_full()
        .min_w(0.)
        .child(status(s))
        .child(content)
}

/// First visit of a tab (or the AI system overview) requests what it lists, one request at a time (Studio has one lock).
pub(super) fn lazy_load(s: &mut State) -> bool {
    if s.headless || s.studio.pending {
        return false;
    }
    // The System page shows project and artifact counts for the offline AI system.
    let on_system = s.page == Page::System && s.systems.active == crate::systems::AI;
    let wanted: &[&'static str] = match (s.page, s.ui.data_tab) {
        (Page::Data, 1) => &["models", "credential_status"],
        (Page::Data, 2 | 3) => &["data_library"],
        _ if on_system => &["data_library"],
        _ => &[],
    };
    for &action in wanted {
        if s.studio.loaded.contains(&action) {
            continue;
        }
        if action == "data_library" && !s.wsl_ready() {
            return false;
        }
        s.studio.loaded.push(action);
        request(s, action);
        return true;
    }
    false
}

/// Token actions all report whether a token is stored, so any of them counts as a status check.
fn credential(s: &mut State, action: &str, record: bool) {
    request(s, action);
    if !s.studio.pending {
        return;
    }
    if !s.studio.loaded.contains(&"credential_status") {
        s.studio.loaded.push("credential_status");
    }
    match (record, action) {
        (true, "save_token") => {
            activity::record(s, "Saved a Hugging Face token to Credential Manager")
        }
        (true, _) => activity::record(s, "Removed the saved Hugging Face token"),
        _ => {}
    }
}

fn hf_access(s: &State, with_status: bool) -> El {
    let r = &s.studio;
    let checked = r.loaded.contains(&"credential_status");
    let state = if r.stored {
        "A token is stored for this Windows user in Credential Manager."
    } else if checked && !r.pending {
        "No token is stored. Public models download without one; gated models need a read or fine-grained token."
    } else {
        "Token status not checked yet. Public models download without a token."
    };
    let enabled = !r.pending;
    let mut block = col()
        .key("huggingface-settings")
        .w_full()
        .min_w(0.)
        .gap(12.)
        .child(
            row()
                .w_full()
                .min_w(0.)
                .gap(10.)
                .center_y()
                .child(dot(if r.stored { success() } else { muted() }))
                .child(meta(if r.stored {
                    "Token stored / Windows Credential Manager"
                } else {
                    "No token confirmed / Windows Credential Manager"
                })
                .wrap()
                .grow(1.)),
        )
        .child(p(state, 11.))
        .child(labeled(
            "Access token",
            field(&r.token, "hf-token", "hf_…")
                .secure()
                .on_edit(|s: &mut State| &mut s.studio.token),
        ))
        .child(
            row()
                .gaps(4., 16.)
                .flex_wrap()
                .center_y()
                .child(pill(
                    "hf-save",
                    "Save token",
                    enabled && !r.token.text.trim().is_empty(),
                    |s| credential(s, "save_token", true),
                ))
                .child(ghost("hf-status", "Check saved token", enabled, |s| {
                    credential(s, "credential_status", false)
                }))
                .child(ghost("hf-validate", "Validate access", enabled, |s| {
                    credential(s, "validate_token", false)
                }))
                .child(ghost("hf-forget", "Remove saved token", enabled && r.stored, |s| {
                    credential(s, "forget_token", true)
                })),
        )
        .child(notice(
            "Removing the saved token does not revoke it on Hugging Face. Manage access and license requests on the model's official page.",
        ));
    if with_status {
        block = block.child(status(s));
    }
    block
}

/// Hugging Face access: token storage in Windows Credential Manager.
pub(crate) fn settings(s: &State) -> El {
    hf_access(s, true)
}

fn repo_valid(t: &str) -> bool {
    let part = |p: Option<&str>| {
        p.is_some_and(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        })
    };
    let mut parts = t.split('/');
    part(parts.next()) && part(parts.next()) && parts.next().is_none()
}

fn mib(bytes: f64) -> String {
    format!("{:.1} MiB", bytes / 1048576.)
}

/// A file size in the unit that keeps small files from reading as 0.0 MiB.
fn size(bytes: f64) -> String {
    if bytes >= 1048576. {
        mib(bytes)
    } else if bytes >= 1024. {
        format!("{:.1} KiB", bytes / 1024.)
    } else {
        format!("{bytes:.0} bytes")
    }
}

fn review(s: &State, preview: &Value, compact: bool) -> El {
    let files = preview["files"].as_array().cloned().unwrap_or_default();
    let mut list = col().w_full().min_w(0.).gap(2.);
    for file in files.iter().take(30) {
        let name = file["name"].as_str().unwrap_or("");
        let line = match file["size"].as_f64() {
            Some(bytes) => format!("{name} · {}", size(bytes)),
            None => format!("{name} · size not reported"),
        };
        list = list.child(code(line, 11., muted()).wrap_anywhere());
    }
    if files.len() > 30 {
        list = list.child(meta(format!("+ {} more files", files.len() - 30)));
    }
    let repo = preview["repo"].as_str().unwrap_or("").to_string();
    let sha = preview["sha"].as_str().unwrap_or("").to_string();
    col()
        .key("model-review")
        .w_full()
        .min_w(0.)
        .gap(10.)
        .mt(8.)
        .pt(18.)
        .border_sides([1., 0., 0., 0.], rule())
        .child(meta("Review before download / immutable revision"))
        .child(h3("Review model download", 16.))
        .child(kv("Repository", repo.clone(), compact))
        .child(kv("Commit", sha.clone(), compact))
        .child(kv(
            "License",
            preview["license"].as_str().unwrap_or("Not specified"),
            compact,
        ))
        .child(kv(
            "Gated",
            match &preview["gated"] {
                Value::Bool(false) => "No".to_string(),
                Value::Bool(true) => "Yes / request access on Hugging Face".to_string(),
                Value::String(t) => format!("Yes / {t}"),
                _ => "Not reported".to_string(),
            },
            compact,
        ))
        .child(kv(
            "Selected files",
            format!(
                "{} files / {}",
                files.len(),
                mib(preview["bytes"].as_f64().unwrap_or(0.))
            ),
            compact,
        ))
        .child(kv(
            "Destination",
            preview["destination"].as_str().unwrap_or(""),
            compact,
        ))
        .child(list)
        .child(notice(
            "Review the model's license and access terms on Hugging Face. Files are downloaded and hash-verified; repository code is not executed. An inference runtime and task-specific inputs are separate requirements.",
        ))
        .child(row().child(pill(
            "download-reviewed-model",
            "Download reviewed files",
            !s.studio.pending,
            move |s| {
                let short: String = sha.chars().take(12).collect();
                act(
                    s,
                    "start_model",
                    format!("Started model download {repo} @ {short}"),
                )
            },
        )))
}

fn local_library(s: &State) -> El {
    let r = &s.studio;
    let mut list = col().key("model-library").w_full().min_w(0.);
    for model in &r.models {
        let repo = model["repo"].as_str().unwrap_or("").to_string();
        let sha = model["sha"].as_str().unwrap_or("").to_string();
        let path = model["path"].as_str().unwrap_or("").to_string();
        let selected = !path.is_empty() && r.model_path == path;
        let short: String = sha.chars().take(12).collect();
        let entry = ruled(false)
            .key(format!("model-{repo}-{sha}"))
            .gap(12.)
            .py(12.)
            .px(4.)
            .items_start()
            .child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .gap(4.)
                    .child(code(repo.clone(), 12., fg()).wrap_anywhere())
                    .child(
                        row()
                            .gap(8.)
                            .center_y()
                            .child(meta("Revision"))
                            .child(code(short, 11., muted()).help(sha.clone())),
                    ),
            )
            .child(pick_label(selected));
        list = list.child(choose(entry, &repo, selected, move |s| {
            s.studio.model_path = path.clone()
        }));
    }
    if r.models.is_empty() {
        let (title, text) = if r.loaded.contains(&"models") && !r.pending {
            (
                "No model assets",
                "Inspect and download a pinned revision to add one. Use the offline AI system for ONNX inference: prepared NPZ tensors and a compatible runtime are also required.",
            )
        } else {
            (
                "Library not loaded",
                "Refresh to read downloaded models from this computer. Nothing is sent to Hugging Face.",
            )
        };
        list = list.child(empty("models-empty", title, text).py(16.));
    }
    let can_verify = !r.pending
        && r.models
            .iter()
            .any(|m| m["path"].as_str() == Some(&r.model_path));
    col()
        .w_full()
        .min_w(0.)
        .gap(8.)
        .child(
            row()
                .w_full()
                .gap(12.)
                .center_y()
                .justify_between()
                .child(meta("Local library"))
                .child(meta(format!("{} assets", r.models.len()))),
        )
        .child(list)
        .child(
            row()
                .gaps(4., 20.)
                .flex_wrap()
                .child(ghost(
                    "refresh-models",
                    "Refresh library",
                    !r.pending,
                    |s| request(s, "models"),
                ))
                .child(ghost(
                    "verify-model",
                    "Verify selected offline",
                    can_verify,
                    |s| {
                        act(
                            s,
                            "verify_model",
                            "Requested offline verification of a local model",
                        )
                    },
                )),
        )
}

fn downloads(s: &State, vp: &Viewport) -> El {
    let r = &s.studio;
    let mut list = col().key("model-downloads").w_full().min_w(0.);
    for job in &r.jobs {
        let id = job["id"].as_str().unwrap_or("").to_string();
        let selected = r.selected_job.as_deref() == Some(id.as_str());
        let status = job["status"].as_str().unwrap_or("Unknown").to_string();
        let alive = job["alive"].as_bool().unwrap_or(false);
        let tone = match status.as_str() {
            "Ready" => success(),
            "Failed" | "Interrupted" => danger(),
            "Cancelled" => warn(),
            _ if alive => fg(),
            _ => muted(),
        };
        let repo = job["repo"].as_str().unwrap_or("").to_string();
        let done = mib(job["progress"]["bytes"].as_f64().unwrap_or(0.));
        let mut detail = match job["progress"]["total"].as_f64() {
            Some(total) if total > 0. => format!("{done} of {}", mib(total)),
            _ => format!("{done} downloaded"),
        };
        if let Some(message) = job["message"].as_str().filter(|m| !m.is_empty()) {
            detail = format!("{detail} / {message}");
        }
        let main = col()
            .w_full()
            .min_w(0.)
            .gap(4.)
            .child(code(repo.clone(), 13., fg()).wrap_anywhere())
            .child(p(detail, 11.));
        let entry = log_row(
            format!("model-job-{id}"),
            lead(status.clone(), tone, vp.compact()),
            main,
            pick_label(selected),
            vp.compact(),
        );
        let pick = id.clone();
        list = list.child(choose(entry, &repo, selected, move |s| {
            s.studio.selected_job = Some(pick.clone())
        }));
        if selected {
            let label = repo.clone();
            list = list.child(
                row()
                    .key("model-job-actions")
                    .w_full()
                    .gaps(4., 20.)
                    .flex_wrap()
                    .pb(12.)
                    .pl(if vp.compact() { 70. } else { 124. })
                    .child(ghost(
                        "cancel-model",
                        "Cancel download",
                        alive && !r.pending,
                        {
                            let label = label.clone();
                            move |s| {
                                act(
                                    s,
                                    "cancel_model",
                                    format!("Requested cancel of model download {label}"),
                                )
                            }
                        },
                    ))
                    .child(ghost(
                        "retry-model",
                        "Retry pinned download",
                        !alive && status != "Ready" && !r.pending,
                        move |s| act(s, "retry_model", format!("Retried model download {label}")),
                    )),
            );
        }
    }
    if r.jobs.is_empty() {
        list = list.child(empty(
            "downloads-empty",
            "No model downloads yet",
            "Inspect a repository revision, review its files and license, then download. Progress persists if Drone Deck closes; interrupted downloads can be retried.",
        ));
    }
    col()
        .w_full()
        .min_w(0.)
        .pt(16.)
        .child(section_head(
            "Model downloads",
            meta("Persistent progress / hash verified"),
            vp.compact(),
        ))
        .child(list)
}

fn models(s: &State, vp: &Viewport) -> El {
    let r = &s.studio;
    let repo = r.repo.text.trim();
    let invalid = !repo.is_empty() && !repo_valid(repo);
    let mut repo_field = field(&r.repo, "model-repo", "organization/model")
        .invalid(invalid)
        .on_edit(|s: &mut State| {
            s.studio.preview = None;
            &mut s.studio.repo
        });
    if invalid {
        repo_field = repo_field.border(1., danger());
    }
    let mut left = col()
        .key("model-source")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(h2("Model assets are not runtimes.", 22.))
        .child(p(
            "Provide a Hub repository and inspect a pinned revision before a managed download. Hash verification and license or gated access are separate checks.",
            12.,
        ))
        .child(labeled("Repository ID", repo_field));
    if invalid {
        left = left.child(body("Use organization/model format.", 11., danger()));
    }
    left = left
        .child(labeled(
            "Revision / tag, branch or commit",
            field(&r.revision, "model-revision", "main").on_edit(|s: &mut State| {
                s.studio.preview = None;
                &mut s.studio.revision
            }),
        ))
        .child(labeled(
            "File patterns / comma separated",
            field(&r.patterns, "model-patterns", "*.onnx,*.json").on_edit(|s: &mut State| {
                s.studio.preview = None;
                &mut s.studio.patterns
            }),
        ))
        .child(row().child(pill(
            "inspect-model",
            "Inspect revision",
            !r.pending && !repo.is_empty(),
            |s| {
                if !repo_valid(s.studio.repo.text.trim()) {
                    s.studio.message = "Use organization/model format before inspecting.".into();
                    return;
                }
                request(s, "inspect_model")
            },
        )));
    if let Some(preview) = &r.preview {
        left = left.child(review(s, preview, vp.compact()));
    }
    let right = col()
        .w_full()
        .min_w(0.)
        .gap(16.)
        .child(local_library(s))
        .child(disclosure(s, "hf-access", "Hugging Face access", || {
            hf_access(s, false)
        }));
    col()
        .w_full()
        .min_w(0.)
        .child(asset(left, right, vp.compact()))
        .child(downloads(s, vp))
}

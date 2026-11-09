//! Datasets & evaluation: dataset versions from finalized recordings, offline evaluation of
//! supplied predictions, and the saved reports.
use super::*;

fn short(v: &str) -> String {
    v.chars().take(12).collect()
}

/// Scalar metrics as `name value`; nested values (confusion matrices) are named, not invented.
fn metrics_summary(metrics: &Value) -> String {
    let Some(map) = metrics.as_object() else {
        return "No metrics recorded".into();
    };
    let parts: Vec<String> = map
        .iter()
        .map(|(k, v)| match v {
            Value::Number(n) => match n.as_f64() {
                Some(f) if f.fract() != 0. => format!("{k} {f:.4}"),
                _ => format!("{k} {n}"),
            },
            Value::String(t) => format!("{k} {t}"),
            Value::Object(_) | Value::Array(_) => format!("{k} (see report)"),
            other => format!("{k} {other}"),
        })
        .collect();
    if parts.is_empty() {
        "No metrics recorded".into()
    } else {
        parts.join(" / ")
    }
}

fn wsl_note(s: &State) -> El {
    col()
        .w_full()
        .min_w(0.)
        .gap(8.)
        .child(
            row()
                .gap(10.)
                .center_y()
                .child(dot(warn()))
                .child(meta_sized("Linux environment not ready", 10., fg())),
        )
        .child(p(
            format!(
                "Datasets, evaluations and projects live inside the selected Ubuntu ({}). Select a working WSL 2 distribution first.",
                s.config.distro
            ),
            11.,
        ))
        .child(ghost("data-environment", "Environment settings ↗", true, |s| {
            s.navigate(Page::Settings)
        }))
}

fn readiness(s: &State) -> El {
    let r = &s.studio;
    let lib = &r.library;
    let count = |k: &str| lib[k].as_array().map(Vec::len);
    let mut right = col().w_full().min_w(0.).gap(10.).child(meta("Readiness"));
    if !s.wsl_ready() {
        return right.child(wsl_note(s));
    }
    let loaded = lib["sessions"].is_array();
    right = right
        .child(
            row()
                .gap(10.)
                .center_y()
                .child(dot(if loaded { success() } else { muted() }))
                .child(meta_sized(
                    if loaded {
                        format!("Library loaded / {}", s.config.distro)
                    } else if r.pending {
                        "Loading library".into()
                    } else {
                        "Library not loaded".into()
                    },
                    10.,
                    fg(),
                )),
        )
        .child(p(
            if loaded {
                format!(
                    "{} finalized recordings / {} dataset versions / {} reports.",
                    count("sessions").unwrap_or(0),
                    count("datasets").unwrap_or(0),
                    count("reports").unwrap_or(0)
                )
            } else {
                "Load the library to list finalized recordings, dataset versions and reports.".into()
            },
            11.,
        ))
        .child(p(
            "A dataset version references recordings by hash; it does not copy them. Removing an original recording breaks verification.",
            11.,
        ));
    let selected = r.dataset.as_ref().map(|d| {
        format!(
            "{} @ {}",
            d["name"].as_str().unwrap_or(""),
            short(d["version"].as_str().unwrap_or(""))
        )
    });
    right
        .child(kv(
            "Selected version",
            selected.unwrap_or_else(|| "None".into()),
            true,
        ))
        .child(ghost("refresh-data", "Reload library", !r.pending, |s| {
            request(s, "data_library")
        }))
}

fn sources(s: &State) -> El {
    let r = &s.studio;
    let mut list = col().key("dataset-sources").w_full().min_w(0.);
    let sessions = r.library["sessions"].as_array();
    for session in sessions.into_iter().flatten() {
        let id = session["id"].as_str().unwrap_or("").to_string();
        let selected = r.session.text == id;
        let label = session["label"].as_str().unwrap_or(&id).to_string();
        let entry = ruled(false)
            .key(format!("source-{id}"))
            .gap(12.)
            .py(10.)
            .px(4.)
            .items_start()
            .child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .gap(4.)
                    .child(h3(label.clone(), 13.))
                    .child(code(id.clone(), 11., muted()).wrap_anywhere()),
            )
            .child(pick_label(selected));
        list = list.child(choose(entry, &label, selected, move |s| {
            s.studio.session = TextEdit::new(&id)
        }));
    }
    if sessions.is_none_or(Vec::is_empty) {
        list = list.child(p(
            if sessions.is_some() {
                "No finalized recordings in this environment. Capture and finalize one under Recordings."
            } else {
                "Recordings appear here once the library is loaded."
            },
            11.,
        ));
    }
    list
}

fn versions(s: &State, vp: &Viewport) -> El {
    let r = &s.studio;
    let mut list = col().key("dataset-versions").w_full().min_w(0.);
    let datasets = r.library["datasets"].as_array();
    for dataset in datasets.into_iter().flatten() {
        let version = dataset["version"].as_str().unwrap_or("").to_string();
        let name = dataset["name"].as_str().unwrap_or("").to_string();
        let selected = r
            .dataset
            .as_ref()
            .is_some_and(|d| d["version"] == dataset["version"]);
        let artifacts = dataset["artifacts"].as_array();
        let bytes: f64 = artifacts
            .into_iter()
            .flatten()
            .filter_map(|a| a["bytes"].as_f64())
            .sum();
        let count = artifacts.map(Vec::len).unwrap_or(0);
        let detail = format!(
            "{count} {} / {}",
            if count == 1 { "artifact" } else { "artifacts" },
            mib(bytes)
        );
        let source = format!(
            "{} @ {}",
            dataset["session"].as_str().unwrap_or("unknown recording"),
            short(&version)
        );
        let main = col()
            .w_full()
            .min_w(0.)
            .gap(4.)
            .child(h3(name.clone(), 15.))
            .child(p(detail, 11.))
            .child(code(source, 11., fg()).wrap_anywhere())
            .child(code(dataset["manifest"].as_str().unwrap_or(""), 11., muted()).wrap_anywhere());
        let entry = log_row(
            format!("dataset-{version}"),
            meta("Dataset"),
            main,
            pick_label(selected),
            vp.compact(),
        );
        let value = dataset.clone();
        list = list.child(choose(entry, &name, selected, move |s| {
            s.studio.dataset = Some(value.clone())
        }));
    }
    if datasets.is_none_or(Vec::is_empty) {
        list = list.child(empty(
            "datasets-empty",
            "No dataset versions",
            "Create a version from a finalized recording above. Each version is immutable and verified by hash.",
        ));
    }
    col()
        .w_full()
        .min_w(0.)
        .pt(16.)
        .child(section_head(
            "Dataset versions",
            meta("Immutable / referenced, not copied"),
            vp.compact(),
        ))
        .child(list)
}

fn task_choice(key: &'static str, name: &'static str, value: &'static str, current: bool) -> El {
    row()
        .key(key)
        .role("tab")
        .label(name)
        .focusable(true)
        .focus_selected(current)
        .cursor(rune::Cursor::Pointer)
        .min_h(44.)
        .py(10.)
        .center_y()
        .border_sides(
            [0., 0., 1., 0.],
            if current {
                fg()
            } else {
                rune::Color::TRANSPARENT
            },
        )
        .hover(soft())
        .focus_ring(2., 4., fg())
        .on_press(move |s: &mut State| s.studio.task = TextEdit::new(value))
        .child(caps(name, 10., 500., 0.08, if current { fg() } else { muted() }).group_color(fg()))
}

fn evaluation(s: &State, vp: &Viewport) -> El {
    let r = &s.studio;
    let ready = s.wsl_ready() && !r.pending && r.dataset.is_some();
    let task = r.task.text.as_str();
    let left = col()
        .key("evaluation")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(h2("Evaluate supplied predictions", 19.))
        .child(p(
            "Scores compare your exported ground truth with your predictions. Nothing is inferred from camera data and no flight command is sent.",
            12.,
        ))
        .child(
            row()
                .key("evaluation-task")
                .role("tablist")
                .gaps(4., 28.)
                .flex_wrap()
                .child(task_choice(
                    "task-classification",
                    "Classification",
                    "classification",
                    task == "classification",
                ))
                .child(task_choice(
                    "task-regression",
                    "Regression",
                    "regression",
                    task == "regression",
                )),
        )
        .child(labeled(
            "Predictions CSV inside WSL",
            field(&r.csv, "evaluation-csv", "/home/user/predictions.csv")
                .on_edit(|s: &mut State| &mut s.studio.csv),
        ))
        .child(notice(
            "Columns: sample_id, target, prediction; optional latency_ms. Sample IDs must be unique.",
        ))
        .child(
            row()
                .gaps(8., 20.)
                .flex_wrap()
                .center_y()
                .child(pill(
                    "run-evaluation",
                    "Evaluate and save report",
                    ready && !r.csv.text.trim().is_empty(),
                    |s| {
                    act(s, "evaluate", "Requested evaluation of predictions against a dataset version")
                }))
                .child(ghost("verify-dataset", "Verify dataset offline", ready, |s| {
                    act(s, "dataset_verify", "Requested offline verification of a dataset version")
                })),
        );
    let right = col()
        .w_full()
        .min_w(0.)
        .gap(10.)
        .child(meta("Requirements"))
        .child(p(
            if r.dataset.is_some() {
                "A dataset version is selected. Verification re-hashes its source recordings before scoring."
            } else {
                "Select a dataset version first. Evaluation is tied to an immutable version and the predictions file hash."
            },
            11.,
        ))
        .child(p(
            "Classification reports accuracy and a confusion matrix. Regression reports MAE and RMSE. Latency statistics appear only when latency_ms is supplied.",
            11.,
        ));
    col()
        .w_full()
        .min_w(0.)
        .mt(16.)
        .border_sides([1., 0., 0., 0.], rule())
        .child(asset(left, right, vp.compact()))
}

fn reports(s: &State, vp: &Viewport) -> El {
    let r = &s.studio;
    let mut list = col().key("evaluation-reports").w_full().min_w(0.);
    let reports = r.library["reports"].as_array();
    for (i, report) in reports
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .rev()
    {
        let task = report["task"].as_str().unwrap_or("Evaluation");
        let title = match report["samples"].as_u64() {
            Some(n) => format!("{task} / {n} samples"),
            None => format!("{task} / samples not recorded"),
        };
        let dataset = format!(
            "{} @ {}",
            report["dataset"].as_str().unwrap_or(""),
            short(report["version"].as_str().unwrap_or(""))
        );
        let main = col()
            .w_full()
            .min_w(0.)
            .gap(4.)
            .child(h3(title, 15.))
            .child(p(metrics_summary(&report["metrics"]), 11.))
            .child(code(dataset, 11., fg()).wrap_anywhere())
            .child(code(report["manifest"].as_str().unwrap_or(""), 11., muted()).wrap_anywhere());
        let created = report["created"].as_str().unwrap_or("");
        let date = created
            .get(..10)
            .filter(|d| !d.is_empty())
            .unwrap_or("Undated");
        list = list.child(log_row(
            format!("report-{i}"),
            lead(date, muted(), vp.compact()),
            main,
            meta("Report"),
            vp.compact(),
        ));
    }
    if reports.is_none_or(Vec::is_empty) {
        list = list.child(
            col()
                .key("reports-empty")
                .w_full()
                .min_w(0.)
                .pb(24.)
                .child(
                    empty(
                        "no-run-artifacts",
                        "No run artifacts yet",
                        "Inference projects retain model and input hashes, runtime versions and output tensors. Evaluation requires a valid predictions CSV and supplied targets.",
                    )
                    .pb(4.),
                )
                .child(ghost("open-projects", "Inspect projects →", true, |s| {
                    s.ui.data_tab = 3
                })),
        );
    }
    col()
        .w_full()
        .min_w(0.)
        .pt(16.)
        .child(section_head(
            "Run outputs",
            meta("Evaluation reports"),
            vp.compact(),
        ))
        .child(list)
}

pub(super) fn datasets(s: &State, vp: &Viewport) -> El {
    let r = &s.studio;
    let can_create = s.wsl_ready() && !r.pending && !r.session.text.trim().is_empty();
    let left = col()
        .key("dataset-create")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .child(h2("Recording datasets", 22.))
        .child(p(
            "Version a finalized recording into an immutable dataset. Versions reference the original files and verify them by hash.",
            12.,
        ))
        .child(labeled(
            "Dataset name",
            field(&r.name, "dataset-name", "flight-data").on_edit(|s: &mut State| &mut s.studio.name),
        ))
        .child(labeled(
            "Finalized recording",
            field(&r.session, "dataset-session", "Choose a recording below")
                .on_edit(|s: &mut State| &mut s.studio.session),
        ))
        .child(sources(s))
        .child(row().child(pill("create-dataset", "Create dataset version", can_create, |s| {
            let name = s.studio.name.text.trim().to_string();
            act(s, "dataset_create", format!("Requested dataset version {name}"))
        })));
    col()
        .w_full()
        .min_w(0.)
        .child(if vp.compact() {
            asset(readiness(s), left, true)
        } else {
            asset(left, readiness(s), false)
        })
        .child(versions(s, vp))
        .child(evaluation(s, vp))
        .child(reports(s, vp))
}

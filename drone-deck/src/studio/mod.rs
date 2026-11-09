mod bridge;
mod projects;
#[cfg(test)]
mod tests;
mod view;
use crate::*;
use serde_json::{Value, json};
use std::sync::mpsc::{self, Receiver, Sender};
pub(crate) use view::{page, settings};
pub(crate) struct Studio {
    pub token: TextEdit,
    pub stored: bool,
    pub repo: TextEdit,
    pub revision: TextEdit,
    pub patterns: TextEdit,
    pub name: TextEdit,
    pub session: TextEdit,
    pub csv: TextEdit,
    pub task: TextEdit,
    pub project_name: TextEdit,
    pub kind: usize,
    pub model_file: TextEdit,
    pub inputs: TextEdit,
    pub model_path: String,
    pub dataset: Option<Value>,
    pub preview: Option<Value>,
    pub models: Vec<Value>,
    pub library: Value,
    pub jobs: Vec<Value>,
    pub message: String,
    pub pending: bool,
    pub selected_job: Option<String>,
    /// Lazy Data & models loads already requested (`models`, `credential_status`, `data_library`).
    pub loaded: Vec<&'static str>,
    tx: Sender<Value>,
    rx: Receiver<Value>,
    last_refresh: Instant,
}
impl Default for Studio {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            token: TextEdit::new("").secure(),
            stored: false,
            repo: TextEdit::new(""),
            revision: TextEdit::new("main"),
            patterns: TextEdit::new("*.json,*.safetensors,*.onnx,*.gguf,*.txt,tokenizer.*"),
            name: TextEdit::new("flight-data"),
            session: TextEdit::new(""),
            csv: TextEdit::new(""),
            task: TextEdit::new("classification"),
            project_name: TextEdit::new("drone-project"),
            kind: 0,
            model_file: TextEdit::new(""),
            inputs: TextEdit::new(""),
            model_path: String::new(),
            dataset: None,
            preview: None,
            models: Vec::new(),
            library: json!({}),
            jobs: Vec::new(),
            message: "Choose a model, recording dataset, or project workflow.".into(),
            pending: false,
            selected_job: None,
            loaded: Vec::new(),
            tx,
            rx,
            last_refresh: Instant::now(),
        }
    }
}
pub(crate) fn request(s: &mut State, action: &str) {
    if s.headless {
        s.studio.message = "Preview: external Studio actions disabled.".into();
        return;
    }
    if s.studio.pending {
        return;
    }
    let linux = [
        "data_library",
        "dataset_create",
        "dataset_verify",
        "evaluate",
        "project_create",
    ]
    .contains(&action);
    if linux && !s.wsl_ready() {
        s.notice = "Select a working WSL 2 environment in Settings → Environment first.".into();
        s.navigate(crate::ui::Page::Settings);
        return;
    }
    let r = &mut s.studio;
    let mut value = json!({"action":action});
    match action {
        "save_token" => {
            let token = std::mem::replace(&mut r.token, TextEdit::new("").secure());
            value["token"] = json!(token.text);
        }
        "inspect_model" => {
            r.preview = None;
            value["repo"] = json!(r.repo.text.trim());
            value["revision"] = json!(r.revision.text.trim());
            value["patterns"] = json!(r.patterns.text.trim());
        }
        "start_model" => {
            let Some(preview) = &r.preview else { return };
            value["review"] = preview["review"].clone();
        }
        "retry_model" | "cancel_model" => {
            value["id"] = json!(r.selected_job);
        }
        "verify_model" => {
            let Some(model) = r
                .models
                .iter()
                .find(|m| m["path"].as_str() == Some(&r.model_path))
            else {
                return;
            };
            value["repo"] = model["repo"].clone();
            value["sha"] = model["sha"].clone();
        }
        "dataset_create" => {
            value["name"] = json!(r.name.text.trim());
            value["session"] = json!(r.session.text.trim());
        }
        "dataset_verify" | "evaluate" => {
            let Some(dataset) = &r.dataset else {
                r.message = "Select a dataset version first.".into();
                return;
            };
            value["name"] = dataset["name"].clone();
            value["version"] = dataset["version"].clone();
            value["csv"] = json!(r.csv.text.trim());
            value["task"] = json!(r.task.text.trim());
        }
        "project_create" => {
            value["name"] = json!(r.project_name.text.trim());
            value["kind"] = json!(["onnx", "ros2", "mavsdk"][r.kind.min(2)]);
            value["model"] = json!(windows_to_linux(&r.model_path));
            value["dataset"] = r
                .dataset
                .as_ref()
                .map(|v| v["manifest"].clone())
                .unwrap_or(json!(""));
        }
        _ => {}
    }
    r.pending = true;
    r.message = format!("{}…", action.replace('_', " "));
    bridge::request(value, s.config.distro.clone(), linux, r.tx.clone());
}
pub(crate) fn windows_to_linux(path: &str) -> String {
    let path = path.replace('\\', "/");
    if path.as_bytes().get(1) == Some(&b':') && path.as_bytes().get(2) == Some(&b'/') {
        format!("/mnt/{}/{}", path[..1].to_lowercase(), &path[3..])
    } else {
        path
    }
}
pub(crate) fn poll(s: &mut State) -> bool {
    let r = &mut s.studio;
    let mut changed = false;
    for event in r.rx.try_iter() {
        changed = true;
        if let Some(progress) = event["progress"].as_str() {
            r.message = progress.into();
            continue;
        }
        r.pending = false;
        if event["ok"].as_bool() != Some(true) {
            r.message = event["error"]
                .as_str()
                .unwrap_or("Studio request failed")
                .into();
            continue;
        }
        let data = event["data"].clone();
        r.message = data["message"].as_str().unwrap_or("Complete").into();
        match event["action"].as_str().unwrap_or("") {
            "credential_status" | "save_token" | "forget_token" | "validate_token" => {
                r.stored = data["stored"].as_bool().unwrap_or(r.stored)
            }
            "inspect_model" => r.preview = Some(data),
            "models" => r.models = data["models"].as_array().cloned().unwrap_or_default(),
            "data_library" => r.library = data,
            "ports" => r.library["ports"] = data["ports"].clone(),
            "start_model" | "retry_model" => {
                r.selected_job = data["id"].as_str().map(str::to_owned)
            }
            "dataset_create" => {
                r.dataset = Some(data["dataset"].clone());
                if !r.library["datasets"].is_array() {
                    r.library["datasets"] = json!([]);
                }
                r.library["datasets"]
                    .as_array_mut()
                    .unwrap()
                    .push(data["dataset"].clone());
            }
            "evaluate" => {
                if !r.library["reports"].is_array() {
                    r.library["reports"] = json!([]);
                }
                r.library["reports"]
                    .as_array_mut()
                    .unwrap()
                    .push(data["report"].clone());
            }
            "project_create" => {
                if !r.library["projects"].is_array() {
                    r.library["projects"] = json!([]);
                }
                r.library["projects"]
                    .as_array_mut()
                    .unwrap()
                    .push(data["project"].clone());
            }
            _ => {}
        }
    }
    if r.last_refresh.elapsed() > Duration::from_secs(1) {
        r.last_refresh = Instant::now();
        let root = backend::root().join("studio/jobs");
        let mut paths: Vec<_> = std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|d| d.path())
            .collect();
        paths.sort();
        paths.reverse();
        let jobs: Vec<Value> = paths
            .into_iter()
            .take(50)
            .filter_map(|path| {
                let mut state: Value =
                    serde_json::from_slice(&std::fs::read(path.join("state.json")).ok()?).ok()?;
                if let Ok(bytes) = std::fs::read(path.join("progress.json")) {
                    if let Ok(progress) = serde_json::from_slice::<Value>(&bytes) {
                        state["progress"] = progress;
                    }
                }
                let alive = crate::setup::job_alive(
                    state["pid"].as_u64().unwrap_or(0) as u32,
                    state["process_ticks"].as_u64().unwrap_or(0),
                );
                if !alive
                    && ["Downloading", "Queued"].contains(&state["status"].as_str().unwrap_or(""))
                    && path
                        .join("state.json")
                        .metadata()
                        .ok()?
                        .modified()
                        .ok()?
                        .elapsed()
                        .ok()?
                        .as_secs()
                        > 20
                {
                    state["status"] = json!("Interrupted");
                }
                state["alive"] = json!(alive);
                Some(state)
            })
            .collect();
        if jobs != r.jobs {
            r.jobs = jobs;
            changed = true;
        }
    }
    changed | view::lazy_load(s)
}

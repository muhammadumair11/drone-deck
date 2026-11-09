//! The launch-profile form shown inline in an expanded Tools & config row.
use super::*;
use crate::ui::kit::*;
use crate::ui::theme::*;

pub struct Form {
    pub fields: Vec<TextEdit>,
    pub creating: bool,
    /// Validation message shown beside the form.
    pub error: String,
}

/// Tools & config filters, matching `systems::Component::filter` and `systems::filter_for`.
pub(crate) const FILTERS: [&str; 7] = [
    "All tools",
    "Simulation",
    "Transport",
    "Telemetry",
    "Ground station",
    "Data",
    "AI",
];

const FIELDS: [(&str, &str); 8] = [
    ("Display name", "Name shown throughout the workspace"),
    (
        "Program",
        "wsl for an Ubuntu command, java for native jMAVSim, or a Windows executable path",
    ),
    (
        "Command / arguments",
        "WSL: one Linux command. Windows or Java: one argument per line, without JSON or surrounding quotes.",
    ),
    (
        "Windows working directory",
        "Optional. WSL commands set their own directory with cd",
    ),
    (
        "Installation probe",
        "A Bash test for WSL, or a file path for Windows",
    ),
    (
        "Package download URL",
        "Optional. The publisher's official HTTPS package. Download asks first, saves to workspace/downloads and never runs the file",
    ),
    ("Ubuntu distribution", "Shared by every WSL profile"),
    (
        "Category",
        "Simulation, Development, Infrastructure, Flight operations, or AI & data",
    ),
];

/// Opens the form for `s.selected` in its Tools & config row.
pub fn open(s: &mut State) {
    if s.page != crate::ui::Page::Tools {
        s.navigate(crate::ui::Page::Tools);
    }
    let Some(p) = s.config.profiles.get(s.selected) else {
        return;
    };
    let id = p.id.clone();
    s.form = Some(Form {
        creating: false,
        error: String::new(),
        fields: [
            p.name.clone(),
            p.program.clone(),
            if p.program == "wsl" {
                p.args.join(" ")
            } else {
                p.args.join("\n")
            },
            p.cwd.clone(),
            p.probe.clone(),
            p.download_url.clone(),
            s.config.distro.clone(),
            p.category.clone(),
        ]
        .into_iter()
        .map(TextEdit::new)
        .collect(),
    });
    reveal(s, &id);
    s.ui.expanded = Some(id);
    s.editing = false;
}

/// Makes sure the row for `id` is listed: widens the scope and clears a filter or search
/// that would hide it.
fn reveal(s: &mut State, id: &str) {
    let in_system = systems::profile_ids(s.systems.active).any(|p| p == id);
    if !in_system {
        s.ui.tools_all = true;
    }
    let Some(p) = s.config.profiles.iter().find(|p| p.id == id) else {
        return;
    };
    let filter = FILTERS[s.ui.tools_filter.min(FILTERS.len() - 1)];
    if s.ui.tools_filter != 0 && systems::filter_for(p) != filter {
        s.ui.tools_filter = 0;
    }
    let q = s.ui.search.text.trim().to_lowercase();
    if !q.is_empty()
        && ![&p.name, &p.description, &p.category]
            .iter()
            .any(|t| t.to_lowercase().contains(&q))
    {
        s.ui.search = TextEdit::new("");
    }
}

/// Opens an empty form for a new tool at the top of Tools & config.
pub fn create(s: &mut State, category: &str) {
    s.navigate(crate::ui::Page::Tools);
    s.form = Some(Form {
        creating: true,
        error: String::new(),
        fields: ["My tool", "wsl", "", "", "", "", &s.config.distro, category]
            .into_iter()
            .map(TextEdit::new)
            .collect(),
    });
    s.ui.expanded = Some("new".into());
    s.editing = false;
}

fn running(s: &State, form: &Form) -> bool {
    !form.creating
        && s.config
            .profiles
            .get(s.selected)
            .is_some_and(|p| systems::running(s, &p.id))
}

fn refuse(s: &mut State, message: &str) {
    s.notice = message.into();
    if let Some(f) = s.form.as_mut() {
        f.error = message.into();
    }
}

fn save(s: &mut State) {
    let Some(form) = &s.form else { return };
    let fields: Vec<_> = form
        .fields
        .iter()
        .map(|e| e.text.trim().to_string())
        .collect();
    let creating = form.creating;
    if running(s, form) {
        return refuse(s, "Stop this process before editing.");
    }
    let args = if fields[1] == "wsl" {
        vec![fields[2].clone()]
    } else {
        fields[2]
            .lines()
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .map(str::to_owned)
            .collect()
    };
    if fields[0].is_empty() || fields[1].is_empty() || fields[6].is_empty() {
        return refuse(s, "Name, program and WSL distribution are required.");
    }
    if fields[6] != s.config.distro && (s.busy() || s.setup.batch.is_some()) {
        return refuse(
            s,
            "Stop active tools, recordings and installations before changing the WSL distribution.",
        );
    }
    let mut next = s.config.clone();
    let mut selected = s.selected.min(next.profiles.len().saturating_sub(1));
    if creating {
        let mut sequence = 1;
        while next
            .profiles
            .iter()
            .any(|p| p.id == format!("custom-{sequence}"))
        {
            sequence += 1;
        }
        next.profiles.push(backend::Profile {
            id: format!("custom-{sequence}"),
            name: String::new(),
            category: String::new(),
            description: "Custom command".into(),
            program: String::new(),
            args: Vec::new(),
            cwd: String::new(),
            probe: String::new(),
            docs: String::new(),
            download_url: String::new(),
            exclusive: false,
        });
        selected = next.profiles.len() - 1;
    }
    let Some(p) = next.profiles.get_mut(selected) else {
        return refuse(
            s,
            "This profile is no longer registered. Close the form and open the tool again.",
        );
    };
    p.name = fields[0].clone();
    p.program = fields[1].clone();
    p.args = args;
    p.cwd = fields[3].clone();
    p.probe = fields[4].clone();
    p.download_url = fields[5].clone();
    p.category = fields[7].clone();
    next.distro = fields[6].clone();
    if !s.headless {
        if let Err(e) = backend::save(&next) {
            return refuse(s, &format!("The profile could not be saved: {e}"));
        }
    }
    let (id, name) = (
        next.profiles[selected].id.clone(),
        next.profiles[selected].name.clone(),
    );
    s.config = next;
    s.selected = selected;
    s.form = None;
    s.detected.remove(&id);
    if creating {
        s.ui.tools_all = true;
        s.ui.tools_filter = 0;
        s.ui.expanded = Some(id.clone());
        activity::record(s, format!("Added tool profile {name}"));
    } else {
        activity::record(s, format!("Saved {name} profile"));
    }
    s.notice = format!(
        "{name} saved. Detect tools to check the new path; a saved profile does not prove a connection."
    );
}

fn cancel(s: &mut State) {
    let creating = s.form.as_ref().is_some_and(|f| f.creating);
    s.form = None;
    if creating {
        s.ui.expanded = None;
    }
}

fn input(s: &State, i: usize, locked: bool) -> El {
    let form = s.form.as_ref().unwrap();
    let key = format!("field-{i}");
    if i == 2 {
        let edit = (!locked).then(|| form_field(i));
        return labeled_input(i, area(&form.fields[i], &key, (4, 12), edit));
    }
    let control = field(&form.fields[i], &key, "");
    let control = if locked {
        control.disabled(true).read_only(true).dim(0.5)
    } else {
        control.on_edit(form_field(i))
    };
    labeled_input(i, control)
}

fn form_field(i: usize) -> impl for<'a> Fn(&'a mut State) -> &'a mut TextEdit + 'static {
    move |s: &mut State| &mut s.form.as_mut().unwrap().fields[i]
}

fn labeled_input(i: usize, control: El) -> El {
    let (name, hint) = FIELDS[i];
    col()
        .w_full()
        .min_w(0.)
        .gap(8.)
        .child(caps(name, 10., 400., 0.08, muted()).wrap())
        .child(control.label(name))
        .child(p(hint, 10.))
}

fn pair(a: El, b: El, compact: bool) -> El {
    if compact {
        return col().w_full().min_w(0.).gap(20.).child(a).child(b);
    }
    row()
        .w_full()
        .min_w(0.)
        .gap(24.)
        .items_start()
        .child(col().grow(1.).min_w(0.).child(a))
        .child(col().grow(1.).min_w(0.).child(b))
}

/// The form for `s.form`. Locked, with an explanation, while its process runs.
pub fn view(s: &State, compact: bool) -> El {
    let Some(form) = s.form.as_ref() else {
        return el();
    };
    let locked = running(s, form);
    let title = if form.creating {
        "New launch profile"
    } else {
        "Launch profile"
    };
    let actions = row()
        .gap(16.)
        .center_y()
        .flex_wrap()
        .child(pill(
            "save-form",
            if form.creating {
                "Add tool"
            } else {
                "Save profile"
            },
            !locked,
            save,
        ))
        .child(ghost("cancel-form", "Cancel", true, cancel));
    let head = if compact {
        col().gap(12.).items_start()
    } else {
        row().gap(24.).center_y().justify_between()
    };
    let status = if locked {
        notice("Stop this process before editing.")
    } else if !form.error.is_empty() {
        body(form.error.clone(), 11., danger())
            .key("form-error")
            .role("status")
    } else if form.creating {
        notice(
            "Adding registers a launch profile only. Nothing runs until you start it; Detect tools checks its probe.",
        )
    } else {
        notice(
            "Saving changes the launch profile only. Detect tools to check the path; a saved profile does not prove a connection.",
        )
    };
    col()
        .key("profile-form")
        .w_full()
        .min_w(0.)
        .gap(20.)
        .pt(8.)
        .child(head.w_full().min_w(0.).child(h3(title, 15.)).child(actions))
        .child(status)
        .child(pair(input(s, 0, locked), input(s, 7, locked), compact))
        .child(pair(input(s, 1, locked), input(s, 6, locked), compact))
        .child(input(s, 2, locked))
        .child(pair(input(s, 3, locked), input(s, 4, locked), compact))
        .child(input(s, 5, locked))
}

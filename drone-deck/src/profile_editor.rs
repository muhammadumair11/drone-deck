use super::*;

pub struct Form {
    pub fields: Vec<TextEdit>,
    pub creating: bool,
}
pub fn open(s: &mut State) {
    s.ui.reset_scroll = true;
    let p = &s.config.profiles[s.selected];
    s.form = Some(Form {
        creating: false,
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
    s.page = 5;
    s.editing = false;
}
pub fn create(s: &mut State, category: &str) {
    open(s);
    let form = s.form.as_mut().unwrap();
    form.creating = true;
    form.fields = ["My tool", "wsl", "", "", "", "", &s.config.distro, category]
        .into_iter()
        .map(TextEdit::new)
        .collect();
}
fn save(s: &mut State) {
    let Some(form) = &s.form else { return };
    let fields: Vec<_> = form.fields.iter().map(|e| e.text.clone()).collect();
    let creating = form.creating;
    let args = if fields[1] == "wsl" {
        vec![fields[2].clone()]
    } else {
        fields[2]
            .lines()
            .filter(|a| !a.is_empty())
            .map(str::to_owned)
            .collect()
    };
    if fields[0].is_empty() || fields[1].is_empty() || fields[6].is_empty() {
        s.notice = "Name, program and WSL distribution are required.".into();
        return;
    }
    let mut next = s.config.clone();
    if s.busy() || s.setup.batch.is_some() {
        s.notice =
            "Stop the active workspace or recording before changing its distribution.".into();
        return;
    }
    let mut selected = s.selected;
    if creating {
        let mut sequence = 1;
        while next
            .profiles
            .iter()
            .any(|p| p.id == format!("custom-{sequence}"))
        {
            sequence += 1;
        }
        let mut profile = next.profiles[s.selected].clone();
        profile.id = format!("custom-{sequence}");
        profile.exclusive = false;
        profile.docs = String::new();
        profile.description = "Custom command".into();
        next.profiles.push(profile);
        selected = next.profiles.len() - 1;
    }
    let p = &mut next.profiles[selected];
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
            s.notice = e;
            return;
        }
    }
    s.config = next;
    s.selected = selected;
    s.form = None;
    s.notice = "Integration settings saved. Detect tools to check the new configuration.".into();
}
pub fn view(s: &State) -> El {
    let form = s.form.as_ref().unwrap();
    let fields = [
        ("Display name", "Name shown throughout the workspace"),
        (
            "Program",
            "Use wsl for an Ubuntu command, java for native jMAVSim, or a Windows executable path",
        ),
        (
            "Command / arguments",
            "WSL: enter a Linux command. Windows/Java: enter one argument per line; do not add JSON or surrounding quotes.",
        ),
        (
            "Windows working directory",
            "Optional; WSL commands set their own directory with cd",
        ),
        (
            "Installation probe",
            "A Bash probe for WSL, or a file path for Windows",
        ),
        (
            "Package download URL",
            "Optional official HTTPS package URL; downloads require confirmation",
        ),
        ("Ubuntu distribution", "Shared by all WSL profiles"),
        (
            "Category",
            "Simulation, Development, Infrastructure, Flight operations, or AI & data",
        ),
    ];
    let mut body = card().child(
        row()
            .flex_wrap()
            .gap(12.)
            .justify_between()
            .child(title("Configure integration", 20.).wrap())
            .child(
                row()
                    .gap(8.)
                    .flex_wrap()
                    .child(button("cancel-form", "Cancel", false, |s| s.form = None))
                    .child(button("save-form", "Save settings", true, save)),
            ),
    );
    for (i, (name, hint)) in fields.iter().enumerate() {
        let mut input = text_input(&form.fields[i], TextStyle::new(12., 400.))
            .key(format!("field-{i}"))
            .on_edit(move |s: &mut State| &mut s.form.as_mut().unwrap().fields[i])
            .w_full()
            .h(if i == 2 { 110. } else { 42. })
            .padding(8.)
            .fill(surface())
            .border(1., line())
            .radius(5.)
            .color(ink());
        if i != 2 {
            input = input.single_line();
        }
        body = body.child(
            col()
                .w_full()
                .min_w(0.)
                .gap(5.)
                .child(title(*name, 12.))
                .child(label(*hint, 10., muted()).wrap())
                .child(input),
        );
    }
    body
}

use super::*;
pub(super) fn catalog(s: &State, category: &str, narrow: bool) -> El {
    let query = s.ui.search.text.to_lowercase();
    let mut list = col().w_full().min_w(0.).gap(10.);
    let mut count = 0;
    for (i, p) in s.config.profiles.iter().enumerate() {
        if (category == "All" || p.category == category)
            && format!("{} {} {}", p.name, p.description, p.category)
                .to_lowercase()
                .contains(&query)
        {
            list = list.child(tool_row(s, i, narrow));
            count += 1;
        }
    }
    if count == 0 {
        list = list.child(
            card().child(title("No matching tools", 18.)).child(
                label(
                    "Change the search text or add a command profile in Integrations.",
                    13.,
                    muted(),
                )
                .wrap(),
            ),
        );
    }
    list
}
fn pipeline(s: &State, vertical: bool) -> El {
    let mut flow = if vertical { col() } else { row() };
    flow = flow.key("pipeline").w_full().min_w(0.).gap(12.);
    for (index, (id, tag, name, desc)) in [
        (
            "sih",
            "01 / FLIGHT STACK",
            "PX4 + SIH",
            "Physics & flight controller",
        ),
        (
            "relay",
            "02 / TRANSPORT",
            "UDP relay",
            "WSL → Windows · 19410",
        ),
        (
            "jmavsim",
            "03 / VISUALIZER",
            "jMAVSim",
            "Native Java · display only",
        ),
    ]
    .iter()
    .enumerate()
    {
        if index > 0 {
            flow = flow.child(label(if vertical { "↓" } else { "→" }, 22., muted()).self_center());
        }
        let mut step = col()
            .key(format!("pipeline-{id}"))
            .min_w(0.)
            .padding(18.)
            .gap(12.)
            .radius(8.)
            .fill(if index == 0 {
                c(242, 249, 255)
            } else {
                surface()
            })
            .border(1., if index == 0 { c(161, 203, 240) } else { line() });
        if !vertical {
            step = step.grow(1.).min_h(168.);
        }
        let status = s.status.get(*id).map(String::as_str).unwrap_or("Stopped");
        let detail = if status == "Running" {
            s.readiness
                .get(*id)
                .map(String::as_str)
                .unwrap_or("Process running")
        } else {
            status
        };
        step = step
            .child(label(*tag, 10., muted()).wrap())
            .child(title(*name, 20.).wrap())
            .child(label(*desc, 13., muted()).wrap())
            .child(
                row()
                    .gap(7.)
                    .center_y()
                    .child(el().size(8., 8.).radius(4.).fill(if status == "Running" {
                        c(19, 122, 44)
                    } else {
                        muted()
                    }))
                    .child(label(detail, 13., muted())),
            );
        flow = flow.child(step);
    }
    flow
}
pub(super) fn session(s: &State, width: f32) -> El {
    let text = col()
        .grow(1.)
        .min_w(0.)
        .gap(10.)
        .child(label("ACTIVE PROFILE / LOCAL SIMULATION", 11., accent()).wrap())
        .child(title("Your flight lab, connected.", 32.).wrap())
        .child(
            label(
                "One workspace for the flight stack, transport, visualizer, and autonomy tools.",
                14.,
                muted(),
            )
            .wrap(),
        );
    let active = s.session.is_some()
        || ["sih", "relay", "jmavsim"].iter().all(|id| {
            s.status
                .get(*id)
                .is_some_and(|v| v == "Running" || v == "Starting")
        });
    let start = button(
        "start-session",
        if active {
            "Workspace active"
        } else {
            "Start SIH workspace"
        },
        true,
        |s| s.bundle(),
    )
    .disabled(active)
    .opacity(if active { 0.5 } else { 1. });
    let top = if width < 680. {
        col().gap(16.).child(text).child(start.w_full())
    } else {
        col().gap(20.).child(text).child(start.self_start())
    };
    let mut tools = card().child(heading("Session tools", "OWNED PROCESSES"));
    for id in ["qgc", "telemetry", "dds"] {
        if let Some(i) = s.config.profiles.iter().position(|p| p.id == id) {
            tools = tools.child(tool_row(s, i, width < 650.));
        }
    }
    col()
        .w_full()
        .min_w(0.)
        .gap(16.)
        .child(
            card()
                .key("launch-panel")
                .child(top)
                .child(pipeline(s, width < 720.)),
        )
        .child(tools)
}
fn hero(eyebrow: &str, headline: &str, copy: &str) -> El {
    card()
        .fill(surface())
        .padding(24.)
        .child(label(eyebrow, 11., accent()).wrap())
        .child(title(headline, 32.).wrap())
        .child(label(copy, 14., muted()).wrap())
}
pub(super) fn page(s: &State, width: f32) -> El {
    if s.page == 8 {
        return crate::setup::page(s, width);
    }
    if s.page == 7 {
        return crate::recordings::page(s, width);
    }
    if s.page == 6 {
        return super::settings::page(s);
    }
    let narrow = width < 650.;
    match s.page {
        0=>session(s,width),
        1=>col().gap(16.).child(hero("SIMULATION CATALOG","Choose the right flight environment.","Review local simulation targets and run a configured flight-stack profile.")).child(catalog(s,"Simulation",narrow)),
        2=>flight(s,narrow),
        3=>col().gap(16.).child(hero("LOCAL TOOLCHAIN","Build, run, and inspect autonomy software.","Run MAVSDK clients, ROS nodes and the DDS agent with managed lifecycle and captured process output."))
            .child(crate::studio::projects(s))
            .child(catalog(s,"Development",narrow)).child(catalog(s,"Infrastructure",narrow)),
        4=>col().gap(16.).child(button("open-recordings","Record & replay sensor data",true,|s|s.navigate(7)))
            .child(crate::studio::page(s))
            .child(catalog(s,"AI & data",narrow)).child(button("add-ai","Add AI command",true,|s|profile_editor::create(s,"AI & data"))),
        _=>integrations(s,narrow),
    }
}
fn flight(s: &State, narrow: bool) -> El {
    let mut body=col().gap(16.).child(hero("VEHICLE OPERATIONS","Choose a vehicle source.","Native monitoring, local simulation and isolated replay have separate connection state." )).child(crate::hardware::page(s));
    if s.hardware.mode == 0 {
        body=body.child(card().child(heading("Simulation telemetry","READ-ONLY MAVSDK"))
    .child(metric("LATITUDE",s.metrics.get("latitude").map(String::as_str).unwrap_or("—"),"degrees"))
    .child(metric("LONGITUDE",s.metrics.get("longitude").map(String::as_str).unwrap_or("—"),"degrees"))
    .child(label("The simulation telemetry adapter uses WSL UDP 14540. Close another listener on that port first.",13.,muted()).wrap()))
    .child(catalog(s,"Development",narrow));
    } else if s.hardware.mode == 2 {
        body = body.child(button(
            "flight-replay",
            "Open isolated replay workspace",
            true,
            |s| s.navigate(7),
        ));
    }
    body.child(catalog(s, "Flight operations", narrow))
}
fn integrations(s: &State, narrow: bool) -> El {
    if s.form.is_some() {
        return profile_editor::view(s);
    }
    if s.editing {
        return card().child(heading("Profile configuration","ADVANCED JSON"))
        .child(row().gap(8.).flex_wrap().child(button("cancel-edit","Cancel",false,|s|s.editing=false)).child(button("save-config","Validate & save",true,|s|s.apply())))
        .child(label("Add command profiles for simulators, ROS nodes or AI services. Windows programs use argument arrays; WSL profiles contain a Bash command.",13.,muted()).wrap())
        .child(text_input(&s.editor,TextStyle::new(12.,400.)).key("profile-editor").on_edit(|s:&mut State|&mut s.editor).w_full().min_w(0.).h(420.).padding(12.).radius(6.).fill(code_bg()).color(c(246,245,244)));
    }
    col().gap(16.).child(hero("CONNECTED SYSTEMS","Make every dependency visible.","Detect, configure and troubleshoot flight software, tools and data. Setup actions use official guidance; package downloads require confirmation."))
        .child(button("open-managed-setup","Set up environments & install tools",true,|s|s.navigate(8)))
        .child(row().gap(8.).flex_wrap().child(button("add-profile","Add tool",true,|s|profile_editor::create(s,"Development"))).child(button("edit-profiles","Advanced: edit JSON",false,|s|s.edit())).child(button("open-workspace","Open workspace folder",false,|s|s.open(backend::root().to_string_lossy().into_owned()))))
        .child(catalog(s,"All",narrow))
}

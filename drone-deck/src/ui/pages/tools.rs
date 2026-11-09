//! 02 Tools & config (`Design/system-tools.html`): the tool catalog for the active system or
//! every registered profile, with filters, search and inline configuration rows.
use super::*;
use crate::backend::Request;
use crate::profile_editor::FILTERS;
use crate::systems::{Backing, Component};
mod detail;

/// A listed tool: a launch profile, a system component, a setup recipe with no profile yet,
/// or a combination.
struct Item {
    id: String,
    index: Option<usize>,
    comp: Option<&'static Component>,
    /// Setup recipe of a tool that has no launch profile yet.
    recipe: Option<usize>,
}
impl Item {
    fn profile<'a>(&self, s: &'a State) -> Option<&'a crate::backend::Profile> {
        self.index.and_then(|i| s.config.profiles.get(i))
    }
    fn recipe<'a>(&self, s: &'a State) -> Option<&'a crate::setup::Recipe> {
        self.recipe.and_then(|i| s.setup.recipes.get(i))
    }
    fn name(&self, s: &State) -> String {
        match (self.profile(s), self.comp, self.recipe(s)) {
            (Some(p), _, _) => p.name.clone(),
            (None, Some(c), _) => c.name.into(),
            (None, None, Some(r)) => r.name.clone(),
            _ => self.id.clone(),
        }
    }
    fn description(&self, s: &State) -> String {
        match (self.comp, self.profile(s), self.recipe(s)) {
            (Some(c), _, _) => c.role.into(),
            (None, Some(p), _) => p.description.clone(),
            (None, None, Some(r)) => r.summary.clone(),
            _ => String::new(),
        }
    }
    fn target(&self, s: &State) -> &'static str {
        match (self.profile(s), self.comp, self.recipe(s)) {
            (Some(p), _, _) => target(&p.program),
            (None, Some(c), _) => c.target,
            (None, None, Some(r)) if r.target == "Windows" => "Windows",
            (None, None, Some(_)) => "Ubuntu / WSL",
            _ => "",
        }
    }
    fn filter(&self, s: &State) -> &'static str {
        match (self.comp, self.profile(s), self.recipe(s)) {
            (Some(c), _, _) => c.filter,
            (None, Some(p), _) => systems::filter_for(p),
            (None, None, Some(r)) => recipe_filter(&r.id),
            _ => "Data",
        }
    }
    /// Profile-backed rows can be started and configured; the rest link to their page.
    fn configurable(&self) -> bool {
        self.comp
            .is_none_or(|c| matches!(c.backing, Backing::Profile(_)))
    }
    fn matches(&self, s: &State, q: &str) -> bool {
        let mut hay = format!(
            "{} {} {}",
            self.name(s),
            self.description(s),
            self.filter(s)
        );
        if let Some(p) = self.profile(s) {
            for extra in [&p.description, &p.category] {
                hay.push(' ');
                hay.push_str(extra);
            }
        }
        hay.to_lowercase().contains(q)
    }
}

pub(super) fn target(program: &str) -> &'static str {
    match program {
        "wsl" => "Ubuntu / WSL",
        "java" => "Windows / Java",
        _ => "Windows",
    }
}

/// Launch profile id that a setup recipe configures.
fn profile_for_recipe(recipe: &str) -> &str {
    match recipe {
        "px4" => "sih",
        "mavsdk" => "telemetry",
        other => other,
    }
}

/// Filter category of a tool that only has a setup recipe.
fn recipe_filter(recipe: &str) -> &'static str {
    match recipe {
        "px4" | "jmavsim" | "gazebo" | "ardupilot" => "Simulation",
        "dds" => "Transport",
        "mavsdk" => "Telemetry",
        "qgc" => "Ground station",
        "onnx" | "torch" => "AI",
        _ => "Data",
    }
}

fn component_for(id: &str) -> Option<&'static Component> {
    systems::all_components()
        .into_iter()
        .find(|c| matches!(c.backing, Backing::Profile(p) if p == id))
}

fn items(s: &State) -> Vec<Item> {
    let position = |id: &str| s.config.profiles.iter().position(|p| p.id == id);
    let listed: Vec<Item> = if s.ui.tools_all {
        let mut all: Vec<Item> = s
            .config
            .profiles
            .iter()
            .enumerate()
            .map(|(i, p)| Item {
                id: p.id.clone(),
                index: Some(i),
                comp: component_for(&p.id),
                recipe: None,
            })
            .collect();
        // Recipe-backed tools without a profile stay reachable for review, detection and adoption.
        for (i, r) in s.setup.recipes.iter().enumerate() {
            let id = profile_for_recipe(&r.id);
            if matches!(r.id.as_str(), "wsl" | "distro") || position(id).is_some() {
                continue;
            }
            all.push(Item {
                id: id.into(),
                index: None,
                comp: component_for(id),
                recipe: Some(i),
            });
        }
        all
    } else {
        systems::components(s.systems.active)
            .iter()
            .map(|c| Item {
                id: c.id.into(),
                index: match c.backing {
                    Backing::Profile(id) => position(id),
                    _ => None,
                },
                comp: Some(c),
                recipe: None,
            })
            .collect()
    };
    let filter = FILTERS[s.ui.tools_filter.min(FILTERS.len() - 1)];
    let q = s.ui.search.text.trim().to_lowercase();
    listed
        .into_iter()
        .filter(|it| s.ui.tools_filter == 0 || it.filter(s) == filter)
        .filter(|it| q.is_empty() || it.matches(s, &q))
        .collect()
}

fn detect(s: &mut State) {
    if s.backend.is_none() {
        s.notice = if s.headless {
            "Headless preview: tool detection is disabled.".into()
        } else {
            "The tool manager is still starting. Detect again in a moment.".into()
        };
        return;
    }
    s.send(Request::Detect(s.config.clone()));
    s.notice = "Checking Windows paths and WSL probes for every registered tool…".into();
    crate::activity::record(s, "Requested tool detection");
}

fn toggle_process(s: &mut State, id: &str) {
    let Some(i) = s.config.profiles.iter().position(|p| p.id == id) else {
        return;
    };
    let name = s.config.profiles[i].name.clone();
    if systems::running(s, id) {
        s.send(Request::Stop(id.into()));
        s.notice = format!("Stopping {name}. This stops the owned process, not the vehicle.");
        crate::activity::record(s, format!("Stopped {name}"));
    } else {
        s.start(i);
        if s.status.get(id).is_some_and(|v| v == "Starting") {
            s.notice = format!("Starting {name}. A running process is not yet readiness.");
            crate::activity::record(s, format!("Started {name}"));
        }
    }
}

fn toggle_config(s: &mut State, id: &str) {
    s.setup.review = false;
    if s.ui.expanded.as_deref() == Some(id) {
        s.ui.expanded = None;
        s.form = None;
        return;
    }
    match s.config.profiles.iter().position(|p| p.id == id) {
        Some(i) => {
            s.selected = i;
            crate::profile_editor::open(s);
        }
        None => {
            s.form = None;
            s.ui.expanded = Some(id.into());
        }
    }
}

/// Where a component without a launch profile is managed.
fn link(c: &Component) -> (&'static str, fn(&mut State)) {
    match c.backing {
        Backing::Recorder => ("Recordings ↗", |s| {
            s.ui.data_tab = 0;
            s.navigate(Page::Data)
        }),
        Backing::Projects => ("Projects ↗", |s| {
            s.ui.data_tab = 3;
            s.navigate(Page::Data)
        }),
        Backing::Artifacts => ("Reports ↗", |s| {
            s.ui.data_tab = 2;
            s.navigate(Page::Data)
        }),
        _ => ("System ↗", |s| s.navigate(Page::System)),
    }
}

fn actions(s: &State, it: &Item, open: bool) -> El {
    let mut a = row().gaps(0., 16.).center_y().flex_wrap();
    if !it.configurable() {
        let (label, go) = link(it.comp.unwrap());
        return a.child(ghost(format!("open-{}", it.id), label, true, go));
    }
    let id = it.id.clone();
    let running = systems::running(s, &it.id);
    // A tool known only from its setup recipe has nothing to start until it is installed or adopted.
    if it.index.is_some() || it.recipe.is_none() {
        a = a.child(ghost(
            format!("start-{}", it.id),
            if running { "Stop" } else { "Start" },
            it.index.is_some(),
            move |s| toggle_process(s, &id),
        ));
    }
    let id = it.id.clone();
    let label = match (open, it.index) {
        (true, _) => "Close ▴",
        (false, Some(_)) => "Configure ▾",
        (false, None) => "Install / adopt ▾",
    };
    a.child(
        ghost(format!("configure-{}", it.id), label, true, move |s| {
            toggle_config(s, &id)
        })
        .expanded(open),
    )
}

fn tool_row(s: &State, it: &Item, vp: &Viewport) -> El {
    let open = it.configurable() && s.ui.expanded.as_deref() == Some(it.id.as_str());
    let (state, tone) = match (it.index, it.comp) {
        (Some(_), _) | (None, None) => systems::profile_state(s, &it.id),
        (None, Some(c)) => systems::state(s, c),
    };
    let name = col()
        .gap(4.)
        .min_w(0.)
        .child(caps(it.name(s), 12., 700., 0.045, fg()).wrap())
        .child(meta_sized(it.description(s), 9., muted()).wrap());
    let target = meta_sized(it.target(s), 9., muted()).wrap();
    let status = row()
        .gap(8.)
        .center_y()
        .min_w(0.)
        .child(dot(systems::tone_color(tone)))
        .child(meta_sized(state, 9., fg()).wrap().grow(1.));
    let actions = actions(s, it, open);
    let line = if vp.compact() {
        col()
            .w_full()
            .min_w(0.)
            .gap(12.)
            .child(
                row()
                    .w_full()
                    .gap(12.)
                    .items_start()
                    .child(name.grow(1.))
                    .child(target.grow(1.)),
            )
            .child(
                row()
                    .w_full()
                    .gap(12.)
                    .center_y()
                    .child(status.grow(1.))
                    .child(col().grow(1.).min_w(0.).items_start().child(actions)),
            )
    } else {
        row()
            .w_full()
            .min_w(0.)
            .gap(if vp.medium() { 12. } else { 20. })
            .center_y()
            .child(name.grow(1.2))
            .child(target.grow(1.))
            .child(status.grow(1.))
            .child(
                col()
                    .w(if vp.medium() { 168. } else { 196. })
                    .items_end()
                    .child(actions),
            )
    };
    let pad = if vp.compact() || vp.wide() { 20. } else { 10. };
    let mut block = col()
        .key(format!("tool-{}", it.id))
        .label(it.name(s))
        .w_full()
        .min_w(0.)
        .border_sides([1., 0., 0., 0.], soft())
        .child(line.py(pad));
    if open {
        block = block.child(detail::view(s, it, vp));
    }
    block
}

fn new_tool(s: &State, vp: &Viewport) -> El {
    col()
        .key("tool-new")
        .w_full()
        .min_w(0.)
        .gap(8.)
        .pt(18.)
        .pb(28.)
        .border_sides([1., 0., 0., 0.], rule())
        .child(meta("New tool"))
        .child(p(
            "Register a command this workspace does not list yet. It joins all registered tools.",
            12.,
        ).max_w(760.))
        .child(crate::profile_editor::view(s, vp.compact()))
}

fn filter_bar(s: &State, vp: &Viewport) -> El {
    let filters = tabs("filter", &FILTERS, s.ui.tools_filter, |s, i| {
        s.ui.tools_filter = i;
    });
    let scope = ghost(
        "tools-scope",
        if s.ui.tools_all {
            "All registered tools ⇄"
        } else {
            "Active system only ⇄"
        },
        true,
        |s| {
            s.ui.tools_all = !s.ui.tools_all;
            s.ui.expanded = None;
            s.form = None;
        },
    )
    .help("Switch between the active system's tools and every registered profile");
    let search = field(&s.ui.search, "tool-search", "Search tools")
        .label("Search tools")
        .on_edit(|s: &mut State| &mut s.ui.search);
    let tools = if vp.compact() {
        col()
            .w_full()
            .gap(8.)
            .items_start()
            .child(scope)
            .child(search)
    } else {
        row()
            .gap(20.)
            .center_y()
            .child(scope)
            .child(col().w(240.).child(search))
    };
    let bar = if vp.medium() {
        col().gap(0.).items_start()
    } else {
        row().gap(24.).center_y().justify_between()
    };
    bar.key("filterbar")
        .w_full()
        .min_w(0.)
        .child(filters.min_w(0.))
        .child(tools)
}

fn list_head(s: &State, count: usize, vp: &Viewport) -> El {
    let scope = if s.ui.tools_all {
        "All registered tools".to_string()
    } else {
        format!("{} / active system", systems::KINDS[s.systems.active])
    };
    let n = format!(
        "{count} tool{} / {scope}",
        if count == 1 { "" } else { "s" }
    );
    let category = FILTERS[s.ui.tools_filter.min(FILTERS.len() - 1)];
    let add = pill(
        "add-profile",
        "Add a tool",
        s.form.is_none() || s.ui.expanded.as_deref() != Some("new"),
        move |s| {
            let category = match category {
                "Simulation" => "Simulation",
                "Transport" => "Infrastructure",
                "Ground station" => "Flight operations",
                "AI" => "AI & data",
                _ => "Development",
            };
            crate::profile_editor::create(s, category)
        },
    );
    let head = if vp.compact() {
        col().gap(12.).items_start()
    } else {
        row().gap(24.).center_y().justify_between()
    };
    head.key("tools-head")
        .w_full()
        .min_w(0.)
        .pt(if vp.compact() { 20. } else { 8. })
        .pb(16.)
        .child(meta(n).wrap())
        .child(add)
}

fn empty_state(s: &State) -> El {
    let searching = !s.ui.search.text.trim().is_empty();
    let (title, text) = if searching {
        (
            "No tools match this search.",
            "Search covers names, roles, descriptions and categories in the current scope.",
        )
    } else if s.ui.tools_all {
        (
            "No registered tools in this category.",
            "Add a tool to register a command for this category.",
        )
    } else {
        (
            "No tools in this category for the selected system.",
            "Other systems or custom profiles may still provide one. Show all registered tools to see them.",
        )
    };
    let mut actions = row().gap(20.).flex_wrap();
    if searching {
        actions = actions.child(ghost("clear-tool-search", "Clear search", true, |s| {
            s.ui.search = TextEdit::new("")
        }));
    }
    if !s.ui.tools_all {
        actions = actions.child(ghost(
            "tools-show-all",
            "Show all registered tools",
            true,
            |s| s.ui.tools_all = true,
        ));
    }
    col()
        .w_full()
        .border_sides([1., 0., 0., 0.], soft())
        .child(empty("tools-empty", title, text))
        .child(actions)
}

pub(super) fn page(s: &State, vp: &Viewport) -> El {
    let kind = s.systems.active;
    let eyebrow = format!(
        "{} / {}",
        s.systems.name(kind),
        if s.systems.prepared(kind) {
            "Configured"
        } else {
            "Preparation required"
        }
    );
    let actions = row()
        .gaps(8., 20.)
        .center_y()
        .flex_wrap()
        .child(pill("detect-tools", "Detect tools", true, detect))
        .child(ghost(
            "guided-preparation",
            "Guided preparation ↗",
            true,
            |s| crate::guided::open(s),
        ));
    let list = items(s);
    let mut content = col()
        .key("tools-page")
        .w_full()
        .min_w(0.)
        .child(mast(
            &eyebrow,
            "Tools & configuration.",
            "Inspect execution targets, commands and saved paths. Reuse known installations before installing another copy.",
            Some(actions),
            vp.compact(),
        ))
        .child(filter_bar(s, vp))
        .child(list_head(s, list.len(), vp));
    if s.ui.expanded.as_deref() == Some("new") && s.form.as_ref().is_some_and(|f| f.creating) {
        content = content.child(new_tool(s, vp));
    }
    let mut rows = col().key("tool-list").w_full().min_w(0.);
    if list.is_empty() {
        rows = rows.child(empty_state(s));
    }
    for it in &list {
        rows = rows.child(tool_row(s, it, vp));
    }
    content
        .child(rows)
        .child(el().w_full().h(1.).fill(soft()))
        .child(
            notice("Running, installed, configured and connected are separate states. A path found by detection is not a verified connection.")
                .mt(16.),
        )
}

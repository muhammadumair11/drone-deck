//! Parameters: fetch (with progress), a searchable table, `.params` export, and a comparison
//! with a loaded file. Nothing here writes a parameter.
use super::{bar, blocked, mine, percent};
use crate::hardware::{self, Baseline};
use crate::mavlink::{ParamSet, ParamValue};
use crate::ui::{kit::*, theme::*};
use crate::{State, TextEdit};
use rune::{El, GrowthSpec, ScrollSpec, col, el, row};
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::rc::Rc;

/// How a parameter compares with the loaded file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mark {
    /// No file loaded.
    Plain,
    Same,
    Changed,
    OnlyVehicle,
    OnlyFile,
}
impl Mark {
    pub(super) fn differs(self) -> bool {
        matches!(self, Self::Changed | Self::OnlyVehicle | Self::OnlyFile)
    }
    fn label(self) -> &'static str {
        match self {
            Self::Changed => "Changed",
            Self::OnlyVehicle => "Only on vehicle",
            Self::OnlyFile => "Only in file",
            Self::Plain | Self::Same => "",
        }
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct ParamRow {
    pub name: String,
    pub vehicle: Option<ParamValue>,
    pub file: Option<ParamValue>,
    pub mark: Mark,
}

/// The table rows: every parameter on the vehicle and in the file, by name, filtered by the
/// search text (any case) and, when `changed_only`, to those that differ from the file.
pub(super) fn rows(
    vehicle: Option<&ParamSet>,
    file: Option<&ParamSet>,
    search: &str,
    changed_only: bool,
) -> Vec<ParamRow> {
    let changed: BTreeSet<String> = match (vehicle, file) {
        (Some(v), Some(f)) => crate::vehicle::diff_params(f, v)
            .into_iter()
            .map(|c| c.name)
            .collect(),
        _ => BTreeSet::new(),
    };
    let mut names: BTreeSet<&String> = BTreeSet::new();
    names.extend(vehicle.iter().flat_map(|v| v.params.keys()));
    names.extend(file.iter().flat_map(|f| f.params.keys()));
    let changed_only = changed_only && vehicle.is_some() && file.is_some();
    let needle = search.trim().to_ascii_lowercase();
    names
        .into_iter()
        .filter(|n| needle.is_empty() || n.to_ascii_lowercase().contains(&needle))
        .filter_map(|name| {
            let on_vehicle = vehicle.and_then(|v| v.params.get(name)).map(|p| p.value);
            let in_file = file.and_then(|f| f.params.get(name)).map(|p| p.value);
            let mark = match (vehicle.is_some() && file.is_some(), on_vehicle, in_file) {
                (false, _, _) => Mark::Plain,
                (true, Some(_), None) => Mark::OnlyVehicle,
                (true, None, Some(_)) => Mark::OnlyFile,
                _ if changed.contains(name) => Mark::Changed,
                _ => Mark::Same,
            };
            (!changed_only || mark.differs()).then(|| ParamRow {
                name: name.clone(),
                vehicle: on_vehicle,
                file: in_file,
                mark,
            })
        })
        .collect()
}

fn value(v: Option<ParamValue>) -> String {
    v.map_or_else(|| "-".into(), |v| v.to_string())
}

fn param_row(r: &ParamRow, compact: bool, has_vehicle: bool, has_file: bool) -> El {
    let marked = r.mark.differs();
    let mark = if marked {
        row()
            .gap(8.)
            .center_y()
            .child(dot(warn()))
            .child(meta_sized(r.mark.label(), 9., warn()))
    } else {
        el()
    };
    let name = code(r.name.clone(), 11., fg()).wrap_anywhere();
    let vehicle = code(value(r.vehicle), 11., fg());
    let file = code(value(r.file), 11., if marked { warn() } else { muted() });
    if compact {
        let mut values = row().gaps(4., 16.).flex_wrap().center_y();
        if has_vehicle {
            values = values
                .child(meta_sized("Vehicle", 9., muted()))
                .child(vehicle);
        }
        if has_file {
            values = values.child(meta_sized("File", 9., muted())).child(file);
        }
        return ruled(false).py(8.).child(
            col()
                .w_full()
                .min_w(0.)
                .gap(4.)
                .child(name)
                .child(values.child(mark)),
        );
    }
    let mut line = ruled(false)
        .py(9.)
        .gap(16.)
        .center_y()
        .child(col().grow(1.).min_w(0.).child(name));
    if has_vehicle {
        line = line.child(col().w(150.).min_w(0.).child(vehicle));
    }
    if has_file {
        line = line.child(col().w(150.).min_w(0.).child(file));
    }
    line.child(col().w(130.).min_w(0.).child(mark))
}

fn header(has_vehicle: bool, has_file: bool) -> El {
    let mut line = row()
        .w_full()
        .min_w(0.)
        .gap(16.)
        .pb(8.)
        .child(col().grow(1.).child(meta_sized("Name", 9., muted())));
    if has_vehicle {
        line = line.child(col().w(150.).child(meta_sized("Vehicle", 9., muted())));
    }
    if has_file {
        line = line.child(col().w(150.).child(meta_sized("File", 9., muted())));
    }
    line.child(col().w(130.).child(meta_sized("Compared", 9., muted())))
}

fn table(s: &State, compact: bool, vehicle: Option<&ParamSet>, file: Option<&Baseline>) -> El {
    let h = &s.hardware;
    let changed_only = h.params_changed && file.is_some() && vehicle.is_some();
    let rows = rows(
        vehicle,
        file.map(|b| &b.set),
        &h.params_search.text,
        changed_only,
    );
    let (has_vehicle, has_file) = (vehicle.is_some(), file.is_some());
    let filters = row()
        .w_full()
        .min_w(0.)
        .gaps(8., 20.)
        .center_y()
        .flex_wrap()
        .child(
            if compact {
                col().w_full()
            } else {
                col().w(320.)
            }
            .min_w(0.)
            .child(
                field(&h.params_search, "params-search", "Search parameters")
                    .on_edit(|s: &mut State| &mut s.hardware.params_search),
            ),
        )
        .child(if has_vehicle && has_file {
            checkbox("params-changed", "Changed only", changed_only, |s| {
                s.hardware.params_changed = !s.hardware.params_changed
            })
        } else {
            el()
        })
        .child(meta_sized(format!("{} shown", rows.len()), 9., muted()));
    let block = col()
        .key("params-list")
        .w_full()
        .min_w(0.)
        .gap(10.)
        .child(filters);
    if rows.is_empty() {
        return block.child(notice(if changed_only {
            "No parameter differs from the loaded file."
        } else {
            "No parameter matches the search."
        }));
    }
    let version = (
        h.params_search.text.clone(),
        changed_only,
        rows.len(),
        file.map(|b| b.path.display().to_string()),
        vehicle.map_or(0, |v| v.params.len()),
    );
    let rows = Rc::new(rows);
    let keys = rows.clone();
    let list = col()
        .key("params-rows")
        .w_full()
        .min_w(0.)
        .growth(GrowthSpec::default())
        .virtual_rows(
            rows.len(),
            if compact { 58. } else { 38. },
            move |_: &State, i| Cow::Owned(format!("param-{}", keys[i].name)),
            move |_: &State, i| param_row(&rows[i], compact, has_vehicle, has_file),
        )
        .virtual_rows_version(version);
    let mut block = block;
    if !compact {
        block = block.child(header(has_vehicle, has_file));
    }
    block.child(
        col()
            .key("params-table")
            .w_full()
            .min_w(0.)
            .max_h(440.)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(list),
    )
}

fn status(s: &State, vehicle: Option<&ParamSet>) -> El {
    let v = &s.vehicle;
    if let Some((received, total)) = v.params_progress.filter(|_| mine(s)) {
        let text = if total == 0 {
            "Asking the vehicle for its parameters…".to_string()
        } else {
            format!(
                "Reading {received} / {total} ({}%)",
                percent(received as u64, total as u64)
            )
        };
        return col()
            .key("params-progress")
            .w_full()
            .min_w(0.)
            .gap(8.)
            .role("status")
            .child(meta_sized(text, 9., fg()))
            .child(bar("params-bar", received as u64, total as u64));
    }
    let Some(set) = vehicle else {
        return notice(match blocked(s) {
            Some(why) => format!("Not fetched. {why}"),
            None => {
                "Not fetched. Fetch reads every parameter from the vehicle; nothing is written."
                    .into()
            }
        })
        .key("params-empty");
    };
    let mut text = format!(
        "{} parameters / {} {} / vehicle {}:{}",
        set.params.len(),
        set.stack,
        set.vehicle,
        set.system_id,
        set.component_id
    );
    if !v.params_missing.is_empty() {
        text.push_str(&format!(" / {} never answered", v.params_missing.len()));
    }
    // Without a live vehicle these are the values of the last fetch, not the vehicle's now.
    if let Some(why) = blocked(s) {
        text.push_str(&format!(" / from the last fetch. {why}"));
    }
    meta_sized(text, 9., fg()).key("params-summary").wrap()
}

fn compare(s: &State, vehicle: Option<&ParamSet>) -> El {
    let h = &s.hardware;
    let id = s.systems.active_id();
    let (pending, baseline) = (h.loading(id), h.baseline(id));
    let loading = pending.is_some();
    let mut block = col()
        .key("params-compare")
        .w_full()
        .min_w(0.)
        .gap(10.)
        .child(labeled(
            "Compare with a .params file",
            field(
                &h.params_file,
                "params-file",
                "Path of a .params file, e.g. an earlier export",
            )
            .on_edit(|s: &mut State| &mut s.hardware.params_file),
        ))
        .child(
            row()
                .gaps(4., 20.)
                .center_y()
                .flex_wrap()
                .child(browse(
                    "params-browse",
                    "Compare with a .params file",
                    &[("QGroundControl parameters", &["params"])],
                    !loading,
                    |s, path| {
                        s.hardware.params_file = TextEdit::new(path.display().to_string());
                    },
                ))
                .child(ghost(
                    "params-load",
                    if loading {
                        "Loading…"
                    } else {
                        "Load and compare"
                    },
                    !loading && !h.params_file.text.trim().is_empty(),
                    hardware::load_baseline,
                ))
                .child(ghost(
                    "params-clear",
                    if loading {
                        "Stop waiting"
                    } else {
                        "Clear comparison"
                    },
                    loading || baseline.is_some(),
                    hardware::clear_baseline,
                )),
        );
    if let Some(l) = pending {
        block = block.child(
            meta_sized(format!("Reading {}…", l.path.display()), 9., muted())
                .key("params-loading")
                .role("status")
                .wrap_anywhere(),
        );
    }
    if let Some(b) = baseline {
        let mut text = format!("{} / {} parameters", b.path.display(), b.set.params.len());
        if let Some(v) = vehicle {
            let changes = crate::vehicle::diff_params(&b.set, v);
            let only_file = changes.iter().filter(|c| c.after.is_none()).count();
            let only_vehicle = changes.iter().filter(|c| c.before.is_none()).count();
            let changed = changes.len() - only_file - only_vehicle;
            text.push_str(&format!(
                " / {changed} changed, {only_file} only in file, {only_vehicle} only on vehicle"
            ));
        }
        block = block.child(code(text, 10., fg()).key("params-baseline").wrap_anywhere());
        if !b.bad.is_empty() {
            let lines: Vec<String> = b.bad.iter().take(12).map(|n| n.to_string()).collect();
            block = block.child(
                body(
                    format!(
                        "{} line(s) did not parse and were skipped: {}{}",
                        b.bad.len(),
                        lines.join(", "),
                        if b.bad.len() > 12 { ", …" } else { "" }
                    ),
                    11.,
                    warn(),
                )
                .wrap(),
            );
        }
    }
    block
}

/// The last export of the active system's parameters, offered for a later comparison.
fn exported(s: &State) -> El {
    let Some(path) = s.hardware.exported(s.systems.active_id()) else {
        return el();
    };
    let shown = path.display().to_string();
    col()
        .key("params-exported")
        .w_full()
        .min_w(0.)
        .gap(6.)
        .child(meta_sized("Exported", 9., muted()))
        .child(code(shown.clone(), 10., fg()).wrap_anywhere())
        .child(ghost(
            "params-compare-export",
            "Compare later with this file",
            true,
            move |s| s.hardware.params_file = rune::TextEdit::new(shown.clone()),
        ))
}

pub(super) fn section(s: &State, vp: &Viewport) -> El {
    let compact = vp.compact();
    let vehicle = s.vehicle.params.as_ref().filter(|_| mine(s));
    let file = s.hardware.baseline(s.systems.active_id());
    let fetching = s.vehicle.params_progress.is_some() && mine(s);
    let actions = row()
        .gaps(8., 20.)
        .center_y()
        .flex_wrap()
        .child(pill(
            "params-fetch",
            if vehicle.is_some() {
                "Fetch again"
            } else {
                "Fetch parameters"
            },
            blocked(s).is_none() && !fetching,
            hardware::fetch,
        ))
        .child(ghost(
            "params-export",
            "Export .params",
            vehicle.is_some() && !fetching,
            hardware::export,
        ))
        .child(ghost(
            "params-open-exports",
            "Open exports folder ↗",
            true,
            hardware::open_exports,
        ));
    let mut block = col()
        .key("vehicle-params")
        .w_full()
        .min_w(0.)
        .gap(14.)
        .pb(24.)
        .child(section_head(
            "Parameters",
            meta("Read-only / fetched, exported and compared, never written").wrap(),
            compact,
        ))
        .child(actions)
        .child(status(s, vehicle))
        .child(exported(s))
        .child(compare(s, vehicle));
    if vehicle.is_some() || file.is_some() {
        block = block.child(table(s, compact, vehicle, file));
    }
    block
}

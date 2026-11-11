//! One PX4 installation per WSL distribution, resolved for bound simulator profiles.
use crate::backend::{Config, Profile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    Sih,
    Gazebo,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Installation {
    pub path: String,
    pub build_target: String,
}
pub(crate) fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
pub(crate) fn validate(i: &Installation) -> Result<(), String> {
    if !i.path.starts_with('/') || i.path.chars().any(|c| matches!(c, '\0' | '\r' | '\n')) {
        return Err(
            "PX4 location must be an absolute Linux folder without control characters.".into(),
        );
    }
    if !["px4_sitl", "px4_sitl_sih"].contains(&i.build_target.as_str()) {
        return Err("Unsupported PX4 SIH build target.".into());
    }
    Ok(())
}
fn old_gazebo_probe(probe: &str) -> bool {
    let Some(q) = probe.strip_prefix("test -x ") else {
        return false;
    };
    let Some(path) = q.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) else {
        return false;
    };
    let path = path.replace("'\\''", "'");
    path.starts_with('/') && path.ends_with("/gz") && quote(&path) == q
}
fn legacy_adoption(p: &Profile) -> Option<Installation> {
    if p.program != "wsl" || p.args.len() != 1 || !p.cwd.is_empty() {
        return None;
    }
    let a = p.args[0].strip_prefix("cd ")?;
    for target in ["px4_sitl_sih", "px4_sitl"] {
        let suffix = format!(" && test -z \"$(pgrep -x px4)\" && make {target} sihsim_quadx");
        if let Some(q) = a.strip_suffix(&suffix) {
            let path = q
                .strip_prefix('\'')?
                .strip_suffix('\'')?
                .replace("'\\''", "'");
            let i = Installation {
                path,
                build_target: target.into(),
            };
            let binary = if target == "px4_sitl" {
                "px4_sitl_default"
            } else {
                target
            };
            if quote(&i.path) == q
                && validate(&i).is_ok()
                && p.probe
                    == format!(
                        "test -x {}",
                        quote(&format!("{}/build/{binary}/bin/px4", i.path))
                    )
            {
                return Some(i);
            }
        }
    }
    None
}
/// Only exact old application-generated profiles become bound; custom commands stay custom.
pub(crate) fn migrate(c: &mut Config) {
    let defaults = crate::backend::defaults();
    for p in &mut c.profiles {
        if p.px4.is_some() {
            continue;
        }
        let Some(d) = defaults
            .profiles
            .iter()
            .find(|d| d.id == p.id && d.px4.is_some())
        else {
            continue;
        };
        if p.program == d.program && p.args == d.args && p.probe == d.probe && p.cwd == d.cwd {
            p.px4 = d.px4;
        } else if p.id == "sih" {
            if let Some(i) = legacy_adoption(p) {
                c.px4_installations
                    .entry(c.distro.to_ascii_lowercase())
                    .or_insert(i);
                p.px4 = Some(Mode::Sih);
            }
        } else if p.id == "gazebo"
            && p.program == "wsl"
            && p.args == d.args
            && p.cwd.is_empty()
            && old_gazebo_probe(&p.probe)
        {
            // Earlier Gazebo adoption changed only its executable probe.
            p.px4 = Some(Mode::Gazebo);
        }
    }
}
/// Generate commands and probes from the same installation record; stored custom profiles
/// pass through unchanged. Missing records use the app-managed location in that distribution.
pub(crate) fn resolve(c: &Config, p: &Profile) -> Profile {
    let mut p = p.clone();
    let Some(mode) = p.px4 else { return p };
    let installation = c.px4_installations.get(&c.distro.to_ascii_lowercase());
    let dir = installation
        .map(|i| quote(&i.path))
        .unwrap_or_else(|| "\"$HOME/.local/share/drone-deck/tools/PX4-Autopilot\"".into());
    let target = installation.map_or("px4_sitl_sih", |i| i.build_target.as_str());
    let guard = "test -z \"$(pgrep -x px4)\"";
    p.program = "wsl".into();
    p.cwd.clear();
    match mode {
        Mode::Sih => {
            p.args = vec![format!("cd {dir} && {guard} && make {target} sihsim_quadx")];
            let binary = if target == "px4_sitl" {
                "px4_sitl_default"
            } else {
                target
            };
            p.probe = format!("test -x {dir}/build/{binary}/bin/px4");
        }
        Mode::Gazebo => {
            p.args = vec![format!(
                "cd {dir} && {guard} && PX4_GZ_WORLD=\"${{DRONE_DECK_GZ_WORLD:-default}}\" make px4_sitl \"gz_${{DRONE_DECK_GZ_MODEL:-x500_mono_cam}}\""
            )];
            p.probe = format!("command -v gz && test -f {dir}/Makefile");
        }
    }
    p
}
#[cfg(test)]
#[path = "px4_tests.rs"]
mod tests;

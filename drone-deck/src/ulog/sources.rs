//! Where flight logs live: PX4 SITL logs inside WSL (read from Windows through
//! `\\wsl.localhost\<distro>\…`, falling back to `\\wsl$\`), vehicle logs downloaded to
//! `<data root>\logs\vehicle\`, and telemetry captures (`.tlog`) under `<data root>\hardware\`.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The list stops growing here; the newest files are kept.
pub(crate) const MAX_FILES: usize = 500;
/// The PX4 checkout the default profiles build in.
pub(crate) const DEFAULT_PX4: &str = "$HOME/.local/share/drone-deck/tools/PX4-Autopilot";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// A PX4 SITL build's `rootfs/log` inside WSL; carries the build folder name.
    Simulation(String),
    /// Downloaded from a vehicle into `<data root>\logs\vehicle\`.
    Vehicle,
    /// A MAVLink telemetry capture.
    Telemetry,
}
impl Source {
    pub(crate) fn label(&self) -> String {
        match self {
            Source::Simulation(build) => format!("Simulation · {build}"),
            Source::Vehicle => "Vehicle download".into(),
            Source::Telemetry => "Telemetry capture".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LogFile {
    pub path: PathBuf,
    pub source: Source,
    pub size: u64,
    pub modified: Option<SystemTime>,
}
impl LogFile {
    pub(crate) fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
    pub(crate) fn is_tlog(&self) -> bool {
        self.path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tlog"))
    }
}

/// What to search. `home` is the Linux home folder when known (the WSL agent reports it);
/// without it every folder under `/home` and `/root` is tried.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Locations {
    pub distro: Option<String>,
    pub home: Option<String>,
    /// Linux paths of PX4 checkouts; may start with `$HOME`, `${HOME}` or `~`.
    pub px4_dirs: Vec<String>,
    pub data_root: PathBuf,
}

/// What a scan found, and which places it could not read.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Discovery {
    pub files: Vec<LogFile>,
    /// Windows paths searched for SITL logs (`…\build\*\rootfs\log`).
    pub searched: Vec<PathBuf>,
    /// Whether the distribution's file system answered at all.
    pub wsl_reachable: bool,
}

/// PX4 checkout paths named by launch profiles: `cd "<…/PX4-Autopilot>"` in the command or the
/// working directory. Always includes the default managed checkout.
pub(crate) fn px4_dirs(profiles: &[crate::backend::Profile]) -> Vec<String> {
    let mut out = vec![DEFAULT_PX4.to_string()];
    for p in profiles.iter().filter(|p| p.program == "wsl") {
        for text in p.args.iter().chain(std::iter::once(&p.cwd)) {
            let mut rest = text.as_str();
            let stop =
                |c: char| matches!(c, '"' | '\'' | ';' | '&' | '|' | '/') || c.is_whitespace();
            while let Some(i) = rest.find("PX4-Autopilot") {
                // The folder name may carry a suffix (`PX4-Autopilot-1.15`).
                let tail = i + "PX4-Autopilot".len();
                let end = rest[tail..].find(stop).map_or(rest.len(), |n| tail + n);
                let start = rest[..i]
                    .char_indices()
                    .rev()
                    .find(|&(_, c)| c != '/' && stop(c))
                    .map_or(0, |(s, c)| s + c.len_utf8());
                let dir = &rest[start..end];
                if (dir.starts_with('/') || dir.starts_with('$') || dir.starts_with('~'))
                    && !out.iter().any(|d| d == dir)
                {
                    out.push(dir.to_string());
                }
                rest = &rest[end..];
            }
        }
    }
    out
}

/// `\\wsl.localhost\<distro>` if it answers, else `\\wsl$\<distro>`.
fn wsl_root(distro: &str) -> Option<PathBuf> {
    [r"\\wsl.localhost\", r"\\wsl$\"]
        .iter()
        .map(|p| PathBuf::from(format!("{p}{distro}")))
        .find(|p| p.join("home").is_dir() || p.join("root").is_dir())
}

/// Linux path → path under `root`.
pub(crate) fn linux_to(root: &Path, linux: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in linux.split('/').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p
}

/// Candidate home folders (Linux paths) when the home is not known.
fn homes(root: &Path, known: Option<&str>) -> Vec<String> {
    if let Some(h) = known {
        return vec![h.to_string()];
    }
    let mut out: Vec<String> = fs::read_dir(root.join("home"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| format!("/home/{}", e.file_name().to_string_lossy()))
        .collect();
    out.sort();
    out.push("/root".into());
    out
}

fn expand(dir: &str, home: &str) -> Option<String> {
    for prefix in ["${HOME}", "$HOME", "~"] {
        if let Some(rest) = dir.strip_prefix(prefix) {
            return Some(format!("{home}{rest}"));
        }
    }
    dir.starts_with('/').then(|| dir.to_string())
}

/// Files with extension `ext` in `dir`, down to `depth` levels of sub-folders.
fn collect(dir: &Path, ext: &str, depth: usize, source: &Source, out: &mut Vec<LogFile>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let Ok(kind) = e.file_type() else { continue };
        let path = e.path();
        if kind.is_dir() {
            if depth > 0 {
                collect(&path, ext, depth - 1, source, out);
            }
        } else if path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case(ext))
        {
            let meta = e.metadata().ok();
            out.push(LogFile {
                path,
                source: source.clone(),
                size: meta.as_ref().map_or(0, |m| m.len()),
                modified: meta.and_then(|m| m.modified().ok()),
            });
        }
    }
}

/// SITL `rootfs\log` folders under one checkout: `<px4>\build\<target>\rootfs\log`.
pub(crate) fn sitl_logs(px4: &Path, discovery: &mut Discovery) {
    let Ok(builds) = fs::read_dir(px4.join("build")) else {
        return;
    };
    for b in builds.flatten() {
        let log = b.path().join("rootfs").join("log");
        if log.is_dir() {
            let build = b.file_name().to_string_lossy().into_owned();
            discovery.searched.push(log.clone());
            collect(
                &log,
                "ulg",
                2,
                &Source::Simulation(build),
                &mut discovery.files,
            );
        }
    }
}

/// Lists every log the locations name, newest first. Runs on the worker thread: UNC paths into
/// WSL can take seconds (and start the distribution).
pub(crate) fn discover(loc: &Locations) -> Discovery {
    let mut d = Discovery::default();
    if let Some(root) = loc
        .distro
        .as_deref()
        .filter(|d| !d.is_empty())
        .and_then(wsl_root)
    {
        d.wsl_reachable = true;
        let mut seen = Vec::new();
        for home in homes(&root, loc.home.as_deref()) {
            for dir in &loc.px4_dirs {
                let Some(linux) = expand(dir, &home) else {
                    continue;
                };
                if seen.contains(&linux) {
                    continue;
                }
                seen.push(linux.clone());
                sitl_logs(&linux_to(&root, &linux), &mut d);
            }
        }
    }
    collect(
        &vehicle_dir(&loc.data_root),
        "ulg",
        2,
        &Source::Vehicle,
        &mut d.files,
    );
    collect(
        &loc.data_root.join("hardware"),
        "tlog",
        2,
        &Source::Telemetry,
        &mut d.files,
    );
    // Captures of the native vehicle link (`vehicle::tlog_dir`).
    collect(
        &loc.data_root.join("logs").join("tlog"),
        "tlog",
        1,
        &Source::Telemetry,
        &mut d.files,
    );
    d.files
        .sort_by(|a, b| b.modified.cmp(&a.modified).then(a.path.cmp(&b.path)));
    d.files.truncate(MAX_FILES);
    d
}

/// `<data root>\logs\vehicle`, where downloaded vehicle logs go.
pub(crate) fn vehicle_dir(data_root: &Path) -> PathBuf {
    data_root.join("logs").join("vehicle")
}

/// `YYYY-MM-DD HH:MM` (UTC) for a Unix time in seconds.
pub(crate) fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rem / 3600,
        (rem / 60) % 60
    )
}
pub(crate) fn modified_utc(t: Option<SystemTime>) -> Option<String> {
    let secs = t?.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
    Some(utc(secs))
}

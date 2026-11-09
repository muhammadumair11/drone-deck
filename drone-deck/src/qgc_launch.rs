//! QGroundControl launch: the opt-in GPU-safe mode (software rendering), kept independent from
//! inherited Qt/RHI settings, and detection of a likely graphics failure after a start.
use crate::State;
use crate::backend::{Event, Profile};
use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

/// QGroundControl's switch for Qt's software OpenGL; `--swrast` is accepted as a spelling of it.
const SOFTWARE: &str = "-swrast";
/// A non-zero exit sooner than this after starting is treated as a likely graphics failure.
pub(crate) const EARLY_EXIT: Duration = Duration::from_secs(20);

pub(crate) fn configure(command: &mut Command, profile: &Profile) -> bool {
    if profile.id != "qgc" || profile.program == "wsl" {
        return false;
    }
    if profile.cwd.is_empty() {
        if let Some(directory) = Path::new(&profile.program).parent() {
            // Match the installed shortcut (Qt's bin/qt.conf has Prefix=..).
            let directory = if directory
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("bin"))
            {
                directory.parent().unwrap_or(directory)
            } else {
                directory
            };
            if !directory.as_os_str().is_empty() {
                command.current_dir(directory);
            }
        }
    }
    let software = gpu_safe(profile);
    if software {
        // AA_UseSoftwareOpenGL alone can still fail Qt Quick's OpenGL initialization.
        command
            .env("QT_QUICK_BACKEND", "software")
            .env("QT_OPENGL", "software")
            .env_remove("QSG_RHI_BACKEND")
            .env_remove("QSG_RHI_PREFER_SOFTWARE_RENDERER");
    }
    software
}

fn is_software(arg: &str) -> bool {
    arg == SOFTWARE || arg == "--swrast"
}

/// Whether the profile launches QGroundControl in GPU-safe mode (software rendering).
pub(crate) fn gpu_safe(profile: &Profile) -> bool {
    profile.args.iter().any(|a| is_software(a))
}

/// `args` with GPU-safe mode switched on (one `-swrast` appended) or off (every spelling
/// removed). Other arguments keep their order.
pub(crate) fn with_gpu_safe(args: &[String], on: bool) -> Vec<String> {
    let mut next: Vec<String> = args.iter().filter(|a| !is_software(a)).cloned().collect();
    if on {
        next.push(SOFTWARE.into());
    }
    next
}

/// Switches GPU-safe mode in the saved QGroundControl profile. Refused while QGroundControl
/// runs, because a running process keeps the renderer it started with. An open launch-profile
/// form gets the same change in its argument lines, so saving the form later keeps it.
pub(crate) fn set_gpu_safe(s: &mut State, on: bool) {
    if crate::systems::running(s, "qgc") {
        s.notice = "Stop QGroundControl before changing GPU-safe mode.".into();
        return;
    }
    let mut next = s.config.clone();
    let Some(p) = next.profiles.iter_mut().find(|p| p.id == "qgc") else {
        s.notice = "QGroundControl has no launch profile yet.".into();
        return;
    };
    if p.program == "wsl" {
        s.notice = "GPU-safe mode applies to the Windows QGroundControl only.".into();
        return;
    }
    p.args = with_gpu_safe(&p.args, on);
    if !s.headless {
        if let Err(e) = crate::backend::save(&next) {
            s.notice = format!("GPU-safe mode could not be saved: {e}");
            return;
        }
    }
    s.config = next;
    let editing_qgc = s
        .config
        .profiles
        .get(s.selected)
        .is_some_and(|p| p.id == "qgc");
    if let Some(form) = s.form.as_mut().filter(|f| !f.creating && editing_qgc) {
        if form.fields.len() > 2 && form.fields[1].text.trim() != "wsl" {
            let lines: Vec<String> = form.fields[2]
                .text
                .lines()
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .map(str::to_owned)
                .collect();
            form.fields[2] = rune::TextEdit::new(&with_gpu_safe(&lines, on).join("\n"));
        }
    }
    if on {
        s.qgc.hint = None;
    }
    let text = if on {
        "QGroundControl GPU-safe mode on: it starts with software rendering."
    } else {
        "QGroundControl GPU-safe mode off: it starts with the graphics card."
    };
    s.notice = format!("{text} Saved to its launch profile.");
    crate::activity::record(s, text);
}

/// Whether a QGroundControl output line reports a failed graphics (RHI, OpenGL, Direct3D,
/// Vulkan) initialisation.
pub(crate) fn graphics_failure(line: &str) -> bool {
    let lower = line.to_lowercase();
    let graphics = lower.split(|c: char| !c.is_ascii_alphanumeric()).any(|t| {
        matches!(t, "rhi" | "graphics" | "gl" | "gles")
            || [
                "libegl",
                "libgles",
                "qrhi",
                "opengl",
                "qopengl",
                "direct3d",
                "d3d",
                "dxgi",
                "vulkan",
                "wgl",
                "egl",
                "glx",
                "qsg",
                "scenegraph",
            ]
            .iter()
            .any(|p| t.starts_with(p))
    });
    graphics
        && [
            "fail",
            "error",
            "unable",
            "could not",
            "cannot",
            "not supported",
            "unsupported",
            "device lost",
            "device removed",
        ]
        .iter()
        .any(|w| lower.contains(w))
}

/// Exit code from a backend `Exited <code>` status.
fn exit_code(status: &str) -> Option<i32> {
    status.strip_prefix("Exited ")?.trim().parse().ok()
}

/// Likely graphics failures of the current QGroundControl run. The hint stays on the
/// Tools & config row until the next start or until GPU-safe mode is switched on.
#[derive(Default)]
pub(crate) struct Watch {
    started: Option<Instant>,
    reported: bool,
    pub(crate) hint: Option<String>,
}

impl Watch {
    /// Follows a QGroundControl status. Returns the notice for a likely graphics failure:
    /// a non-zero exit within `EARLY_EXIT` of starting.
    pub(crate) fn status(&mut self, status: &str, now: Instant, gpu_safe: bool) -> Option<String> {
        if status == "Running" {
            *self = Self {
                started: Some(now),
                ..Self::default()
            };
            return None;
        }
        let started = self.started.take()?;
        let code = exit_code(status).filter(|c| *c != 0)?;
        let after = now.saturating_duration_since(started);
        if after >= EARLY_EXIT || self.reported {
            return None;
        }
        let seconds = after.as_secs().max(1);
        let what = format!("QGroundControl closed {seconds} s after starting (exit code {code})");
        Some(self.report(what, gpu_safe))
    }

    /// Follows a QGroundControl output line. Returns the notice for the first graphics
    /// initialisation failure of a run.
    pub(crate) fn line(&mut self, line: &str, gpu_safe: bool) -> Option<String> {
        if self.reported || !graphics_failure(line) {
            return None;
        }
        let quoted: String = line.trim().chars().take(160).collect();
        let what = format!("QGroundControl reported a graphics failure (\u{201c}{quoted}\u{201d})");
        Some(self.report(what, gpu_safe))
    }

    fn report(&mut self, what: String, gpu_safe: bool) -> String {
        self.reported = true;
        let hint = if gpu_safe {
            format!(
                "{what} even in GPU-safe mode. Check Activity → Process output and update the graphics driver."
            )
        } else {
            format!(
                "{what}. This usually means its graphics (OpenGL / Direct3D) could not start: turn on GPU-safe mode in Tools & config → QGroundControl and start it again."
            )
        };
        self.hint = Some(hint.clone());
        hint
    }
}

/// Feeds a backend event to the QGroundControl watch and sets the notice on a likely
/// graphics failure.
pub(crate) fn observe(s: &mut State, event: &Event) {
    let gpu_safe = s
        .config
        .profiles
        .iter()
        .find(|p| p.id == "qgc")
        .is_some_and(gpu_safe);
    let found = match event {
        Event::Status(id, status) if id == "qgc" => s.qgc.status(status, Instant::now(), gpu_safe),
        Event::Log(id, line) if id == "qgc" => s.qgc.line(line, gpu_safe),
        _ => None,
    };
    if let Some(notice) = found {
        s.notice = notice;
        crate::activity::record(s, "QGroundControl: likely graphics failure detected");
    }
}

#[cfg(test)]
#[path = "qgc_launch_tests.rs"]
mod tests;

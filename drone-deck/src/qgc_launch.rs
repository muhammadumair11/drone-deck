//! Keep QGC's explicit GPU-safe launch independent from inherited Qt/RHI settings.
use crate::backend::Profile;
use std::{path::Path, process::Command};

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
    let software = profile
        .args
        .iter()
        .any(|a| a == "-swrast" || a == "--swrast");
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

#[cfg(test)]
#[path = "qgc_launch_tests.rs"]
mod tests;

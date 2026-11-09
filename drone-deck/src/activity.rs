//! A traceable history of what was requested in this workspace: session starts and stops,
//! tool and profile changes, setup steps. Process output stays in `State::logs`.
use crate::*;
use serde::{Deserialize, Serialize};

const LIMIT: usize = 200;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub time: String,
    pub system: String,
    pub text: String,
}
#[derive(Default)]
pub(crate) struct Activity {
    pub entries: VecDeque<Entry>,
    /// Activity page tab: 0 history, 1 process output, 2 installations.
    pub tab: usize,
    /// Process output filtered to one tool id.
    pub filter: Option<String>,
}
fn path() -> std::path::PathBuf {
    backend::root().join("activity.json")
}
impl Activity {
    pub fn load(headless: bool) -> Self {
        let mut a = Self::default();
        if headless {
            return a;
        }
        if let Some(mut entries) = std::fs::read(path())
            .ok()
            .and_then(|b| serde_json::from_slice::<VecDeque<Entry>>(&b).ok())
        {
            entries.truncate(LIMIT);
            a.entries = entries;
        }
        a
    }
}
/// Adds an entry (newest first) and persists the bounded history.
pub(crate) fn record(s: &mut State, text: impl Into<String>) {
    let system = s.systems.name(s.systems.active).to_string();
    s.activity.entries.push_front(Entry {
        time: now(),
        system,
        text: text.into(),
    });
    s.activity.entries.truncate(LIMIT);
    if !s.headless {
        let _ = std::fs::create_dir_all(backend::root());
        let _ = std::fs::write(
            path(),
            serde_json::to_vec_pretty(&s.activity.entries).unwrap_or_default(),
        );
    }
}
/// Writes the history as JSON next to the workspace and returns its path.
pub(crate) fn export(s: &State) -> Result<std::path::PathBuf, String> {
    let file = backend::root().join("activity-export.json");
    std::fs::create_dir_all(backend::root()).map_err(|e| e.to_string())?;
    let body = serde_json::json!({ "exported": now(), "events": s.activity.entries });
    std::fs::write(&file, serde_json::to_vec_pretty(&body).unwrap()).map_err(|e| e.to_string())?;
    Ok(file)
}

/// Local wall-clock time as `YYYY-MM-DD HH:MM:SS`.
pub(crate) fn now() -> String {
    #[cfg(windows)]
    {
        #[repr(C)]
        #[derive(Default)]
        struct SystemTime {
            year: u16,
            month: u16,
            weekday: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            millis: u16,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetLocalTime(time: *mut SystemTime);
        }
        let mut t = SystemTime::default();
        unsafe { GetLocalTime(&mut t) };
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            t.year, t.month, t.day, t.hour, t.minute, t.second
        )
    }
    #[cfg(not(windows))]
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
        format!("{h:02}:{m:02}:{s:02} UTC")
    }
}

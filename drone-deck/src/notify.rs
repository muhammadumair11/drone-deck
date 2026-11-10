//! Windows toast notifications for long work that ends while Drone Deck is in the background:
//! installation jobs that finish, fail or need the user. Elsewhere (and in headless runs)
//! every call does nothing.
use std::sync::atomic::{AtomicBool, Ordering};

static REGISTERED: AtomicBool = AtomicBool::new(false);

/// Gives the process Drone Deck's notification identity (on Windows its `AppUserModelID`,
/// with the name and icon its toasts show). Call before the window opens.
pub(crate) fn register() {
    let mut identity = rune_notify::Identity::new("local.drone-deck", "Drone Deck");
    if let Some(icon) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("assets").join("drone-deck.png")))
        .filter(|p| p.is_file())
    {
        identity = identity.icon(icon);
    }
    match rune_notify::register(&identity) {
        Ok(()) => REGISTERED.store(true, Ordering::Relaxed),
        Err(e) => crate::diagnostics::log("warn", &format!("Notifications unavailable: {e}")),
    }
}

/// The toast for an installation job whose status changed to `status`, or `None` when that
/// status is not worth interrupting for (queued, running, cancelled by the user).
pub(crate) fn job_message(tool: &str, status: &str, message: &str) -> Option<(String, String)> {
    let title = match status {
        "Ready" | "Existing installation" => format!("{tool} is ready"),
        "Failed" => format!("{tool} installation failed"),
        "Interrupted" => format!("{tool} installation was interrupted"),
        "Needs input" | "Needs restart" | "Needs attention" => format!("{tool}: {}", status.to_lowercase()),
        _ => return None,
    };
    let body = if message.trim().is_empty() {
        "Open Drone Deck → Activity → Installations for details.".to_string()
    } else {
        message.trim().to_string()
    };
    Some((title, body))
}

/// Posts a toast for an installation job that changed status (see `job_message`).
pub(crate) fn job(id: &str, tool: &str, status: &str, message: &str) {
    if !REGISTERED.load(Ordering::Relaxed) {
        return;
    }
    if let Some((title, body)) = job_message(tool, status, message) {
        rune_notify::post(&rune_notify::Notification::new(format!("job-{id}"), title).body(body));
    }
}

#[cfg(test)]
mod tests {
    use super::job_message;

    #[test]
    fn only_finished_failed_or_blocked_jobs_notify() {
        let (title, body) = job_message("PX4 SIH", "Ready", "").unwrap();
        assert_eq!(title, "PX4 SIH is ready");
        assert!(body.contains("Activity"));
        assert_eq!(job_message("PX4 SIH", "Failed", "make exited 2").unwrap().1, "make exited 2");
        assert!(job_message("PX4 SIH", "Needs restart", "").unwrap().0.contains("needs restart"));
        for quiet in ["Queued", "Running", "Cancelled", "Cancelling"] {
            assert!(job_message("PX4 SIH", quiet, "").is_none(), "{quiet}");
        }
    }
}

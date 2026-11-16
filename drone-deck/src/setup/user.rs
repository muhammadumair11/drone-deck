//! The Linux user a distribution needs before tools install, and keeping the setup view current.
//!
//! A distribution installed with `--no-launch` never ran Windows' first-run account prompt, so it
//! has only root; Linux tools refuse to install as root. `Initialize Linux user` (init-user.ps1)
//! creates the account in its own console. This module knows when that is missing, rescans when
//! a WSL or distribution job changes state, and watches for the account to appear.
use super::*;

/// How often the host is rescanned while waiting for the Linux user to be created.
const WATCH_EVERY: Duration = Duration::from_secs(3);
/// How long Drone Deck keeps watching after "Initialize Linux user" before giving up.
const WATCH_FOR: Duration = Duration::from_secs(600);

fn distro_without_user(host: &Option<Host>, name: &str) -> bool {
    host.as_ref().is_some_and(|h| {
        h.distros.iter().any(|d| {
            d.name.eq_ignore_ascii_case(name) && d.version == 2 && d.regular_user == Some(false)
        })
    })
}

/// The distribution Drone Deck works in is installed as WSL 2 but has only root.
pub(crate) fn missing(s: &State) -> bool {
    distro_without_user(&s.setup.host, &s.config.distro)
}

/// The distribution typed on the setup page has only root.
pub(crate) fn typed_missing(r: &Setup) -> bool {
    distro_without_user(&r.host, r.distro.text.trim())
}

/// A Linux tool cannot be installed yet: its environment has no regular user.
pub(crate) fn blocks_install(s: &State) -> bool {
    s.setup
        .recipes
        .get(s.setup.selected)
        .is_some_and(|r| r.target == "WSL")
        && typed_missing(&s.setup)
}

pub(crate) const BLOCKED: &str = "Create the Linux user first: this environment has only root, and Linux tools are not installed as root. Use Initialize Linux user in the Linux environment step.";

/// `Initialize Linux user` was started: rescan until the account exists.
pub(super) fn begin_watch(r: &mut Setup) {
    let now = Instant::now();
    r.user_watch = Some((now, now));
}

/// A scan result arrived: stop watching once the account exists, else wait before the next scan
/// (measured from now, so a slow scan never leaves the host permanently being scanned).
pub(super) fn saw_host(r: &mut Setup) {
    if let Some((since, _)) = r.user_watch {
        r.user_watch = if typed_missing(r) {
            Some((since, Instant::now()))
        } else {
            None
        };
    }
}

/// A WSL or distribution job changed state: the host snapshot is out of date.
pub(super) fn job_changed(r: &mut Setup, tool: &str, status: &str) {
    if matches!(tool, "wsl" | "distro")
        && matches!(
            status,
            "Ready" | "Needs input" | "Needs restart" | "Existing installation" | "Failed"
        )
    {
        r.rescan_wanted = true;
    }
}

/// Starts the rescans that `job_changed` and `begin_watch` ask for, when nothing else is running.
pub(super) fn tick(s: &mut State) -> bool {
    let scan = !s.headless && due(s);
    if scan {
        request(s, "scan");
    }
    scan
}

/// Whether a rescan is due now. Never while a request is out, a queue is installing, or the
/// wizard's own check is waiting for its scan: a background rescan must not starve that wait.
fn due(s: &mut State) -> bool {
    if s.setup.pending
        || s.setup.batch.is_some()
        || !s.setup.queue.is_empty()
        || s.guided.check != crate::guided::Check::Idle
    {
        return false;
    }
    let mut scan = std::mem::take(&mut s.setup.rescan_wanted);
    if let Some((since, last)) = s.setup.user_watch {
        if since.elapsed() > WATCH_FOR {
            s.setup.user_watch = None;
        } else if last.elapsed() >= WATCH_EVERY {
            s.setup.user_watch = Some((since, Instant::now()));
            scan = true;
        }
    }
    scan
}

#[cfg(test)]
#[path = "user_tests.rs"]
mod tests;

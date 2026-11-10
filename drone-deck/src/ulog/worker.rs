//! The flight-log worker thread: scans sources and analyzes files off the UI thread. Requests
//! the user made (a selected log) go ahead of background ones (durations for the list).
use super::sources::{Discovery, Locations, discover};
use super::{Report, analyze_file};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

pub(crate) enum Job {
    Scan(Locations),
    Analyze {
        path: PathBuf,
        urgent: bool,
    },
    /// Moves a queued analysis to the front; does nothing once it has started or finished, so
    /// a file is never read twice for one request.
    Promote(PathBuf),
}
pub(crate) enum Done {
    Scanned(Discovery),
    Analyzed(PathBuf, Result<Report, String>),
}

pub(crate) struct Worker {
    tx: Sender<Job>,
    pub rx: Receiver<Done>,
}
impl Worker {
    pub(crate) fn spawn() -> Worker {
        let (tx, jobs) = channel();
        let (done, rx) = channel();
        std::thread::Builder::new()
            .name("flight-logs".into())
            .spawn(move || run(jobs, done))
            .expect("spawn flight-log worker");
        Worker { tx, rx }
    }
    pub(crate) fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }
}

fn queue(q: &mut VecDeque<Job>, job: Job) {
    match job {
        Job::Scan(_) => q.push_front(job),
        Job::Analyze { path, urgent } => {
            q.retain(|j| !matches!(j, Job::Analyze { path: p, .. } if *p == path));
            let job = Job::Analyze { path, urgent };
            if urgent {
                q.push_front(job)
            } else {
                q.push_back(job)
            }
        }
        Job::Promote(path) => {
            let at = q
                .iter()
                .position(|j| matches!(j, Job::Analyze { path: p, .. } if *p == path));
            if let Some(job) = at.and_then(|i| q.remove(i)) {
                q.push_front(job);
            }
        }
    }
}

/// Runs one job. A panic (a reader bug on some file) becomes that job's error instead of
/// killing the thread, which would leave the file "Reading…" and the list unscanned forever.
fn work(job: Job) -> Option<Done> {
    match job {
        Job::Scan(loc) => Some(Done::Scanned(
            catch_unwind(AssertUnwindSafe(|| discover(&loc))).unwrap_or_default(),
        )),
        Job::Analyze { path, .. } => {
            let r = catch_unwind(AssertUnwindSafe(|| analyze_file(&path))).unwrap_or_else(|_| {
                Err(format!(
                    "Drone Deck could not read {} (internal reader error).",
                    path.display()
                ))
            });
            Some(Done::Analyzed(path, r))
        }
        Job::Promote(_) => None,
    }
}

fn run(jobs: Receiver<Job>, done: Sender<Done>) {
    let mut q = VecDeque::new();
    loop {
        if q.is_empty() {
            match jobs.recv() {
                Ok(job) => queue(&mut q, job),
                Err(_) => return,
            }
        }
        while let Ok(job) = jobs.try_recv() {
            queue(&mut q, job);
        }
        let Some(job) = q.pop_front() else { continue };
        if let Some(out) = work(job)
            && done.send(out).is_err()
        {
            return;
        }
    }
}

#[cfg(test)]
pub(super) fn order(jobs: Vec<Job>) -> Vec<PathBuf> {
    let mut q = VecDeque::new();
    for j in jobs {
        queue(&mut q, j);
    }
    q.into_iter()
        .filter_map(|j| match j {
            Job::Analyze { path, .. } => Some(path),
            _ => None,
        })
        .collect()
}

use super::*;
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// A child that stays alive for about a minute unless something kills it.
#[cfg(windows)]
fn sleeper() -> Child {
    use std::os::windows::process::CommandExt;
    Command::new("ping.exe")
        .args(["-n", "60", "127.0.0.1"])
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("ping.exe starts")
}

#[cfg(windows)]
#[test]
fn closing_the_job_kills_its_children() {
    let job = Job::new().expect("job object");
    let mut child = sleeper();
    job.assign(&child).expect("assign");
    assert_eq!(job.active_processes(), Some(1));
    assert!(
        child.try_wait().unwrap().is_none(),
        "child runs while the job is open"
    );
    // The child pings for about a minute; it must end within seconds of the job closing.
    // (Windows ends job processes with exit code 0, so the code proves nothing.)
    drop(job);
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "child outlived its job");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `cmd.exe` starting `ping.exe` at once: the grandchild exists before any assignment made
/// after the spawn could run.
#[cfg(windows)]
fn launcher() -> Command {
    let mut c = Command::new("cmd.exe");
    c.args(["/c", "ping.exe -n 60 127.0.0.1 >nul"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

#[cfg(windows)]
#[test]
fn spawned_children_and_their_children_start_inside_the_job() {
    let job = Job::new().expect("job object");
    let (mut child, assigned) = job
        .spawn(&mut launcher(), 0x08000000)
        .expect("cmd.exe starts");
    assigned.expect("assigned before it ran");
    // cmd.exe runs (it was resumed) and its ping.exe joins the job with it.
    let deadline = Instant::now() + Duration::from_secs(10);
    while job.active_processes().unwrap_or(0) < 2 {
        assert!(
            Instant::now() < deadline,
            "grandchild not in the job: {:?}",
            job.active_processes()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(job);
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "child outlived its job");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(windows)]
#[test]
fn the_shared_job_accepts_children() {
    let mut c = Command::new("ping.exe");
    c.args(["-n", "60", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let (mut child, assigned) = spawn(&mut c, 0x08000000).expect("ping.exe starts");
    assigned.expect("assign to the shared job");
    assert!(child.try_wait().unwrap().is_none(), "the child was resumed");
    assert!(describe().contains("kill on close"), "{}", describe());
    child.kill().unwrap();
    child.wait().unwrap();
}

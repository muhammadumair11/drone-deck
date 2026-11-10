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

#[cfg(windows)]
mod process {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        pub fn GetExitCodeProcess(process: isize, code: *mut u32) -> i32;
        pub fn TerminateProcess(process: isize, code: u32) -> i32;
        pub fn IsProcessInJob(process: isize, job: isize, result: *mut i32) -> i32;
    }
    /// Query, terminate and wait rights.
    pub const ACCESS: u32 = 0x1000 | 0x0001 | 0x0010_0000;
    pub const STILL_ACTIVE: u32 = 259;
}

/// A helper (PowerShell) that starts `ping.exe` the way the setup and Studio helpers detach
/// their workers (`Start-Process`), prints the worker's id and then waits.
#[cfg(windows)]
fn detaching_helper() -> Command {
    let mut c = Command::new("powershell.exe");
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "$p = Start-Process ping.exe -ArgumentList '-n','30','127.0.0.1' -WindowStyle Hidden \
         -PassThru; [Console]::Out.WriteLine($p.Id); [Console]::Out.Flush(); Start-Sleep 30",
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    c
}

#[cfg(windows)]
#[test]
fn a_helper_dies_with_its_job_while_the_worker_it_starts_breaks_away() {
    use std::io::BufRead;
    let job = Job::for_helpers().expect("helper job");
    let (mut helper, assigned) = job
        .spawn(&mut detaching_helper(), 0x08000000)
        .expect("powershell.exe starts");
    assigned.expect("the helper is in the job");
    let mut line = String::new();
    std::io::BufReader::new(helper.stdout.take().unwrap())
        .read_line(&mut line)
        .expect("the helper prints its worker's id");
    let pid: u32 = line.trim().parse().expect("a process id");
    let worker = unsafe { process::OpenProcess(process::ACCESS, 0, pid) };
    assert_ne!(worker, 0, "the worker {pid} runs");
    let mut inside = 1;
    let ok = unsafe { process::IsProcessInJob(worker, job.handle, &mut inside) };
    assert!(ok != 0 && inside == 0, "the worker broke away from the job");
    drop(job);
    let deadline = Instant::now() + Duration::from_secs(10);
    while helper.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the helper outlived its job");
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut code = 0;
    unsafe { process::GetExitCodeProcess(worker, &mut code) };
    let survived = code == process::STILL_ACTIVE;
    unsafe {
        process::TerminateProcess(worker, 1);
        ffi::CloseHandle(worker);
    }
    assert!(survived, "the detached worker survives the helper's job");
}

#[cfg(windows)]
#[test]
fn the_shared_helper_job_accepts_helpers_and_is_described() {
    let mut c = Command::new("ping.exe");
    c.args(["-n", "60", "127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let (mut child, assigned) = spawn_helper(&mut c, 0x08000000).expect("ping.exe starts");
    assigned.expect("assign to the helper job");
    assert!(child.try_wait().unwrap().is_none(), "the child was resumed");
    let text = describe();
    assert!(text.starts_with("Job object: kill on close"), "{text}");
    assert!(
        text.contains("Helper job: kill on close, children break away; "),
        "{text}"
    );
    child.kill().unwrap();
    child.wait().unwrap();
}

use super::*;
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    process::{Command, Stdio},
    thread,
};

pub(super) fn request(value: Value, tx: Sender<Result<Value, String>>) {
    thread::spawn(move || {
        let result = (|| -> Result<Value, String> {
            let script = crate::backend::scripts().join("setup/control.ps1");
            if !script.exists() {
                return Err("Setup helper scripts are missing. Rebuild with Build.cmd so runtime/ sits beside the app.".into());
            }
            let shell =
                PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
                    .join("System32/WindowsPowerShell/v1.0/powershell.exe");
            let mut command = Command::new(shell);
            command
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(script)
                .arg("-Root")
                .arg(backend::root())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            // The helper dies with Drone Deck; the install worker it detaches breaks away from
            // the helper job and keeps running, and the app re-attaches to it by its status files.
            let (mut child, _) =
                crate::winjob::spawn_helper(&mut command, 0x08000000).map_err(|e| e.to_string())?;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(value.to_string().as_bytes())
                .map_err(|e| e.to_string())?;
            let result = child.wait_with_output().map_err(|e| e.to_string())?;
            let output = String::from_utf8_lossy(&result.stdout);
            let line = output
                .lines()
                .rev()
                .find(|l| l.trim_start().starts_with('{'))
                .ok_or_else(|| {
                    format!(
                        "Setup helper failed: {}",
                        String::from_utf8_lossy(&result.stderr)
                    )
                })?;
            serde_json::from_str(line.trim_start_matches('\u{feff}'))
                .map_err(|e| format!("Invalid setup response: {e}"))
        })();
        let _ = tx.send(result);
    });
}
#[cfg(windows)]
pub(crate) fn process_alive(pid: u32, ticks: u64) -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn GetProcessTimes(
            handle: isize,
            created: *mut u64,
            exit: *mut u64,
            kernel: *mut u64,
            user: *mut u64,
        ) -> i32;
        fn GetExitCodeProcess(handle: isize, code: *mut u32) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    if pid == 0 || ticks == 0 {
        return false;
    }
    unsafe {
        let handle = OpenProcess(0x1000, 0, pid);
        if handle == 0 {
            return false;
        }
        let (mut created, mut exit, mut kernel, mut user, mut code) = (0, 0, 0, 0, 0);
        let alive = GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user) != 0
            && GetExitCodeProcess(handle, &mut code) != 0
            && code == 259
            && created == ticks;
        CloseHandle(handle);
        alive
    }
}
#[cfg(not(windows))]
pub(crate) fn process_alive(_: u32, _: u64) -> bool {
    false
}
pub(super) fn read_jobs(folder: &std::path::Path, selected: Option<&str>) -> Vec<Job> {
    let mut paths: Vec<_> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    paths.sort();
    paths.reverse();
    paths.into_iter().take(50).filter_map(|path|{
        let state_path=path.join("state.json");
        let bytes=fs::read(&state_path).ok()?;
        let mut job:Job=serde_json::from_slice(&bytes).ok()?;
        if path.join("configuration-applied").is_file(){job.configure_after=false;}
        job.alive=process_alive(job.pid,job.process_ticks);
        if job.alive {
            if let Ok(message)=fs::read_to_string(path.join("output.log.waiting")) {
                job.status="Needs attention".into();job.message=message;
            }
        }
        let old=state_path.metadata().ok()?.modified().ok()?.elapsed().ok()?.as_secs()>20;
        if !job.alive&&old&&["Queued","Running","Downloading","Installing","Checking"].contains(&job.status.as_str()) {
            job.status="Interrupted".into();job.message="Setup worker is no longer running. Review the log and retry; completed work is rechecked.".into();
        }
        if selected==Some(job.id.as_str()) {
            if let Ok(mut file)=fs::File::open(path.join("output.log")) {
                let length=file.metadata().ok()?.len();let _=file.seek(SeekFrom::Start(length.saturating_sub(16384)));
                let mut bytes=Vec::new();let _=file.read_to_end(&mut bytes);let text=String::from_utf8_lossy(&bytes);
                job.log_tail=text.lines().rev().take(35).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
            }
        }
        Some(job)
    }).collect()
}

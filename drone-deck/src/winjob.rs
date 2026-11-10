//! Windows Job Objects that tie Drone Deck's Windows children to its lifetime. Both jobs are
//! created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and their handles stay open for the life
//! of the process, so when Drone Deck exits for any reason (normal close, panic, crash, Task
//! Manager) Windows closes the handles and terminates what is in them. Children start
//! suspended and run only once they are in their job.
//!
//! - The process job ([`spawn`]) owns Windows-native profiles and `wsl.exe` agents: they and
//!   everything they start die with Drone Deck. Killing `wsl.exe` does not kill the Linux side;
//!   its stdin closes and the agent shuts down on EOF (owned Linux processes are detached by
//!   design and listed by the next agent).
//! - The helper job ([`spawn_helper`]) owns the PowerShell helpers (setup, Studio, opening a
//!   file with its app). It also has `JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK`: the helper dies
//!   with Drone Deck, but whatever it starts is outside the job, so the install and model
//!   download workers it detaches (and the apps it opens) survive a restart on purpose.
use std::{
    io,
    process::{Child, Command},
    sync::OnceLock,
};

#[cfg(test)]
#[path = "winjob_tests.rs"]
mod tests;

const KILL_ON_JOB_CLOSE: u32 = 0x2000;
const SILENT_BREAKAWAY_OK: u32 = 0x1000;

/// A job object handle. Dropping it closes the handle, which kills the processes in the job.
pub(crate) struct Job {
    #[cfg(windows)]
    handle: isize,
}

#[cfg(windows)]
mod ffi {
    pub const CREATE_SUSPENDED: u32 = 0x4;
    pub const BASIC_ACCOUNTING: i32 = 1;
    pub const EXTENDED_LIMIT: i32 = 9;

    #[repr(C)]
    #[derive(Default)]
    pub struct BasicLimit {
        pub per_process_user_time: i64,
        pub per_job_user_time: i64,
        pub limit_flags: u32,
        pub minimum_working_set: usize,
        pub maximum_working_set: usize,
        pub active_process_limit: u32,
        pub affinity: usize,
        pub priority_class: u32,
        pub scheduling_class: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    pub struct IoCounters {
        pub counts: [u64; 6],
    }
    #[repr(C)]
    #[derive(Default)]
    pub struct ExtendedLimit {
        pub basic: BasicLimit,
        pub io: IoCounters,
        pub process_memory_limit: usize,
        pub job_memory_limit: usize,
        pub peak_process_memory_used: usize,
        pub peak_job_memory_used: usize,
    }
    #[repr(C)]
    #[derive(Default)]
    pub struct BasicAccounting {
        pub total_user_time: i64,
        pub total_kernel_time: i64,
        pub this_period_user_time: i64,
        pub this_period_kernel_time: i64,
        pub page_faults: u32,
        pub total_processes: u32,
        pub active_processes: u32,
        pub terminated_processes: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn CreateJobObjectW(attributes: *const core::ffi::c_void, name: *const u16) -> isize;
        pub fn SetInformationJobObject(
            job: isize,
            class: i32,
            info: *const core::ffi::c_void,
            length: u32,
        ) -> i32;
        pub fn QueryInformationJobObject(
            job: isize,
            class: i32,
            info: *mut core::ffi::c_void,
            length: u32,
            returned: *mut u32,
        ) -> i32;
        pub fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
        pub fn CloseHandle(handle: isize) -> i32;
    }
    #[link(name = "ntdll", kind = "raw-dylib")]
    unsafe extern "system" {
        /// Resumes every thread of a process (an NTSTATUS; negative is a failure).
        pub fn NtResumeProcess(process: isize) -> i32;
    }
}

impl Job {
    /// Creates an unnamed job whose processes, and their descendants, are killed when its last
    /// handle closes.
    pub(crate) fn new() -> io::Result<Self> {
        Self::with_limits(KILL_ON_JOB_CLOSE)
    }
    /// Creates a job whose own processes are killed when its last handle closes, while the
    /// processes they start are created outside it (silent breakaway) and outlive it.
    pub(crate) fn for_helpers() -> io::Result<Self> {
        Self::with_limits(KILL_ON_JOB_CLOSE | SILENT_BREAKAWAY_OK)
    }
    fn with_limits(flags: u32) -> io::Result<Self> {
        #[cfg(windows)]
        {
            let handle = unsafe { ffi::CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle == 0 {
                return Err(io::Error::last_os_error());
            }
            let job = Self { handle };
            let limits = ffi::ExtendedLimit {
                basic: ffi::BasicLimit {
                    limit_flags: flags,
                    ..Default::default()
                },
                ..Default::default()
            };
            let ok = unsafe {
                ffi::SetInformationJobObject(
                    job.handle,
                    ffi::EXTENDED_LIMIT,
                    (&limits as *const ffi::ExtendedLimit).cast(),
                    size_of::<ffi::ExtendedLimit>() as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
        #[cfg(not(windows))]
        {
            let _ = flags;
            Ok(Self {})
        }
    }
    /// Puts `child` (and the processes it starts later) in this job.
    pub(crate) fn assign(&self, child: &Child) -> io::Result<()> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            let process = child.as_raw_handle() as isize;
            if unsafe { ffi::AssignProcessToJobObject(self.handle, process) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            Ok(())
        }
    }
    /// Starts `command` suspended with `flags` (its other creation flags), puts it in this job
    /// and only then lets it run, so every process it starts is in the job too. The child comes
    /// back with the assignment result: on a failed assignment it runs outside the job. Failing
    /// to start (or to resume) it is the error; a child that cannot be resumed is killed.
    pub(crate) fn spawn(
        &self,
        command: &mut Command,
        flags: u32,
    ) -> io::Result<(Child, io::Result<()>)> {
        #[cfg(windows)]
        {
            use std::os::windows::{io::AsRawHandle, process::CommandExt};
            command.creation_flags(flags | ffi::CREATE_SUSPENDED);
            let mut child = command.spawn()?;
            let assigned = self.assign(&child);
            let status = unsafe { ffi::NtResumeProcess(child.as_raw_handle() as isize) };
            if status < 0 {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::other(format!(
                    "the process could not be resumed (NTSTATUS {status:#x})"
                )));
            }
            Ok((child, assigned))
        }
        #[cfg(not(windows))]
        {
            let _ = flags;
            Ok((command.spawn()?, Ok(())))
        }
    }
    /// Processes currently alive in the job, descendants included.
    pub(crate) fn active_processes(&self) -> Option<u32> {
        #[cfg(windows)]
        {
            let mut info = ffi::BasicAccounting::default();
            let ok = unsafe {
                ffi::QueryInformationJobObject(
                    self.handle,
                    ffi::BASIC_ACCOUNTING,
                    (&mut info as *mut ffi::BasicAccounting).cast(),
                    size_of::<ffi::BasicAccounting>() as u32,
                    std::ptr::null_mut(),
                )
            };
            (ok != 0).then_some(info.active_processes)
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe { ffi::CloseHandle(self.handle) };
    }
}

/// The process-wide jobs. They live in statics, so they are never dropped: the handles close
/// only when Drone Deck itself exits.
fn shared() -> Result<&'static Job, &'static str> {
    static JOB: OnceLock<Result<Job, String>> = OnceLock::new();
    JOB.get_or_init(|| Job::new().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(String::as_str)
}
fn helpers() -> Result<&'static Job, &'static str> {
    static JOB: OnceLock<Result<Job, String>> = OnceLock::new();
    JOB.get_or_init(|| Job::for_helpers().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(String::as_str)
}

/// Starts a Windows-native child in the process-wide job ([`Job::spawn`]), so neither it nor
/// anything it starts can outlive Drone Deck. A failed assignment is written to the app log
/// and returned beside the running child.
pub(crate) fn spawn(command: &mut Command, flags: u32) -> io::Result<(Child, io::Result<()>)> {
    spawn_in(shared(), command, flags)
}

/// Starts a helper script in the helper job: the helper dies with Drone Deck, while the
/// processes it starts break away, so a worker it detaches keeps running after a restart.
/// A failed assignment is written to the app log and returned beside the running child.
pub(crate) fn spawn_helper(
    command: &mut Command,
    flags: u32,
) -> io::Result<(Child, io::Result<()>)> {
    spawn_in(helpers(), command, flags)
}

fn spawn_in(
    job: Result<&Job, &str>,
    command: &mut Command,
    flags: u32,
) -> io::Result<(Child, io::Result<()>)> {
    let (child, assigned) = match job {
        Ok(job) => job.spawn(command, flags)?,
        Err(e) => {
            #[cfg(windows)]
            std::os::windows::process::CommandExt::creation_flags(command, flags);
            (
                command.spawn()?,
                Err(io::Error::other(format!("job object unavailable: {e}"))),
            )
        }
    };
    if let Err(e) = &assigned {
        crate::diagnostics::log(
            "warn",
            &format!(
                "Process {} was not added to the job object: {e}",
                child.id()
            ),
        );
    }
    Ok((child, assigned))
}

/// One line for diagnostics: both jobs, their limits and how many processes each holds now.
pub(crate) fn describe() -> String {
    if !cfg!(windows) {
        return "Job object: not used on this platform".into();
    }
    let state = |job: Result<&Job, &str>, limits: &str| match job {
        Ok(job) => match job.active_processes() {
            Some(n) => format!("{limits}; {n} active process(es)"),
            None => format!("{limits}; process count unavailable"),
        },
        Err(e) => format!("unavailable ({e})"),
    };
    format!(
        "Job object: {}. Helper job: {}",
        state(shared(), "kill on close"),
        state(helpers(), "kill on close, children break away")
    )
}

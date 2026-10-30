//! Windows process handling for `timeout`. The command is placed in a Job Object with
//! `KILL_ON_JOB_CLOSE`, so expiry terminates the whole process tree with `TerminateJobObject`.
//! Windows has no signals, so every signal other than 0 has that effect.

use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::{Child, Command};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// A running command under supervision, together with its job.
pub struct Monitored {
    child: Child,
    job: HANDLE,
}

pub fn spawn(program: &Path, args: &[String], _foreground: bool) -> io::Result<Monitored> {
    let job = create_job()?;
    let child = match Command::new(program).args(args).spawn() {
        Ok(child) => child,
        Err(error) => {
            // SAFETY: `job` was returned by CreateJobObjectW and is closed exactly once.
            unsafe { CloseHandle(job) };
            return Err(error);
        }
    };
    // SAFETY: both handles are valid. A failure leaves the command outside the job, and
    // `send` then falls back to terminating the command itself.
    unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) };
    Ok(Monitored { child, job })
}

/// Creates a job whose processes all end when its last handle closes.
fn create_job() -> io::Result<HANDLE> {
    // SAFETY: null attributes and name are allowed; the result is checked below.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: an all-zero struct is a valid starting value for this plain C structure.
    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: `info` is a live JOBOBJECT_EXTENDED_LIMIT_INFORMATION of the stated size.
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if ok == 0 {
        let error = io::Error::last_os_error();
        // SAFETY: `job` is valid and closed exactly once on this path.
        unsafe { CloseHandle(job) };
        return Err(error);
    }
    Ok(job)
}

impl Monitored {
    pub fn child(&mut self) -> &mut Child {
        &mut self.child
    }

    /// Terminates the whole job. Signal 0 sends nothing.
    pub fn send(&mut self, signal: i32) {
        if signal == 0 {
            return;
        }
        // SAFETY: `job` is a valid job handle owned by this value.
        unsafe { TerminateJobObject(self.job, 1) };
        let _ = self.child.kill();
    }
}

impl Drop for Monitored {
    fn drop(&mut self) {
        // SAFETY: `job` is valid and closed exactly once, here.
        unsafe { CloseHandle(self.job) };
    }
}

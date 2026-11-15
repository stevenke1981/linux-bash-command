use std::{
    io,
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        Threading::{OpenThread, ResumeThread, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME},
    },
};

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}
pub(super) struct Group {
    job: OwnedHandle,
}

fn job_object() -> io::Result<OwnedHandle> {
    // SAFETY: null pointers request a new unnamed job with default security.
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    let job = OwnedHandle(raw);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: limits has the exact structure and length required by this information class.
    if unsafe {
        SetInformationJobObject(
            raw,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(job)
}

fn primary_thread(pid: u32) -> io::Result<OwnedHandle> {
    // SAFETY: snapshot is an owned kernel handle and entry size is initialized as required.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let snapshot = OwnedHandle(snapshot);
    let mut entry = THREADENTRY32 {
        dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut more = unsafe { Thread32First(snapshot.0, &mut entry) };
    while more != 0 {
        if entry.th32OwnerProcessID == pid {
            // The new process was created suspended and has not created further threads.
            let handle = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            return if handle.is_null() {
                Err(io::Error::last_os_error())
            } else {
                Ok(OwnedHandle(handle))
            };
        }
        more = unsafe { Thread32Next(snapshot.0, &mut entry) };
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "cannot locate suspended child thread",
    ))
}

pub(super) fn spawn(
    command: &mut Command,
    _foreground: bool,
) -> Result<(Child, Group), super::SpawnError> {
    let job = job_object().map_err(super::SpawnError::Monitor)?;
    command.creation_flags(CREATE_SUSPENDED);
    let mut child = command.spawn().map_err(super::SpawnError::Command)?;
    // SAFETY: both handles are live, and the child has not yet been resumed.
    if unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle().cast()) } == 0 {
        let e = io::Error::last_os_error();
        let _ = child.kill();
        let _ = child.wait();
        return Err(super::SpawnError::Monitor(e));
    }
    let thread = match primary_thread(child.id()) {
        Ok(t) => t,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(super::SpawnError::Monitor(e));
        }
    };
    // SAFETY: this is the owned handle for the suspended initial thread.
    if unsafe { ResumeThread(thread.0) } == u32::MAX {
        let e = io::Error::last_os_error();
        let _ = child.kill();
        let _ = child.wait();
        return Err(super::SpawnError::Monitor(e));
    }
    Ok((child, Group { job }))
}
impl Group {
    pub(super) fn signal(&self, _child: &mut Child, signal: i32) -> io::Result<()> {
        if signal == 0 {
            return Ok(());
        }
        // SAFETY: the job handle remains valid for this Group's lifetime.
        if unsafe { TerminateJobObject(self.job.0, 1) } != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

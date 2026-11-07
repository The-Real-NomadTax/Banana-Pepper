//! Process ownership, process-tree cancellation, events and bounded session commands.
use super::*;
use std::{
    collections::VecDeque,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
pub struct ProcessSession {
    child: Child,
    events: VecDeque<SessionEvent>,
    exit: Option<i32>,
    started: Instant,
    timeout: Option<Duration>,
    timed_out: bool,
    #[allow(dead_code)]
    endpoint: Option<std::path::PathBuf>,
    #[cfg(windows)]
    job: std::os::windows::io::OwnedHandle,
}
impl ProcessSession {
    pub fn spawn(
        command: &mut Command,
        timeout_ms: Option<u64>,
    ) -> Result<Box<dyn SessionHandle>, String> {
        Self::spawn_controlled(command, timeout_ms, None)
    }
    pub fn spawn_controlled(
        command: &mut Command,
        timeout_ms: Option<u64>,
        endpoint: Option<std::path::PathBuf>,
    ) -> Result<Box<dyn SessionHandle>, String> {
        command.stdin(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            use std::os::unix::process::CommandExt;
            let parent = std::process::id();
            unsafe {
                command.pre_exec(move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::getppid() as u32 != parent {
                        libc::_exit(125)
                    }
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        let (child, job) = spawn_windows(command)?;
        #[cfg(not(windows))]
        let child = command.spawn().map_err(|e| e.to_string())?;
        Ok(Box::new(Self {
            child,
            events: VecDeque::from([SessionEvent::Started]),
            exit: None,
            started: Instant::now(),
            timeout: timeout_ms.map(Duration::from_millis),
            timed_out: false,
            endpoint,
            #[cfg(windows)]
            job: unsafe { std::os::windows::io::FromRawHandle::from_raw_handle(job) },
        }))
    }
    fn poll(&mut self) -> Result<(), String> {
        if self.exit.is_some() {
            return Ok(());
        }
        if self.timeout.is_some_and(|t| self.started.elapsed() >= t) && !self.timed_out {
            self.timed_out = true;
            self.events.push_back(SessionEvent::TimedOut);
            self.cancel()?;
        }
        if let Some(s) = self.child.try_wait().map_err(|e| e.to_string())? {
            let code = if self.timed_out {
                124
            } else {
                s.code().unwrap_or(1)
            };
            self.exit = Some(code);
            self.events.push_back(SessionEvent::Exited(code));
        }
        Ok(())
    }
}
impl SessionHandle for ProcessSession {
    fn next_event(&mut self) -> Result<Option<SessionEvent>, String> {
        self.poll()?;
        Ok(self.events.pop_front())
    }
    fn command(&mut self, action: ActionRequest) -> Result<(), String> {
        if action.id == "cancel" {
            return self.cancel();
        }
        self.poll()?;
        if self.exit.is_some() {
            return Err("Session has completed".into());
        }
        #[cfg(unix)]
        if let Some(endpoint) = &self.endpoint {
            let data = serde_json::to_vec(&action).map_err(|e| e.to_string())?;
            if data.len() > 8192 {
                return Err("Action payload exceeds 8 KiB".into());
            }
            let socket = std::os::unix::net::UnixDatagram::unbound().map_err(|e| e.to_string())?;
            socket.set_nonblocking(true).map_err(|e| e.to_string())?;
            socket.send_to(&data, endpoint).map_err(|e| e.to_string())?;
            self.events
                .push_back(SessionEvent::ActionDispatched(action.id));
            return Ok(());
        }
        Err(format!(
            "This session has no registered handler for {}",
            action.id
        ))
    }
    fn cancel(&mut self) -> Result<(), String> {
        // Terminate descendants even when their original parent has already exited.
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(
                std::os::windows::io::AsRawHandle::as_raw_handle(&self.job),
                1,
            );
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        if self.exit.is_some() {
            return Ok(());
        }
        match self.child.kill() {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
    fn wait(&mut self) -> Result<i32, String> {
        loop {
            self.poll()?;
            if let Some(code) = self.exit {
                return Ok(code);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for ProcessSession {
    fn drop(&mut self) {
        let _ = self.cancel();
        let _ = self.child.wait();
    }
}
#[cfg(windows)]
fn spawn_windows(
    command: &mut Command,
) -> Result<(Child, windows_sys::Win32::Foundation::HANDLE), String> {
    use std::os::windows::{io::AsRawHandle, process::CommandExt};
    use windows_sys::Win32::{
        Foundation::*,
        System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
    };
    // Start suspended so no guest instruction or child process can run outside its job.
    command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of_val(&info) as u32,
        ) == 0
        {
            let error = std::io::Error::last_os_error().to_string();
            CloseHandle(job);
            return Err(error);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                CloseHandle(job);
                return Err(e.to_string());
            }
        };
        let result = (|| {
            if AssignProcessToJobObject(job, child.as_raw_handle()) == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of_val(&entry) as u32;
            let mut more = Thread32First(snapshot, &mut entry);
            let mut thread = std::ptr::null_mut();
            while more != 0 {
                if entry.th32OwnerProcessID == child.id() {
                    thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    break;
                }
                more = Thread32Next(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
            if thread.is_null() {
                return Err("Cannot open the suspended guest thread".into());
            }
            let resumed = ResumeThread(thread);
            CloseHandle(thread);
            if resumed == u32::MAX {
                return Err("Cannot resume guest thread".into());
            }
            Ok(())
        })();
        if let Err(e) = result {
            TerminateJobObject(job, 1);
            let _ = child.kill();
            let _ = child.wait();
            CloseHandle(job);
            return Err(e);
        }
        Ok((child, job))
    }
}
#[path = "process_util.rs"]
pub mod utilities;

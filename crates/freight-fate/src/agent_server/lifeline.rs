//! The server's link to the MCP client that started it: a flag cut when
//! the client closes stdin or stops reading, and a watch on the parent
//! process for when that never arrives.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::Duration;

use super::{Command, Request};

/// Shared notice that the MCP client which owns this server has gone away.
#[derive(Clone, Default)]
pub struct Lifeline {
    cut: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<String>>>,
}

impl Lifeline {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cut the lifeline. The first observed reason is retained for diagnostics.
    pub fn cut(&self, reason: &str) {
        if !self.cut.swap(true, Ordering::AcqRel) {
            if let Ok(mut stored) = self.reason.lock() {
                *stored = Some(reason.to_string());
            }
        }
    }

    pub fn is_cut(&self) -> bool {
        self.cut.load(Ordering::Acquire)
    }

    pub fn reason(&self) -> Option<String> {
        self.reason.lock().ok().and_then(|reason| reason.clone())
    }
}

/// The process that started this server, watched so the server can end when
/// it does. Stdin closing is the usual signal, but on Windows the pipe can
/// stay open after the client exits (another process inherited its end), so
/// the parent itself is watched as well.
pub struct ParentWatch {
    #[cfg(unix)]
    parent_pid: u32,
    /// The parent's process handle as an integer (so the watch can move to
    /// its thread), or 0 when the parent was already gone at startup.
    #[cfg(windows)]
    handle: usize,
}

/// Capture the process which started the server, when the platform exposes it.
pub fn parent_alive_check() -> Option<ParentWatch> {
    #[cfg(unix)]
    {
        let parent_pid = std::os::unix::process::parent_id();
        (parent_pid != 1).then_some(ParentWatch { parent_pid })
    }
    #[cfg(windows)]
    {
        windows::parent_watch()
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

/// This process's parent id, when the platform can say.
pub fn parent_pid() -> Option<u32> {
    #[cfg(unix)]
    {
        Some(std::os::unix::process::parent_id())
    }
    #[cfg(windows)]
    {
        windows::parent_pid()
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

impl ParentWatch {
    pub fn alive(&self) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::process::parent_id() == self.parent_pid
        }
        #[cfg(windows)]
        {
            windows::handle_alive(self.handle)
        }
        #[cfg(not(any(unix, windows)))]
        {
            true
        }
    }
}

#[cfg(windows)]
impl Drop for ParentWatch {
    fn drop(&mut self) {
        windows::close(self.handle);
    }
}

/// Whether `pid` is a running process. Unknown counts as alive, so a holder
/// is never judged dead on a guess: on Windows a process this one may not
/// open (access denied) is alive, and on Unix other than Linux, where there
/// is no `/proc` to look in, every pid is.
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        windows::pid_alive(pid)
    }
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = pid;
        true
    }
}

/// Wake the game loop once the client disconnects or its parent exits.
pub fn spawn_watch(
    lifeline: Lifeline,
    wake: mpsc::Sender<Request>,
    watch: Option<ParentWatch>,
    poll: Duration,
) -> JoinHandle<()> {
    spawn_watch_with(
        lifeline,
        wake,
        move || watch.as_ref().is_none_or(ParentWatch::alive),
        poll,
    )
}

pub fn spawn_watch_with<F>(
    lifeline: Lifeline,
    wake: mpsc::Sender<Request>,
    alive: F,
    poll: Duration,
) -> JoinHandle<()>
where
    F: Fn() -> bool + Send + 'static,
{
    std::thread::spawn(move || loop {
        if !lifeline.is_cut() && !alive() {
            lifeline.cut("the process that started the server has exited");
        }
        if lifeline.is_cut() {
            let (reply, _dropped) = mpsc::channel();
            let _ = wake.send(Request {
                command: Command::Quit,
                reply,
            });
            return;
        }
        std::thread::sleep(poll);
    })
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };

    use super::ParentWatch;

    /// Open a process to wait on it. `Err(true)` when there is no such
    /// process, `Err(false)` when it exists but cannot be opened.
    fn open(pid: u32) -> Result<HANDLE, bool> {
        // SAFETY: no pointers cross the call; a non-null handle is owned by
        // the caller, who closes it.
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        if handle.is_null() {
            // SAFETY: reads this thread's last error, nothing else.
            let gone = unsafe { GetLastError() } == ERROR_INVALID_PARAMETER;
            Err(gone)
        } else {
            Ok(handle)
        }
    }

    pub(super) fn close(handle: usize) {
        if handle != 0 {
            // SAFETY: the handle came from OpenProcess and is closed once.
            unsafe {
                CloseHandle(handle as HANDLE);
            }
        }
    }

    pub(super) fn handle_alive(handle: usize) -> bool {
        // SAFETY: the handle is a live process handle owned by the caller.
        handle != 0 && unsafe { WaitForSingleObject(handle as HANDLE, 0) } == WAIT_TIMEOUT
    }

    pub(super) fn pid_alive(pid: u32) -> bool {
        match open(pid) {
            Ok(handle) => {
                let alive = handle_alive(handle as usize);
                close(handle as usize);
                alive
            }
            Err(gone) => !gone,
        }
    }

    pub(super) fn parent_pid() -> Option<u32> {
        // SAFETY: the snapshot handle is checked and closed below; the entry
        // is a plain C struct whose size field is set before the first call.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let ours = GetCurrentProcessId();
            let mut found = None;
            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    if entry.th32ProcessID == ours {
                        found = Some(entry.th32ParentProcessID);
                        break;
                    }
                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snapshot);
            found
        }
    }

    fn created(handle: HANDLE) -> Option<u64> {
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: four valid out-pointers to stack FILETIMEs for one call.
        let ok =
            unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
        (ok != 0)
            .then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }

    pub(super) fn parent_watch() -> Option<ParentWatch> {
        let Some(parent) = parent_pid() else {
            eprintln!("Could not find the process that started the server; parent watch is off.");
            return None;
        };
        let handle = match open(parent) {
            Ok(handle) => handle,
            // Already gone before we looked: the watch reports it at once.
            Err(true) => return Some(ParentWatch { handle: 0 }),
            Err(false) => {
                eprintln!(
                    "Could not open process {parent} that started the server; parent watch is off."
                );
                return None;
            }
        };
        // SAFETY: the pseudo-handle for this process needs no closing.
        let ours = created(unsafe { GetCurrentProcess() });
        match (created(handle), ours) {
            // A "parent" younger than this process is a stranger that reused
            // the dead parent's id.
            (Some(theirs), Some(ours)) if theirs > ours => {
                close(handle as usize);
                Some(ParentWatch { handle: 0 })
            }
            _ => Some(ParentWatch {
                handle: handle as usize,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn current_process_is_alive() {
        assert!(super::pid_alive(std::process::id()));
    }
}

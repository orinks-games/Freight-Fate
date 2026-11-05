//! Native crashes into the session log (Windows).
//!
//! The panic hook in `logging` writes a Rust panic to the log, but a crash
//! in native code (an access violation in a DLL, a stack overflow) or an
//! abort (an allocation failure) never reaches it: the game is a windowed
//! app with no console, so whatever Rust prints to stderr on the way down
//! goes nowhere, and the log just stops (issue #513). Two things close that
//! gap:
//!
//! - stderr is pointed at the log file, so Rust's own last words ("has
//!   overflowed its stack", "memory allocation of N bytes failed", the
//!   default panic message) land in the log;
//! - an unhandled-exception filter writes the exception code, the faulting
//!   address and the module it falls in, the thread it happened on, and the
//!   call stack as module offsets, before Windows ends the process. The
//!   first report through it (#517) named only `sapi.dll`; the thread and
//!   the frames under it say which of the game's speech calls led there.
//!
//! Both write through a duplicate of the log file's handle. A duplicate
//! shares the file's position with the logger's own handle, so these lines
//! append after the last log line instead of overwriting it.

use std::fs::File;
use std::os::windows::io::AsRawHandle;
use std::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{
    DuplicateHandle, LocalFree, DUPLICATE_SAME_ACCESS, HANDLE, HMODULE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::WriteFile;
use windows_sys::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE};
use windows_sys::Win32::System::Diagnostics::Debug::{
    RtlCaptureStackBackTrace, SetUnhandledExceptionFilter, EXCEPTION_POINTERS,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, GetCurrentThreadId, GetThreadDescription,
};

/// The duplicate handle the filter writes through; null until installed.
static CRASH_HANDLE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// The thread that installed the filter: the game loop's.
static GAME_THREAD: AtomicU32 = AtomicU32::new(0);

/// Stack frames written, at most.
const MAX_FRAMES: usize = 32;

/// `EXCEPTION_CONTINUE_SEARCH`: let Windows go on and end the process.
const CONTINUE_SEARCH: i32 = 0;

/// Point stderr and the crash filter at the session's log file. Called once,
/// before the logger takes the file. A failure leaves the game as it was.
pub fn install(log: &File) {
    let Some(handle) = duplicate(log) else {
        return;
    };
    CRASH_HANDLE.store(handle, Ordering::SeqCst);
    // SAFETY: no arguments; returns this thread's id.
    GAME_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
    if let Some(stderr) = duplicate(log) {
        // SAFETY: `stderr` is a valid handle this process owns for the rest
        // of its life; SetStdHandle only records it.
        unsafe {
            SetStdHandle(STD_ERROR_HANDLE, stderr);
        }
    }
    // SAFETY: `on_crash` matches the filter signature and touches only the
    // stored handle and its own stack.
    unsafe {
        SetUnhandledExceptionFilter(Some(on_crash));
    }
}

fn duplicate(file: &File) -> Option<HANDLE> {
    let mut out: HANDLE = std::ptr::null_mut();
    // SAFETY: the source handle is the open log file, borrowed for the
    // call; `out` receives a new handle owned by this process.
    let ok = unsafe {
        let process = GetCurrentProcess();
        DuplicateHandle(
            process,
            file.as_raw_handle() as HANDLE,
            process,
            &mut out,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    (ok != 0 && !out.is_null() && out != INVALID_HANDLE_VALUE).then_some(out)
}

/// A line built on the stack: the filter may run with the heap corrupt or
/// the stack nearly gone, so it neither allocates nor formats.
struct Line {
    buf: [u8; 768],
    len: usize,
}

impl Line {
    fn new() -> Self {
        Self {
            buf: [0; 768],
            len: 0,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }

    fn decimal(&mut self, mut value: u64) {
        let mut digits = [0u8; 20];
        let mut n = 0;
        loop {
            digits[n] = b'0' + (value % 10) as u8;
            n += 1;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        for i in (0..n).rev() {
            self.push(&[digits[i]]);
        }
    }

    /// A wide string as ASCII; anything else becomes '?'.
    fn wide(&mut self, units: &[u16]) {
        for &unit in units {
            self.push(&[if unit < 0x80 { unit as u8 } else { b'?' }]);
        }
    }

    fn write_to(&self, handle: HANDLE) {
        let mut written = 0u32;
        // SAFETY: `handle` is the duplicated log handle; the buffer is ours.
        unsafe {
            WriteFile(
                handle,
                self.buf.as_ptr(),
                self.len as u32,
                &mut written,
                std::ptr::null_mut(),
            );
        }
    }

    fn hex(&mut self, value: u64, digits: usize) {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        self.push(b"0x");
        for i in (0..digits).rev() {
            self.push(&[HEX[((value >> (i * 4)) & 0xF) as usize]]);
        }
    }
}

/// The exception's usual name, for the codes a game crash shows.
fn code_name(code: u32) -> &'static [u8] {
    match code {
        0xC000_0005 => b"access violation",
        0xC000_00FD => b"stack overflow",
        0xC000_001D => b"illegal instruction",
        0xC000_0094 => b"integer divide by zero",
        0xC000_0409 => b"stack buffer overrun or fail-fast",
        0xC000_0374 => b"heap corruption",
        0xE06D_7363 => b"unhandled C++ exception",
        0x8000_0003 => b"breakpoint",
        _ => b"native exception",
    }
}

/// The module `address` falls in, and the offset into it. `full` writes
/// the module's whole path, otherwise its file name alone.
fn push_location(line: &mut Line, address: usize, full: bool) {
    let mut module: HMODULE = std::ptr::null_mut();
    // SAFETY: FROM_ADDRESS reads the address as a location, not a string;
    // UNCHANGED_REFCOUNT leaves nothing to release.
    let found = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            address as *const u16,
            &mut module,
        )
    };
    if found == 0 || module.is_null() {
        line.push(b" in no module");
        return;
    }
    let mut wide = [0u16; 260];
    // SAFETY: the buffer and its length match.
    let n = unsafe { GetModuleFileNameW(module, wide.as_mut_ptr(), wide.len() as u32) };
    let path = &wide[..(n as usize).min(wide.len())];
    let name = if full {
        path
    } else {
        let from = path
            .iter()
            .rposition(|&unit| unit == u16::from(b'\\'))
            .map_or(0, |i| i + 1);
        &path[from..]
    };
    line.push(b" in ");
    line.wide(name);
    line.push(b" +");
    line.hex(address.wrapping_sub(module as usize) as u64, 8);
}

/// "on the game thread" or "on thread 1234 \"speech\"": which of the
/// game's threads (the loop, the speech worker, ...) or a library's own.
fn push_thread(line: &mut Line) {
    // SAFETY: no arguments; returns this thread's id.
    let id = unsafe { GetCurrentThreadId() };
    if id == GAME_THREAD.load(Ordering::SeqCst) {
        line.push(b" on the game thread");
        return;
    }
    line.push(b" on thread ");
    line.decimal(u64::from(id));
    let mut name: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: the pseudo-handle names this thread; on success `name` is a
    // LocalAlloc'd, NUL-terminated string this code frees.
    let hr = unsafe { GetThreadDescription(GetCurrentThread(), &mut name) };
    if hr >= 0 && !name.is_null() {
        let mut len = 0;
        // SAFETY: NUL-terminated, as above; capped so a bad string cannot run on.
        while len < 64 && unsafe { *name.add(len) } != 0 {
            len += 1;
        }
        if len > 0 {
            line.push(b" \"");
            // SAFETY: `len` units were just read.
            line.wide(unsafe { std::slice::from_raw_parts(name, len) });
            line.push(b"\"");
        }
        // SAFETY: allocated by GetThreadDescription.
        unsafe {
            LocalFree(name.cast());
        }
    }
}

/// The call stack, as return addresses Windows' own walker finds from here:
/// the filter's frames, the exception dispatcher, then the frames that led
/// to the fault. No symbols, so it works in a release build with nothing
/// installed.
fn write_stack(handle: HANDLE) {
    let mut frames = [std::ptr::null_mut(); MAX_FRAMES];
    // SAFETY: the buffer holds MAX_FRAMES pointers; no hash is asked for.
    let n = unsafe {
        RtlCaptureStackBackTrace(
            0,
            MAX_FRAMES as u32,
            frames.as_mut_ptr(),
            std::ptr::null_mut(),
        )
    };
    for (index, &pc) in frames[..usize::from(n).min(MAX_FRAMES)].iter().enumerate() {
        let mut line = Line::new();
        line.push(b"  ");
        line.decimal(index as u64);
        line.push(b": ");
        line.hex(pc as usize as u64, 16);
        push_location(&mut line, pc as usize, false);
        line.push(b"\n");
        line.write_to(handle);
    }
}

unsafe extern "system" fn on_crash(info: *const EXCEPTION_POINTERS) -> i32 {
    let handle = CRASH_HANDLE.load(Ordering::SeqCst);
    if handle.is_null() || info.is_null() {
        return CONTINUE_SEARCH;
    }
    // SAFETY: Windows passes valid exception pointers to the filter.
    let record = unsafe { (*info).ExceptionRecord };
    if record.is_null() {
        return CONTINUE_SEARCH;
    }
    // SAFETY: as above; the record lives for the filter call.
    let record = unsafe { &*record };
    let code = record.ExceptionCode as u32;
    let address = record.ExceptionAddress as usize;
    let mut line = Line::new();
    line.push(b"\nFatal native crash: ");
    line.push(code_name(code));
    line.push(b" (");
    line.hex(u64::from(code), 8);
    line.push(b") at ");
    line.hex(address as u64, 16);
    push_location(&mut line, address, true);
    if code == 0xC000_0005 && record.NumberParameters >= 2 {
        line.push(match record.ExceptionInformation[0] {
            0 => b", reading ".as_slice(),
            1 => b", writing ".as_slice(),
            8 => b", executing ".as_slice(),
            _ => b", touching ".as_slice(),
        });
        line.hex(record.ExceptionInformation[1] as u64, 16);
    }
    push_thread(&mut line);
    line.push(b"\n");
    line.write_to(handle);
    write_stack(handle);
    CONTINUE_SEARCH
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Diagnostics::Debug::{CONTEXT, EXCEPTION_RECORD};

    #[test]
    fn a_native_crash_appends_its_code_and_module_after_the_last_log_line() {
        let dir = std::env::temp_dir().join(format!("ff-crash-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("game.log");
        let mut file = File::create(&path).expect("log file");
        std::io::Write::write_all(&mut file, b"last line\n").expect("write");
        let handle = duplicate(&file).expect("duplicate");
        CRASH_HANDLE.store(handle, Ordering::SeqCst);

        // SAFETY: plain C structs; all-zero is a valid empty value.
        let mut record: EXCEPTION_RECORD = unsafe { std::mem::zeroed() };
        record.ExceptionCode = 0xC000_0005_u32 as i32;
        record.NumberParameters = 2;
        record.ExceptionInformation[0] = 0;
        record.ExceptionInformation[1] = 0x10;
        // An address inside this test binary, so the module lookup succeeds.
        record.ExceptionAddress = a_native_crash_appends_its_code_and_module_after_the_last_log_line
            as *const () as *mut core::ffi::c_void;
        // SAFETY: as above.
        let mut context: CONTEXT = unsafe { std::mem::zeroed() };
        let pointers = EXCEPTION_POINTERS {
            ExceptionRecord: &mut record,
            ContextRecord: &mut context,
        };
        // SAFETY: the pointers are valid for the call.
        let verdict = unsafe { on_crash(&pointers) };
        CRASH_HANDLE.store(std::ptr::null_mut(), Ordering::SeqCst);
        assert_eq!(verdict, CONTINUE_SEARCH);
        drop(file);

        let text = std::fs::read_to_string(&path).expect("read log");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(text.starts_with("last line\n"), "{text}");
        assert!(
            text.contains("Fatal native crash: access violation (0xC0000005) at 0x"),
            "{text}"
        );
        assert!(text.contains(".exe +0x"), "{text}");
        assert!(text.contains(", reading 0x0000000000000010"), "{text}");
        assert!(text.contains(" on thread "), "{text}");
    }

    #[test]
    fn the_stack_is_written_one_frame_a_line() {
        let dir = std::env::temp_dir().join(format!("ff-crash-stack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("game.log");
        let file = File::create(&path).expect("log file");
        let handle = duplicate(&file).expect("duplicate");
        write_stack(handle);
        drop(file);

        let text = std::fs::read_to_string(&path).expect("read log");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(text.starts_with("  0: 0x"), "{text}");
        assert!(text.contains("\n  1: 0x"), "{text}");
        assert!(text.contains(".exe +0x"), "{text}");
    }
}

//! Test-only SDK fixture, compiled directly with rustc (no UI dependencies).
//! Windows Node streams retain additional inherited pipe handles; a native
//! process lets the regression test produce actual stdout EOF without exiting.

use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::time::Duration;

fn trace(path: &Path, stage: &str) -> io::Result<()> {
    writeln!(
        OpenOptions::new().create(true).append(true).open(path)?,
        "{{\"stage\":\"{stage}\",\"fixture\":\"native\",\"pid\":{}}}",
        std::process::id()
    )
}

#[cfg(windows)]
fn close_stdout() -> io::Result<()> {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(id: u32) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    extern "C" {
        fn _get_osfhandle(fd: i32) -> isize;
    }
    // The Win32 standard handle and the CRT descriptor can name distinct
    // inherited handles. Close each unique valid handle exactly once.
    // https://learn.microsoft.com/en-us/windows/console/getstdhandle
    // https://learn.microsoft.com/en-us/cpp/c-runtime-library/reference/get-osfhandle
    unsafe {
        let standard = GetStdHandle(-11_i32 as u32);
        let crt = _get_osfhandle(1);
        if standard.is_null() || standard as isize == -1 {
            return Err(io::Error::last_os_error());
        }
        if CloseHandle(standard) == 0 {
            return Err(io::Error::last_os_error());
        }
        if crt >= 0 && crt != standard as isize && CloseHandle(crt as *mut c_void) == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(unix)]
fn close_stdout() -> io::Result<()> {
    extern "C" {
        fn close(fd: i32) -> i32;
    }
    if unsafe { close(1) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn main() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().expect("fixture mode");
    assert!(mode == "eof" || mode == "partial");
    let trace_path = args.next().expect("fixture trace path");
    let trace_path = Path::new(&trace_path);
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    assert!(line.contains("\"type\":\"snapshot\""));
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "{{\"version\":1,\"type\":\"ready\",\"pid\":{},\"platform\":\"{}\"}}",
        std::process::id(),
        if cfg!(windows) { "win32" } else { "test" }
    )?;
    if mode == "partial" {
        stdout.write_all(b"{")?;
    }
    stdout.flush()?;
    close_stdout()?;
    trace(trace_path, "stdout-closed-child-alive")?;
    // Deliberately ignore stdin/quit. Only the SDK's termination can complete
    // these tests; the fixture cannot confuse stdout EOF with process exit.
    std::thread::sleep(Duration::from_secs(30));
    std::process::exit(77);
}

//! `--launch <argv file>`: runs the product's command as a child of the
//! local app's executable, so macOS attributes its prompts and grants to the
//! app. Moved unchanged from `hraness-companion`.

use std::path::Path;

use crate::wire::ProtocolError;

#[cfg(unix)]
use std::sync::atomic::{AtomicI32, Ordering};

#[cfg(unix)]
static LAUNCHED_CHILD: AtomicI32 = AtomicI32::new(0);
#[cfg(unix)]
static PENDING_SIGNAL: AtomicI32 = AtomicI32::new(0);

#[cfg(unix)]
extern "C" fn forward_signal(signal: libc::c_int) {
    let child = LAUNCHED_CHILD.load(Ordering::Acquire);
    if child > 0 {
        unsafe { libc::kill(child, signal) };
    } else {
        // Arrived before the child existed; delivered right after spawn.
        PENDING_SIGNAL.store(signal, Ordering::Release);
    }
}

/// Stays the parent on purpose; `exec` would make the child responsible for
/// itself. The child gets its own process group, so a terminal Ctrl-C
/// reaches it once, through this forwarder. On success this exits the
/// process with the child's status and never returns.
#[cfg(unix)]
pub fn launch(argv_file: &Path) -> Result<(), ProtocolError> {
    use crate::identity;
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    let executable = std::env::current_exe().map_err(|_| ProtocolError("launch-outside-app"))?;
    let bundle_id =
        identity::own_bundle_id(&executable).ok_or(ProtocolError("launch-outside-app"))?;
    let argv = identity::read_argv_file(argv_file).ok_or(ProtocolError("invalid-launch-file"))?;
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        unsafe { libc::signal(signal, forward_signal as *const () as libc::sighandler_t) };
    }
    let mut child = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .env(identity::BUNDLE_ID_ENV, bundle_id)
        .process_group(0)
        .spawn()
        .map_err(|_| ProtocolError("launch-failed"))?;
    let pid = child.id() as i32;
    LAUNCHED_CHILD.store(pid, Ordering::Release);
    let pending = PENDING_SIGNAL.swap(0, Ordering::AcqRel);
    if pending != 0 {
        unsafe { libc::kill(pid, pending) };
    }
    let status = child.wait();
    // Never signal a reused pid after the child is gone.
    LAUNCHED_CHILD.store(0, Ordering::Release);
    let status = status.map_err(|_| ProtocolError("launch-failed"))?;
    std::process::exit(
        status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)),
    );
}

#[cfg(not(unix))]
pub fn launch(_argv_file: &Path) -> Result<(), ProtocolError> {
    Err(ProtocolError("launch-outside-app"))
}

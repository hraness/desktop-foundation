//! Human-initiated HTTPS handoffs for product menu actions.
//!
//! Constructing an opener and reading its status perform no IO. Call `open`
//! only from an explicit action, with a product-owned URL. This module neither
//! sends an HTTP request nor establishes signup, payment, or browser visibility.

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_URL_BYTES: usize = 4096;
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Bounded diagnostic categories, deliberately containing no address or argv.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserError {
    InvalidUrl,
    Busy,
    Unavailable,
    LaunchFailed,
    TimedOut,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowserStatus {
    Idle,
    Opening,
    /// The operating-system opener exited successfully; viewing is not proven.
    Opened,
    Failed(BrowserError),
}

/// One bounded launch at a time. Clones share the same admission and status.
///
/// Dropping an opener does not abandon an already launched child: its worker
/// retains ownership until the launcher exits or is killed and reaped. A
/// browser may already have received navigation, so failures are never retried.
#[derive(Clone)]
pub struct BrowserOpener {
    state: Arc<Mutex<BrowserStatus>>,
}

impl Default for BrowserOpener {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(BrowserStatus::Idle)),
        }
    }
}

impl BrowserOpener {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn status(&self) -> BrowserStatus {
        *self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Queue an explicit menu action without waiting on the native UI thread.
    ///
    /// `Ok(())` means the request was admitted, not that a browser opened. Poll
    /// `status` from the next snapshot for completion. URLs may have public
    /// product/source routing parameters; callers must not supply credentials,
    /// personal data, session tokens or untrusted daemon-provided addresses.
    pub fn open(&self, address: &str) -> Result<(), BrowserError> {
        let address = checked_https(address)?;
        self.start(move || {
            let mut command = browser_command(&address);
            let child = command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| match error.kind() {
                    std::io::ErrorKind::NotFound => BrowserError::Unavailable,
                    _ => BrowserError::LaunchFailed,
                })?;
            supervise(child, OPEN_TIMEOUT)
        })
    }

    fn start<F>(&self, operation: F) -> Result<(), BrowserError>
    where
        F: FnOnce() -> Result<(), BrowserError> + Send + 'static,
    {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if *state == BrowserStatus::Opening {
            return Err(BrowserError::Busy);
        }
        *state = BrowserStatus::Opening;
        let shared = Arc::clone(&self.state);
        if std::thread::Builder::new()
            .name("desktop-browser-open".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation))
                    .unwrap_or(Err(BrowserError::LaunchFailed));
                *shared.lock().unwrap_or_else(|error| error.into_inner()) = match result {
                    Ok(()) => BrowserStatus::Opened,
                    Err(error) => BrowserStatus::Failed(error),
                };
            })
            .is_err()
        {
            *state = BrowserStatus::Failed(BrowserError::Unavailable);
            return Err(BrowserError::Unavailable);
        }
        Ok(())
    }
}

fn checked_https(address: &str) -> Result<String, BrowserError> {
    if address.is_empty()
        || address.len() > MAX_URL_BYTES
        || !address.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        || !address.starts_with("https://")
        || address.contains('\\')
    {
        return Err(BrowserError::InvalidUrl);
    }
    let authority = address[8..].split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return Err(BrowserError::InvalidUrl);
    }
    let url = tauri::Url::parse(address).map_err(|_| BrowserError::InvalidUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(BrowserError::InvalidUrl);
    }
    Ok(url.into())
}

fn browser_command(address: &str) -> Command {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");
    command.arg(address);
    command
}

fn supervise(mut child: Child, timeout: Duration) -> Result<(), BrowserError> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(BrowserError::LaunchFailed)
                };
            }
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BrowserError::LaunchFailed);
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(BrowserError::TimedOut);
        }
        std::thread::sleep(POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn wait_for(opener: &BrowserOpener, expected: BrowserStatus) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while opener.status() == BrowserStatus::Opening && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(opener.status(), expected);
    }

    #[test]
    fn validates_https_before_any_launch_and_never_echoes_input() {
        let opener = BrowserOpener::new();
        for value in [
            "",
            "http://example.com",
            "file:///tmp/a",
            "https://",
            "https:///",
            "https:///example.com",
            "https://?example.com",
            "https://user:secret@example.com",
            "https://@example.com",
            "https://example.com\n",
            " https://example.com",
            "https://example.com/a b",
            "https://example.com/\\other",
            "https://example.com/\u{202e}text",
            "https://example.com/\0text",
        ] {
            assert_eq!(opener.open(value), Err(BrowserError::InvalidUrl));
            assert_eq!(opener.status(), BrowserStatus::Idle);
        }
        assert_eq!(
            checked_https(&format!(
                "https://example.com/{}",
                "a".repeat(MAX_URL_BYTES)
            )),
            Err(BrowserError::InvalidUrl)
        );
        let address = "https://account.hraness.com/support?product=hra&source=desktop#support";
        assert_eq!(checked_https(address).unwrap(), address);
        assert_eq!(format!("{:?}", BrowserError::InvalidUrl), "InvalidUrl");
    }

    #[test]
    fn public_query_is_one_argument_without_a_shell() {
        let address = "https://example.com/?product=tools&source=desktop#updates";
        let command = browser_command(address);
        let args: Vec<_> = command.get_args().collect();
        assert_eq!(args.last().unwrap(), &std::ffi::OsStr::new(address));
        assert_eq!(args.len(), if cfg!(target_os = "windows") { 2 } else { 1 });
        assert!(!["sh", "bash", "cmd.exe", "powershell.exe"]
            .contains(&command.get_program().to_str().unwrap()));
    }

    #[test]
    fn cloned_openers_share_one_nonblocking_launch_and_can_recover() {
        let opener = BrowserOpener::new();
        let (entered, observed) = mpsc::channel();
        let (finish, wait) = mpsc::channel();
        opener
            .start(move || {
                entered.send(()).unwrap();
                wait.recv().unwrap();
                Ok(())
            })
            .unwrap();
        observed.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(
            opener.clone().start(|| panic!("must not start")),
            Err(BrowserError::Busy)
        );
        finish.send(()).unwrap();
        wait_for(&opener, BrowserStatus::Opened);
        opener.start(|| Err(BrowserError::Unavailable)).unwrap();
        wait_for(&opener, BrowserStatus::Failed(BrowserError::Unavailable));
        opener.start(|| Ok(())).unwrap();
        wait_for(&opener, BrowserStatus::Opened);
    }

    fn fixture(mode: &str) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "browser::tests::subprocess_fixture", "--ignored"])
            .env("DESKTOP_BROWSER_TEST_FIXTURE", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    #[test]
    fn supervises_success_failure_and_timeout_without_a_browser() {
        assert_eq!(
            supervise(fixture("success"), Duration::from_secs(3)),
            Ok(())
        );
        assert_eq!(
            supervise(fixture("failure"), Duration::from_secs(3)),
            Err(BrowserError::LaunchFailed)
        );
        assert_eq!(
            supervise(fixture("wait"), Duration::from_millis(100)),
            Err(BrowserError::TimedOut)
        );
    }

    #[test]
    #[ignore = "child fixture used only by the supervisor test"]
    fn subprocess_fixture() {
        match std::env::var("DESKTOP_BROWSER_TEST_FIXTURE").as_deref() {
            Ok("success") => std::process::exit(0),
            Ok("failure") => std::process::exit(7),
            Ok("wait") => loop {
                std::thread::sleep(Duration::from_secs(1));
            },
            _ => panic!("fixture mode required"),
        }
    }
}

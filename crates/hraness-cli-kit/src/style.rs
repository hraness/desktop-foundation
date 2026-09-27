//! The Hraness CLI style contract: status symbols with ASCII fallbacks,
//! color only where a person reads a terminal, one-sentence errors with one
//! next step, `Next:` hints, and quiet exits on closed pipes. The TypeScript
//! twin is `@hraness/desktop-foundation/cli-style`.

use crate::audience::{self, Audience};
use std::cell::RefCell;
use std::io::{IsTerminal as _, Write};

/// The symbols a CLI prints. Each has one meaning, one ASCII fallback and one
/// tint; only the symbol is ever colored, never the sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Symbol {
    /// ✓ done, healthy
    Ok,
    /// ✗ failed
    Fail,
    /// ⚠ needs attention
    Warn,
    /// → the next step
    Next,
    /// ● running, on
    On,
    /// ○ idle, off
    Off,
    /// – skipped
    Skip,
    /// ↻ in progress
    Progress,
    /// 🔐 a macOS permission notice (only there)
    Notice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tint {
    Green,
    Red,
    Yellow,
    Dim,
}

impl Symbol {
    pub const ALL: [Symbol; 9] = [
        Symbol::Ok,
        Symbol::Fail,
        Symbol::Warn,
        Symbol::Next,
        Symbol::On,
        Symbol::Off,
        Symbol::Skip,
        Symbol::Progress,
        Symbol::Notice,
    ];

    /// The Unicode glyph. `⚠` carries no variation selector in the CLI.
    pub fn glyph(self) -> &'static str {
        match self {
            Symbol::Ok => "✓",
            Symbol::Fail => "✗",
            Symbol::Warn => "⚠",
            Symbol::Next => "→",
            Symbol::On => "●",
            Symbol::Off => "○",
            Symbol::Skip => "–",
            Symbol::Progress => "↻",
            Symbol::Notice => "🔐",
        }
    }

    /// The fallback for `TERM=dumb`, a locale without UTF-8, or `HRANESS_ASCII=1`.
    pub fn ascii(self) -> &'static str {
        match self {
            Symbol::Ok => "OK",
            Symbol::Fail => "FAIL",
            Symbol::Warn => "WARN",
            Symbol::Next => "->",
            Symbol::On => "*",
            Symbol::Off => "o",
            Symbol::Skip => "-",
            Symbol::Progress => "...",
            Symbol::Notice => "NOTE",
        }
    }

    fn tint(self) -> Option<Tint> {
        match self {
            Symbol::Ok | Symbol::On => Some(Tint::Green),
            Symbol::Fail => Some(Tint::Red),
            Symbol::Warn => Some(Tint::Yellow),
            Symbol::Next | Symbol::Skip => Some(Tint::Dim),
            Symbol::Off | Symbol::Progress | Symbol::Notice => None,
        }
    }
}

/// How one stream renders symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub color: bool,
    pub ascii: bool,
}

impl Style {
    /// Unicode symbols, no color: what tests and golden files use.
    pub const PLAIN: Style = Style {
        color: false,
        ascii: false,
    };
    /// ASCII fallbacks, no color.
    pub const ASCII: Style = Style {
        color: false,
        ascii: true,
    };

    /// Color only for a terminal stream, `TERM` other than `dumb`, and
    /// `NO_COLOR` unset or empty (`NO_COLOR` wins over `FORCE_COLOR`).
    /// ASCII fallbacks replace every symbol for `TERM=dumb`, a locale that
    /// names no UTF-8, or `HRANESS_ASCII=1`.
    pub fn detect(env: &dyn Fn(&str) -> Option<String>, is_tty: bool) -> Self {
        let set = |name: &str| env(name).is_some_and(|value| !value.is_empty());
        let dumb = env("TERM").as_deref() == Some("dumb");
        let force = env("FORCE_COLOR")
            .is_some_and(|value| !value.is_empty() && value != "0" && value != "false");
        let color = !set("NO_COLOR") && (force || (is_tty && !dumb));
        let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"].iter().any(|name| {
            env(name).is_some_and(|value| {
                let value = value.to_ascii_lowercase();
                value.contains("utf-8") || value.contains("utf8")
            })
        });
        let ascii = dumb || !utf8 || env("HRANESS_ASCII").as_deref() == Some("1");
        Self { color, ascii }
    }

    /// The style for this process's stdout.
    pub fn stdout() -> Self {
        Self::detect(&audience::process_env, std::io::stdout().is_terminal())
    }

    /// The style for this process's stderr.
    pub fn stderr() -> Self {
        Self::detect(&audience::process_env, std::io::stderr().is_terminal())
    }

    /// No color unless a person is reading.
    pub fn for_audience(self, audience: Audience) -> Self {
        Self {
            color: self.color && audience == Audience::Human,
            ascii: self.ascii,
        }
    }

    /// The symbol, with its ASCII fallback and tint applied.
    pub fn symbol(self, symbol: Symbol) -> String {
        let text = if self.ascii {
            symbol.ascii()
        } else {
            symbol.glyph()
        };
        match symbol.tint() {
            Some(tint) if self.color => {
                let code = match tint {
                    Tint::Green => "32",
                    Tint::Red => "31",
                    Tint::Yellow => "33",
                    Tint::Dim => "2",
                };
                format!("\u{1b}[{code}m{text}\u{1b}[0m")
            }
            _ => text.to_owned(),
        }
    }

    /// `✓ text`, `⚠ text` and so on, without a newline.
    pub fn line(self, symbol: Symbol, text: &str) -> String {
        format!("{} {text}", self.symbol(symbol))
    }
}

/// Human file size for progress lines: `1.8 MB`, `640 KB`.
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 999_500 {
        // Tenths rounded half up, as JavaScript's toFixed(1) does here:
        // 1_250_000 is "1.3 MB".
        let tenths = bytes.saturating_add(50_000) / 100_000;
        format!("{}.{} MB", tenths / 10, tenths % 10)
    } else if bytes >= 1_000 {
        format!("{} KB", (bytes as f64 / 1_000.0).round() as u64)
    } else {
        format!("{bytes} bytes")
    }
}

/// One line saying how a check list ended, for the end of `status` and
/// `doctor`: `All 5 checks passed.`, `1 warning.`, `2 problems, 1 warning.`
pub fn check_summary(passed: usize, warnings: usize, failures: usize) -> String {
    let plural = |count: usize, one: &str, many: &str| {
        format!("{count} {}", if count == 1 { one } else { many })
    };
    match (failures, warnings) {
        (0, 0) => match passed {
            0 => "Nothing to check.".to_owned(),
            1 => "The check passed.".to_owned(),
            count => format!("All {count} checks passed."),
        },
        (0, warnings) => format!("{}.", plural(warnings, "warning", "warnings")),
        (failures, 0) => format!("{}.", plural(failures, "problem", "problems")),
        (failures, warnings) => format!(
            "{}, {}.",
            plural(failures, "problem", "problems"),
            plural(warnings, "warning", "warnings")
        ),
    }
}

/// True for an error from writing to a reader that went away.
pub fn is_broken_pipe(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::BrokenPipe
}

/// Write `text` to stdout and flush, ignoring a closed pipe (`| head -1`).
/// Returns false when the reader went away, so a listing can stop early.
pub fn write_stdout(text: &str) -> bool {
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => true,
        Err(error) if is_broken_pipe(&error) => false,
        Err(_) => true,
    }
}

/// Make a closed stdout end the process quietly with exit 0 instead of a
/// `failed printing to stdout: Broken pipe` panic from `println!`. Call once
/// at the top of `main`. Only that panic exits: a broken socket or child pipe
/// elsewhere still panics or errors as before, so a server thread that loses
/// a client is never mistaken for a closed stdout.
pub fn exit_quietly_on_broken_pipe() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("");
        if is_stdout_broken_pipe_panic(message) {
            std::process::exit(0);
        }
        default(info);
    }));
}

/// True for the panic `print!`/`println!` raise when stdout's reader went
/// away.
fn is_stdout_broken_pipe_panic(message: &str) -> bool {
    message.starts_with("failed printing to stdout")
        && (message.contains("Broken pipe") || message.contains("os error 32"))
}

/// Restore the default SIGPIPE action, so a closed pipe stops the process at
/// once. Only for read-only listing commands: a server or a command that
/// writes to a child process must keep SIGPIPE ignored, or a peer that hangs
/// up kills it. Prefer [`exit_quietly_on_broken_pipe`] when unsure. Does
/// nothing outside Unix.
pub fn restore_default_sigpipe() {
    #[cfg(unix)]
    {
        extern "C" {
            fn signal(signum: i32, handler: usize) -> usize;
        }
        const SIGPIPE: i32 = 13;
        const SIG_DFL: usize = 0;
        // SAFETY: `signal` with SIG_DFL for SIGPIPE is always valid. SIGPIPE
        // is 13 and SIG_DFL is 0 on every Unix Rust supports.
        unsafe {
            signal(SIGPIPE, SIG_DFL);
        }
    }
}

/// An error a person can act on: one sentence, an optional detail line, and
/// the one command to run next. `code` is kept for `--json` readers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    /// A stable kebab-case code for JSON (`usage`, `not-found`,
    /// `permission-denied`).
    pub code: String,
    /// One sentence saying what happened, ending with a period.
    pub message: String,
    /// One more line, shown indented under the sentence (what to change in
    /// Settings, for example).
    pub detail: Option<String>,
    /// The one next command, shown after `→`.
    pub next: Option<String>,
    /// `(kind, settingsUrl)` for a permission failure.
    pub permission: Option<(String, Option<String>)>,
    /// The process exit code: 1 for failures, 2 for usage errors.
    pub exit_code: i32,
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

impl CliError {
    /// A failure (exit 1).
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            detail: None,
            next: None,
            permission: None,
            exit_code: 1,
        }
    }

    /// A usage error (exit 2): the input was wrong.
    pub fn usage(message: impl Into<String>, next: impl Into<String>) -> Self {
        Self::new("usage", message)
            .with_next(next)
            .with_exit_code(2)
    }

    pub fn with_next(mut self, next: impl Into<String>) -> Self {
        self.next = Some(next.into());
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = code;
        self
    }

    /// The text form: `✗ message`, an indented detail, then `→ next`.
    pub fn render_human(&self, style: Style) -> String {
        let mut out = style.line(Symbol::Fail, &self.message);
        out.push('\n');
        if let Some(detail) = &self.detail {
            out.push_str("  ");
            out.push_str(detail);
            out.push('\n');
        }
        if let Some(next) = &self.next {
            out.push_str(&style.line(Symbol::Next, next));
            out.push('\n');
        }
        out
    }

    /// The JSON form, one line:
    /// `{"ok":false,"error":{"code","message","next"}}`, plus
    /// `"permission":{"kind","settingsUrl"}` for permission failures. A
    /// detail line joins the message.
    pub fn render_json(&self) -> String {
        let message = match &self.detail {
            Some(detail) => format!("{} {detail}", self.message),
            None => self.message.clone(),
        };
        let mut error = crate::json::Object::new()
            .str("code", &self.code)
            .str("message", &message)
            .str_or_null("next", self.next.as_deref());
        if let Some((kind, url)) = &self.permission {
            error = error.raw(
                "permission",
                &crate::json::Object::new()
                    .str("kind", kind)
                    .str_or_null("settingsUrl", url.as_deref())
                    .finish(),
            );
        }
        crate::json::Object::new()
            .raw("ok", "false")
            .raw("error", &error.finish())
            .finish()
    }

    /// Print this error for its reader and return the exit code. With
    /// `--json` or an agent reader the JSON form goes to stdout; everyone
    /// else gets the text form on stderr. Commands whose stdout belongs to a
    /// protocol peer pass `json: false` and `Audience::Quiet`.
    pub fn report(&self, json: bool, audience: Audience) -> i32 {
        if json || audience == Audience::Agent {
            write_stdout(&(self.render_json() + "\n"));
        } else {
            let style = Style::stderr().for_audience(audience);
            let _ = std::io::stderr().write_all(self.render_human(style).as_bytes());
        }
        self.exit_code
    }
}

/// First letter up and a closing period, so an internal message reads as a
/// sentence. Text that starts with one of `keep` (a lowercase product or
/// command name) keeps its case.
pub fn sentence(text: &str, keep: &[&str]) -> String {
    let text = text.trim();
    let mut out = String::with_capacity(text.len() + 1);
    if keep.iter().any(|prefix| text.starts_with(prefix)) || text.starts_with('`') {
        out.push_str(text);
    } else {
        let mut chars = text.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if !out.is_empty() && !out.ends_with(['.', '?', '!']) {
        out.push('.');
    }
    out
}

/// Output with one set of rules. Results go to stdout; errors, warnings and
/// `Next:` hints go to stderr. The quiet audience gets plain text with no
/// color and no hints; agents should print JSON and use this only for what
/// JSON cannot carry.
pub struct Output {
    audience: Audience,
    out_style: Style,
    err_style: Style,
    out: RefCell<Box<dyn Write>>,
    err: RefCell<Box<dyn Write>>,
}

impl Output {
    /// This process's streams and audience.
    pub fn detect() -> Self {
        let audience = audience::detect_current();
        Self::with_streams(
            audience,
            Style::stdout(),
            Style::stderr(),
            Box::new(std::io::stdout()),
            Box::new(std::io::stderr()),
        )
    }

    /// Explicit streams, for tests and embedders.
    pub fn with_streams(
        audience: Audience,
        out_style: Style,
        err_style: Style,
        out: Box<dyn Write>,
        err: Box<dyn Write>,
    ) -> Self {
        Self {
            audience,
            out_style: out_style.for_audience(audience),
            err_style: err_style.for_audience(audience),
            out: RefCell::new(out),
            err: RefCell::new(err),
        }
    }

    pub fn audience(&self) -> Audience {
        self.audience
    }

    pub fn stdout_style(&self) -> Style {
        self.out_style
    }

    pub fn stderr_style(&self) -> Style {
        self.err_style
    }

    fn write_out(&self, text: &str) {
        let mut out = self.out.borrow_mut();
        let _ = out.write_all(text.as_bytes()).and_then(|()| out.flush());
    }

    fn write_err(&self, text: &str) {
        let mut err = self.err.borrow_mut();
        let _ = err.write_all(text.as_bytes()).and_then(|()| err.flush());
    }

    /// A result line on stdout.
    pub fn result(&self, text: &str) {
        self.write_out(&format!("{text}\n"));
    }

    /// A result line on stdout led by a symbol.
    pub fn result_with(&self, symbol: Symbol, text: &str) {
        self.write_out(&format!("{}\n", self.out_style.line(symbol, text)));
    }

    /// An indented detail line under the previous result.
    pub fn detail(&self, text: &str) {
        self.write_out(&format!("  {text}\n"));
    }

    /// `⚠ message` on stderr.
    pub fn warn(&self, message: &str) {
        self.write_err(&format!("{}\n", self.err_style.line(Symbol::Warn, message)));
    }

    /// `Next: command` on stderr, for a person only.
    pub fn next(&self, command: &str) {
        if self.audience == Audience::Human {
            self.write_err(&format!("Next: {command}\n"));
        }
    }

    /// The text form of an error on stderr. Returns its exit code.
    pub fn error(&self, error: &CliError) -> i32 {
        self.write_err(&error.render_human(self.err_style));
        error.exit_code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn style_respects_no_color_term_and_locale() {
        let tty = Style::detect(&env_of(&[("LANG", "en_US.UTF-8")]), true);
        assert_eq!(tty.symbol(Symbol::Ok), "\u{1b}[32m✓\u{1b}[0m");
        assert_eq!(tty.symbol(Symbol::Off), "○", "untinted symbols stay plain");
        let no_color = Style::detect(&env_of(&[("LANG", "en_US.UTF-8"), ("NO_COLOR", "1")]), true);
        assert_eq!(no_color.symbol(Symbol::Fail), "✗", "NO_COLOR keeps symbols");
        let empty_no_color =
            Style::detect(&env_of(&[("LANG", "en_US.UTF-8"), ("NO_COLOR", "")]), true);
        assert!(empty_no_color.color, "an empty NO_COLOR is unset");
        let piped = Style::detect(&env_of(&[("LANG", "en_US.UTF-8")]), false);
        assert_eq!(piped, Style::PLAIN);
        let dumb = Style::detect(&env_of(&[("LANG", "en_US.UTF-8"), ("TERM", "dumb")]), true);
        assert_eq!(dumb, Style::ASCII);
        assert_eq!(dumb.symbol(Symbol::Next), "->");
        let c_locale = Style::detect(&env_of(&[("LANG", "C")]), false);
        assert_eq!(c_locale.symbol(Symbol::On), "*");
        let lc_all = Style::detect(&env_of(&[("LC_ALL", "C.utf8")]), false);
        assert!(!lc_all.ascii);
        let ascii = Style::detect(
            &env_of(&[("LANG", "en_US.UTF-8"), ("HRANESS_ASCII", "1")]),
            true,
        );
        assert_eq!(ascii.symbol(Symbol::Notice), "NOTE");
        let forced = Style::detect(
            &env_of(&[("LANG", "en_US.UTF-8"), ("FORCE_COLOR", "1")]),
            false,
        );
        assert!(forced.color);
        for off in ["0", "false", ""] {
            let not_forced = Style::detect(
                &env_of(&[("LANG", "en_US.UTF-8"), ("FORCE_COLOR", off)]),
                false,
            );
            assert!(!not_forced.color, "FORCE_COLOR={off:?}");
        }
        let both = Style::detect(
            &env_of(&[
                ("LANG", "en_US.UTF-8"),
                ("FORCE_COLOR", "1"),
                ("NO_COLOR", "1"),
            ]),
            true,
        );
        assert!(!both.color, "NO_COLOR wins over FORCE_COLOR");
        assert!(!tty.for_audience(Audience::Quiet).color);
        assert!(tty.for_audience(Audience::Human).color);
    }

    #[test]
    fn symbols_match_the_contract_table() {
        let table: Vec<(&str, &str)> = Symbol::ALL.iter().map(|s| (s.glyph(), s.ascii())).collect();
        assert_eq!(
            table,
            [
                ("✓", "OK"),
                ("✗", "FAIL"),
                ("⚠", "WARN"),
                ("→", "->"),
                ("●", "*"),
                ("○", "o"),
                ("–", "-"),
                ("↻", "..."),
                ("🔐", "NOTE")
            ]
        );
        assert!(!Symbol::Warn.glyph().contains('\u{fe0f}'));
    }

    #[test]
    fn errors_render_one_sentence_and_one_next_step() {
        let error = CliError::new("not-found", "No chat matches \"Mom\".")
            .with_next("textbutler chats list");
        assert_eq!(
            error.render_human(Style::PLAIN),
            "✗ No chat matches \"Mom\".\n→ textbutler chats list\n"
        );
        assert_eq!(
            error.render_human(Style::ASCII),
            "FAIL No chat matches \"Mom\".\n-> textbutler chats list\n"
        );
        assert_eq!(
            error.render_json(),
            r#"{"ok":false,"error":{"code":"not-found","message":"No chat matches \"Mom\".","next":"textbutler chats list"}}"#
        );
        let bare = CliError::new("failed", "Something broke.").with_detail("Try again.");
        assert_eq!(
            bare.render_human(Style::PLAIN),
            "✗ Something broke.\n  Try again.\n"
        );
        assert_eq!(
            bare.render_json(),
            r#"{"ok":false,"error":{"code":"failed","message":"Something broke. Try again.","next":null}}"#
        );
        let usage = CliError::usage("Unknown command \"x\".", "xcb --help");
        assert_eq!(usage.exit_code, 2);
        assert_eq!(usage.code, "usage");
    }

    #[test]
    fn sentences_and_summaries() {
        assert_eq!(sentence("account not found", &[]), "Account not found.");
        assert_eq!(sentence("xcb can't start", &["xcb "]), "xcb can't start.");
        assert_eq!(sentence("Done?", &[]), "Done?");
        assert_eq!(sentence("", &[]), "");
        assert_eq!(check_summary(5, 0, 0), "All 5 checks passed.");
        assert_eq!(check_summary(1, 0, 0), "The check passed.");
        assert_eq!(check_summary(0, 0, 0), "Nothing to check.");
        assert_eq!(check_summary(3, 1, 0), "1 warning.");
        assert_eq!(check_summary(3, 2, 0), "2 warnings.");
        assert_eq!(check_summary(3, 0, 1), "1 problem.");
        assert_eq!(check_summary(3, 1, 2), "2 problems, 1 warning.");
        assert_eq!(format_bytes(1_800_000), "1.8 MB");
        assert_eq!(format_bytes(1_250_000), "1.3 MB");
        assert_eq!(format_bytes(999_500), "1.0 MB");
        assert_eq!(format_bytes(12_340_000), "12.3 MB");
        assert!(is_stdout_broken_pipe_panic(
            "failed printing to stdout: Broken pipe (os error 32)"
        ));
        assert!(!is_stdout_broken_pipe_panic(
            "socket write: Broken pipe (os error 32)"
        ));
        assert!(!is_stdout_broken_pipe_panic(
            "failed printing to stdout: disk full"
        ));
        assert_eq!(format_bytes(640_000), "640 KB");
        assert_eq!(format_bytes(12), "12 bytes");
    }

    #[derive(Clone, Default)]
    struct Shared(Rc<RefCell<Vec<u8>>>);
    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Shared {
        fn text(&self) -> String {
            String::from_utf8(self.0.borrow().clone()).unwrap()
        }
    }

    #[test]
    fn output_sends_hints_to_people_only() {
        let color = Style {
            color: true,
            ascii: false,
        };
        for (audience, hint, colored) in [
            (Audience::Human, true, true),
            (Audience::Quiet, false, false),
            (Audience::Agent, false, false),
        ] {
            let (out, err) = (Shared::default(), Shared::default());
            let output = Output::with_streams(
                audience,
                color,
                color,
                Box::new(out.clone()),
                Box::new(err.clone()),
            );
            output.result_with(Symbol::Ok, "Added Mom.");
            output.detail("Automatic replies are off.");
            output.next("textbutler chats on Mom");
            output.warn("Messages access isn't set up yet");
            let code = output.error(&CliError::new("x", "Failed.").with_next("x doctor"));
            assert_eq!(code, 1);
            assert_eq!(out.text().contains("\u{1b}["), colored, "{audience:?}");
            assert!(
                out.text()
                    .ends_with("Added Mom.\n  Automatic replies are off.\n")
                    || colored
            );
            assert_eq!(
                err.text().contains("Next: textbutler chats on Mom\n"),
                hint,
                "{audience:?}"
            );
            assert!(err.text().contains("Failed."));
        }
    }
}

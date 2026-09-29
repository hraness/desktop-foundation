//! Views that render to an interactive terminal, a plain-text snapshot or
//! JSON. Without a terminal on stdout, `tui` prints a snapshot, so agents
//! and logs get the same screen a person sees.
//!
//! Keys in interactive mode: Tab and Shift-Tab switch views, `r` reloads,
//! `q` or Esc quits.

use std::io::{self, IsTerminal, Write};
use std::time::Duration;

use ratatui::backend::TestBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Tabs};
use ratatui::{Frame, Terminal};
use serde::Serialize;

use crate::envelope::{self, Envelope};

pub use crossterm;
pub use ratatui;
pub use ratatui_textarea;

/// The width a snapshot uses unless told otherwise.
pub const DEFAULT_SNAPSHOT_WIDTH: u16 = 80;

/// One screen of a product's state.
pub trait View<S> {
    /// A stable id such as `status`.
    fn id(&self) -> &str;
    /// The tab label.
    fn title(&self) -> &str;
    /// Draws `state` into `area`.
    fn render(&self, state: &S, frame: &mut Frame, area: Rect);
    /// The rows a snapshot of `state` needs at `width`.
    fn height(&self, _state: &S, _width: u16) -> u16 {
        24
    }
}

/// How `tui` shows its views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Interactive,
    Snapshot,
    Json,
}

/// `--json` wins, then `--snapshot`; otherwise interactive only when
/// stdout is a terminal.
pub fn choose_mode(json: bool, snapshot: bool, stdout_is_terminal: bool) -> Mode {
    if json {
        Mode::Json
    } else if snapshot || !stdout_is_terminal {
        Mode::Snapshot
    } else {
        Mode::Interactive
    }
}

/// [`choose_mode`] for this process's stdout.
pub fn mode_for_stdout(json: bool, snapshot: bool) -> Mode {
    choose_mode(json, snapshot, io::stdout().is_terminal())
}

fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    let area = buffer.area;
    let mut lines = Vec::with_capacity(area.height as usize);
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Renders one view to plain text at `width` through ratatui's
/// `TestBackend`. Trailing spaces and blank rows are trimmed.
pub fn render_to_string<S>(view: &dyn View<S>, state: &S, width: u16) -> String {
    let width = width.max(20);
    let height = view.height(state, width).max(1);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
    terminal
        .draw(|frame| view.render(state, frame, frame.area()))
        .expect("draw to test backend");
    buffer_text(terminal.backend().buffer())
}

/// What [`run`] needs.
pub struct RunOptions<'a, S> {
    /// Loads fresh state. Called once, and again on `r`.
    pub load: Box<dyn FnMut() -> Envelope<S> + 'a>,
    pub views: Vec<Box<dyn View<S> + 'a>>,
    pub mode: Mode,
    /// Snapshot width; defaults to [`DEFAULT_SNAPSHOT_WIDTH`].
    pub width: Option<u16>,
}

/// Shows the views and returns the exit status. JSON prints the envelope;
/// a snapshot prints every view, each under its title; interactive mode
/// takes over the terminal until `q`.
pub fn run<S: Serialize>(mut opts: RunOptions<'_, S>, out: &mut dyn Write) -> u8 {
    let loaded = (opts.load)();
    match opts.mode {
        Mode::Json => envelope::emit(&mut WriteRef(out), &loaded),
        Mode::Snapshot => {
            let width = opts.width.unwrap_or(DEFAULT_SNAPSHOT_WIDTH);
            let code = loaded.exit_code();
            let text = match &loaded {
                Envelope::Ok { data, .. } => opts
                    .views
                    .iter()
                    .map(|v| {
                        format!(
                            "== {} ==\n{}",
                            v.title(),
                            render_to_string(v.as_ref(), data, width)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                Envelope::Err { error, .. } => format!("{}: {}\n", error.code, error.message),
            };
            match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
                Ok(()) => code,
                Err(_) => envelope::EXIT_FAILURE,
            }
        }
        Mode::Interactive => match interactive(loaded, &mut opts) {
            Ok(code) => code,
            Err(_) => envelope::EXIT_FAILURE,
        },
    }
}

struct WriteRef<'a>(&'a mut dyn Write);
impl Write for WriteRef<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// The key handling of interactive mode, kept pure for tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Next,
    Previous,
    Reload,
    Quit,
    None,
}

pub fn action_for(key: crossterm::event::KeyEvent) -> Action {
    use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
    if key.kind == KeyEventKind::Release {
        return Action::None;
    }
    match key.code {
        KeyCode::Tab => Action::Next,
        KeyCode::BackTab => Action::Previous,
        KeyCode::Char('r') => Action::Reload,
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
        _ => Action::None,
    }
}

fn draw<S>(
    frame: &mut Frame,
    views: &[Box<dyn View<S> + '_>],
    selected: usize,
    state: &Envelope<S>,
) {
    let [tabs, body, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());
    frame.render_widget(
        Tabs::new(views.iter().map(|v| Line::from(v.title().to_string())))
            .select(selected)
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        tabs,
    );
    match state {
        Envelope::Ok { data, .. } => {
            if let Some(view) = views.get(selected) {
                view.render(data, frame, body)
            }
        }
        Envelope::Err { error, .. } => frame.render_widget(
            Paragraph::new(format!("{}: {}", error.code, error.message)),
            body,
        ),
    }
    frame.render_widget(
        Paragraph::new("tab switch · r reload · q quit")
            .style(Style::default().add_modifier(Modifier::DIM)),
        help,
    );
}

fn interactive<S>(mut state: Envelope<S>, opts: &mut RunOptions<'_, S>) -> io::Result<u8> {
    use crossterm::event::{self, Event};
    let mut terminal = ratatui::try_init()?;
    let mut selected = 0usize;
    let count = opts.views.len().max(1);
    let result = (|| -> io::Result<u8> {
        loop {
            terminal.draw(|f| draw(f, &opts.views, selected, &state))?;
            if !event::poll(Duration::from_millis(250))? {
                continue;
            }
            if let Event::Key(key) = event::read()? {
                match action_for(key) {
                    Action::Next => selected = (selected + 1) % count,
                    Action::Previous => selected = (selected + count - 1) % count,
                    Action::Reload => state = (opts.load)(),
                    Action::Quit => return Ok(state.exit_code()),
                    Action::None => {}
                }
            }
        }
    })();
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{ErrorBody, ErrorCode};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::widgets::{Block, Borders, Row, Table};

    #[derive(Serialize)]
    struct Status {
        owner: &'static str,
        pending: Vec<(&'static str, &'static str)>,
    }

    struct StatusView;
    impl View<Status> for StatusView {
        fn id(&self) -> &str {
            "status"
        }
        fn title(&self) -> &str {
            "Status"
        }
        fn height(&self, s: &Status, _: u16) -> u16 {
            s.pending.len() as u16 + 3
        }
        fn render(&self, s: &Status, frame: &mut Frame, area: Rect) {
            let rows = s
                .pending
                .iter()
                .map(|(id, what)| Row::new(vec![*id, *what]));
            let table = Table::new(rows, [Constraint::Length(6), Constraint::Fill(1)])
                .header(Row::new(vec!["ID", "Waiting request"]))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!(" Owner {} ", s.owner)),
                );
            frame.render_widget(table, area);
        }
    }

    fn state() -> Status {
        Status {
            owner: "running",
            pending: vec![
                (
                    "a1",
                    "codex wants network access to api.github.com for a release check",
                ),
                ("b2", "claude wants to read ~/Documents/notes"),
            ],
        }
    }

    #[test]
    fn snapshots_match_goldens_at_three_widths() {
        for width in [40u16, 80, 120] {
            let got = render_to_string(&StatusView, &state(), width);
            let path = format!(
                "{}/tests/golden/tui-status-{width}.txt",
                env!("CARGO_MANIFEST_DIR")
            );
            if std::env::var_os("UPDATE_GOLDEN").is_some() {
                std::fs::write(&path, &got).unwrap();
            }
            let want = std::fs::read_to_string(&path).unwrap();
            assert_eq!(got, want, "width {width}");
            assert!(got.lines().all(|l| l.chars().count() <= width as usize));
        }
    }

    #[test]
    fn mode_defaults_to_snapshot_without_a_terminal() {
        assert_eq!(choose_mode(false, false, false), Mode::Snapshot);
        assert_eq!(choose_mode(false, false, true), Mode::Interactive);
        assert_eq!(choose_mode(false, true, true), Mode::Snapshot);
        assert_eq!(choose_mode(true, false, true), Mode::Json);
    }

    #[test]
    fn run_prints_snapshots_json_and_errors() {
        let views = || -> Vec<Box<dyn View<Status>>> { vec![Box::new(StatusView)] };
        let mut out = Vec::new();
        let code = run(
            RunOptions {
                load: Box::new(|| Envelope::ok("example.status/1", state())),
                views: views(),
                mode: Mode::Snapshot,
                width: None,
            },
            &mut out,
        );
        assert_eq!(code, 0);
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("== Status ==\n"));
        assert!(text.contains("a1"));

        let mut out = Vec::new();
        let code = run(
            RunOptions {
                load: Box::new(|| Envelope::ok("example.status/1", state())),
                views: views(),
                mode: Mode::Json,
                width: None,
            },
            &mut out,
        );
        assert_eq!(code, 0);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["schema"], "example.status/1");

        let mut out = Vec::new();
        let code = run(
            RunOptions {
                load: Box::new(|| {
                    Envelope::error(ErrorBody::new(ErrorCode::OwnerUnavailable, "Not running."))
                }),
                views: views(),
                mode: Mode::Snapshot,
                width: Some(40),
            },
            &mut out,
        );
        assert_eq!(code, envelope::EXIT_OWNER_UNAVAILABLE);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "owner-unavailable: Not running.\n"
        );
    }

    #[test]
    fn keys_map_to_actions() {
        let k = |c| KeyEvent::new(c, KeyModifiers::NONE);
        assert_eq!(action_for(k(KeyCode::Tab)), Action::Next);
        assert_eq!(action_for(k(KeyCode::BackTab)), Action::Previous);
        assert_eq!(action_for(k(KeyCode::Char('r'))), Action::Reload);
        assert_eq!(action_for(k(KeyCode::Char('q'))), Action::Quit);
        assert_eq!(
            action_for(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Action::Quit
        );
        assert_eq!(action_for(k(KeyCode::Char('x'))), Action::None);
    }
    /// The columns a string takes in a ratatui buffer. The TypeScript kit's
    /// `columns()` must give the same number for every case in
    /// contract/text-width.json.
    #[test]
    fn text_width_corpus_matches_contract() {
        let path = format!(
            "{}/../../contract/text-width.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let update = std::env::var_os("UPDATE_GOLDEN").is_some();
        let cases = doc["cases"].as_array_mut().unwrap();
        assert!(!cases.is_empty());
        for case in cases.iter_mut() {
            let text = case["text"].as_str().unwrap().to_owned();
            let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 400, 1));
            let (x, _) = buf.set_stringn(0, 0, &text, usize::MAX, ratatui::style::Style::default());
            if update {
                case["columns"] = serde_json::json!(x);
            }
            assert_eq!(case["columns"].as_u64(), Some(u64::from(x)), "{text:?}");
        }
        if update {
            std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap() + "\n").unwrap();
        }
    }
}
// probe: throwaway, will be closed

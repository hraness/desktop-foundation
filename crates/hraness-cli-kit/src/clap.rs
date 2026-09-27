//! Usage errors for CLIs built on clap 4 (feature `clap`).
//!
//! clap's own usage error is several lines with a usage dump. The contract
//! asks for one sentence, the closest match, and the help to read next:
//!
//! ```text
//! ✗ Unknown command "stauts". Did you mean "status"?
//! → textbutler --help
//! ```
//!
//! exit 2. With `--json` or an agent reader the same error is JSON on stdout:
//! `{"ok":false,"error":{"code":"usage","message":"…","next":"…"}}`. Help and
//! version requests print to stdout and exit 0.
//!
//! ```no_run
//! use hraness_cli_kit::clap::{exit_on_parse_error, UsageOptions};
//! // With clap's derive API this is `Cli::command()` and `Cli::try_parse_from`.
//! let root = clap::Command::new("demo").subcommand(clap::Command::new("status"));
//! let args: Vec<String> = std::env::args().collect();
//! let matches = match root.clone().try_get_matches_from(&args) {
//!     Ok(matches) => matches,
//!     Err(error) => std::process::exit(exit_on_parse_error(error, &root, &args, &UsageOptions::default())),
//! };
//! ```

use crate::audience::{self, Audience};
use crate::style::{write_stdout, CliError};
use clap::error::{ContextKind, ContextValue, ErrorKind};

/// Options for [`usage_error`] and [`exit_on_parse_error`].
#[derive(Debug, Clone, Default)]
pub struct UsageOptions {
    /// The CLI name used in `→ <cli> --help`. Defaults to the root
    /// command's name.
    pub cli: Option<String>,
    /// Extra suggestions for words that are not commands at this level:
    /// `("status", "proxy status")` makes `stauts` suggest `proxy status`.
    pub aliases: Vec<(String, String)>,
    /// The reader. `None` detects it from the environment.
    pub audience: Option<Audience>,
}

impl UsageOptions {
    pub fn cli(mut self, cli: impl Into<String>) -> Self {
        self.cli = Some(cli.into());
        self
    }

    pub fn alias(mut self, word: impl Into<String>, command: impl Into<String>) -> Self {
        self.aliases.push((word.into(), command.into()));
        self
    }

    pub fn audience(mut self, audience: Audience) -> Self {
        self.audience = Some(audience);
        self
    }
}

/// True when `args` asks for JSON (`--json` before any `--`).
pub fn wants_json(args: &[String]) -> bool {
    args.iter()
        .skip(1)
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "--json")
}

/// The subcommand path the arguments name, as far as they name known
/// subcommands: `["proxy", "serve"]`.
pub fn command_path(root: &clap::Command, args: &[String]) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = root;
    for arg in args.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg.starts_with('-') {
            continue;
        }
        match current.find_subcommand(arg) {
            Some(sub) => {
                path.push(sub.get_name().to_owned());
                current = sub;
            }
            None => break,
        }
    }
    path
}

fn find_path<'a>(root: &'a clap::Command, path: &[String]) -> &'a clap::Command {
    path.iter().fold(root, |current, name| {
        current.find_subcommand(name).unwrap_or(current)
    })
}

/// Optimal string alignment distance: edits, where swapping two neighbours
/// counts once (`stauts` is one edit from `status`).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[a.len()][b.len()]
}

/// The closest candidate to `input`, if any is close enough: a unique
/// prefix of at least three letters, or at most one edit per three letters.
/// Each candidate is `(word to compare, text to suggest)`.
pub fn closest(input: &str, candidates: &[(String, String)]) -> Option<String> {
    let lower = input.to_lowercase();
    if lower.chars().count() >= 3 {
        let mut prefixed = candidates
            .iter()
            .filter(|(word, _)| word.starts_with(&lower));
        if let (Some((_, only)), None) = (prefixed.next(), prefixed.next()) {
            return Some(only.clone());
        }
    }
    let limit = (lower.chars().count() / 3).max(1);
    candidates
        .iter()
        .map(|(word, suggestion)| (edit_distance(&lower, word), suggestion))
        .filter(|(distance, _)| *distance <= limit)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, suggestion)| suggestion.clone())
}

fn first(value: Option<&ContextValue>) -> Option<String> {
    match value? {
        ContextValue::String(text) => Some(text.clone()),
        ContextValue::Strings(list) => list.first().cloned(),
        _ => None,
    }
}

fn all(value: Option<&ContextValue>) -> Vec<String> {
    match value {
        Some(ContextValue::String(text)) => vec![text.clone()],
        Some(ContextValue::Strings(list)) => list.clone(),
        _ => vec![],
    }
}

/// `<SESSION>` → `<session>`, `--limit <N>` → `--limit`.
fn arg_name(raw: &str) -> String {
    let raw = raw.trim();
    if raw.starts_with('-') {
        return raw.split([' ', '=']).next().unwrap_or(raw).to_owned();
    }
    raw.to_lowercase()
}

/// The first line of clap's own message, as a sentence.
fn clap_sentence(error: &clap::Error) -> String {
    let rendered = error.render().to_string();
    let line = rendered
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim_start_matches("error: ")
        .trim();
    crate::style::sentence(line, &[])
}

/// The one-sentence usage error for a clap parse failure, or `None` when
/// clap is answering a help or version request instead.
pub fn usage_error(
    error: &clap::Error,
    root: &clap::Command,
    args: &[String],
    options: &UsageOptions,
) -> Option<CliError> {
    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp
            | ErrorKind::DisplayVersion
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    ) {
        return None;
    }
    let cli = options
        .cli
        .clone()
        .unwrap_or_else(|| root.get_name().to_owned());
    let path = command_path(root, args);
    let help = if path.is_empty() {
        format!("{cli} --help")
    } else {
        format!("{cli} {} --help", path.join(" "))
    };
    let (message, detail) = match error.kind() {
        ErrorKind::InvalidSubcommand => {
            let name = first(error.get(ContextKind::InvalidSubcommand)).unwrap_or_default();
            let current = find_path(root, &path);
            let mut candidates: Vec<(String, String)> = current
                .get_subcommands()
                .filter(|sub| !sub.is_hide_set())
                .flat_map(|sub| {
                    let name = sub.get_name().to_owned();
                    std::iter::once((name.clone(), name.clone())).chain(
                        sub.get_visible_aliases()
                            .map(move |alias| (alias.to_owned(), name.clone())),
                    )
                })
                .collect();
            if path.is_empty() {
                candidates.extend(options.aliases.iter().cloned());
            }
            match closest(&name, &candidates) {
                Some(suggestion) => (
                    format!("Unknown command \"{name}\". Did you mean \"{suggestion}\"?"),
                    None,
                ),
                None => (format!("Unknown command \"{name}\"."), None),
            }
        }
        ErrorKind::UnknownArgument => {
            let name = first(error.get(ContextKind::InvalidArg)).unwrap_or_default();
            match first(error.get(ContextKind::SuggestedArg)) {
                Some(suggestion) => (
                    format!(
                        "Unknown option \"{name}\". Did you mean \"{}\"?",
                        arg_name(&suggestion)
                    ),
                    None,
                ),
                None if !name.starts_with('-') => {
                    (format!("Unexpected argument \"{name}\"."), None)
                }
                None => (format!("Unknown option \"{name}\"."), None),
            }
        }
        ErrorKind::MissingRequiredArgument => {
            let names: Vec<String> = all(error.get(ContextKind::InvalidArg))
                .iter()
                .map(|name| arg_name(name))
                .collect();
            (format!("Missing {}.", names.join(", ")), None)
        }
        ErrorKind::MissingSubcommand => ("Missing a command.".to_owned(), None),
        ErrorKind::InvalidValue => {
            let value = first(error.get(ContextKind::InvalidValue)).unwrap_or_default();
            let arg = first(error.get(ContextKind::InvalidArg))
                .map(|arg| arg_name(&arg))
                .unwrap_or_default();
            let valid = all(error.get(ContextKind::ValidValue));
            let message = if value.is_empty() {
                format!("{arg} needs a value.")
            } else {
                format!("\"{value}\" isn't a valid value for {arg}.")
            };
            let detail =
                (!valid.is_empty()).then(|| format!("Choose one of: {}.", valid.join(", ")));
            (message, detail)
        }
        ErrorKind::InvalidUtf8 => ("Arguments must be valid UTF-8.".to_owned(), None),
        _ => (clap_sentence(error), None),
    };
    let mut usage = CliError::usage(message, help);
    usage.detail = detail;
    Some(usage)
}

/// Handle a clap parse failure the Hraness way and return the exit code:
/// help and version print to stdout and return 0 (including a command group
/// run without its subcommand); usage errors print one sentence and the help
/// to read, and return 2. `--json` or an agent reader gets the JSON error on
/// stdout.
pub fn exit_on_parse_error(
    error: clap::Error,
    root: &clap::Command,
    args: &[String],
    options: &UsageOptions,
) -> i32 {
    let Some(usage) = usage_error(&error, root, args, options) else {
        write_stdout(&error.render().to_string());
        return 0;
    };
    let audience = options.audience.unwrap_or_else(audience::detect_current);
    usage.report(wants_json(args), audience)
}

/// Help lines wider than `width` columns in `command` and every visible
/// subcommand, as `(command path, line)`, for a test that keeps help
/// readable in a narrow terminal.
pub fn help_lines_over(command: &clap::Command, width: usize) -> Vec<(String, String)> {
    fn walk(command: &clap::Command, path: String, width: usize, out: &mut Vec<(String, String)>) {
        let help = command.clone().render_long_help().to_string();
        for line in help.lines() {
            if line.chars().count() > width {
                out.push((path.clone(), line.to_owned()));
            }
        }
        // clap's generated `help` subcommand repeats the tree; skip it.
        for sub in command
            .get_subcommands()
            .filter(|sub| !sub.is_hide_set() && sub.get_name() != "help")
        {
            walk(sub, format!("{path} {}", sub.get_name()), width, out);
        }
    }
    let mut built = command.clone();
    built.build();
    let mut out = Vec::new();
    walk(&built, built.get_name().to_owned(), width, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Arg, Command};

    fn cli() -> Command {
        Command::new("demo")
            .version("1.2.3")
            .subcommand_required(false)
            .subcommand(Command::new("status").about("Show status"))
            .subcommand(Command::new("vault").about("List snapshots"))
            .subcommand(
                Command::new("proxy")
                    .about("Run the proxy")
                    .subcommand_required(true)
                    .arg_required_else_help(true)
                    .subcommand(
                        Command::new("serve")
                            .arg(Arg::new("port").long("port").value_parser(["80", "8260"])),
                    )
                    .subcommand(Command::new("status")),
            )
            .subcommand(Command::new("plan").arg(Arg::new("session").required(true)))
            .subcommand(Command::new("secret").hide(true))
            .arg(
                Arg::new("json")
                    .long("json")
                    .global(true)
                    .action(clap::ArgAction::SetTrue),
            )
    }

    fn fail(args: &[&str], options: &UsageOptions) -> CliError {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let error = cli().try_get_matches_from(&args).expect_err("parse fails");
        usage_error(&error, &cli(), &args, options).expect("usage error")
    }

    #[test]
    fn unknown_commands_suggest_the_closest_match() {
        let error = fail(&["demo", "stauts"], &UsageOptions::default());
        assert_eq!(
            error.message,
            "Unknown command \"stauts\". Did you mean \"status\"?"
        );
        assert_eq!(error.next.as_deref(), Some("demo --help"));
        assert_eq!(error.exit_code, 2);
        assert_eq!(
            error.render_human(crate::Style::PLAIN),
            "✗ Unknown command \"stauts\". Did you mean \"status\"?\n→ demo --help\n"
        );
        assert_eq!(
            error.render_json(),
            r#"{"ok":false,"error":{"code":"usage","message":"Unknown command \"stauts\". Did you mean \"status\"?","next":"demo --help"}}"#
        );
        assert_eq!(
            fail(&["demo", "zzzzzz"], &UsageOptions::default()).message,
            "Unknown command \"zzzzzz\"."
        );
        // Hidden commands are never suggested.
        assert_eq!(
            fail(&["demo", "secrte"], &UsageOptions::default()).message,
            "Unknown command \"secrte\"."
        );
        // Nested: the suggestion and the help belong to the group.
        let nested = fail(
            &["demo", "proxy", "srve"],
            &UsageOptions::default().cli("demo"),
        );
        assert_eq!(
            nested.message,
            "Unknown command \"srve\". Did you mean \"serve\"?"
        );
        assert_eq!(nested.next.as_deref(), Some("demo proxy --help"));
        // A unique prefix counts.
        assert_eq!(
            fail(&["demo", "pro"], &UsageOptions::default()).message,
            "Unknown command \"pro\". Did you mean \"proxy\"?"
        );
    }

    #[test]
    fn aliases_suggest_commands_that_live_elsewhere() {
        let tree = Command::new("gob")
            .subcommand(Command::new("vault"))
            .subcommand(Command::new("proxy").subcommand(Command::new("status")));
        let args: Vec<String> = ["gob", "stauts"]
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect();
        let error = tree.clone().try_get_matches_from(&args).unwrap_err();
        let options = UsageOptions::default().alias("status", "proxy status");
        assert_eq!(
            usage_error(&error, &tree, &args, &options).unwrap().message,
            "Unknown command \"stauts\". Did you mean \"proxy status\"?"
        );
    }

    #[test]
    fn options_values_and_missing_arguments() {
        let unknown = fail(&["demo", "status", "--jsno"], &UsageOptions::default());
        assert_eq!(
            unknown.message,
            "Unknown option \"--jsno\". Did you mean \"--json\"?"
        );
        assert_eq!(unknown.next.as_deref(), Some("demo status --help"));
        let missing = fail(&["demo", "plan"], &UsageOptions::default());
        assert_eq!(missing.message, "Missing <session>.");
        assert_eq!(missing.next.as_deref(), Some("demo plan --help"));
        let value = fail(
            &["demo", "proxy", "serve", "--port", "9"],
            &UsageOptions::default(),
        );
        assert_eq!(value.message, "\"9\" isn't a valid value for --port.");
        assert_eq!(value.detail.as_deref(), Some("Choose one of: 80, 8260."));
        let extra = fail(&["demo", "status", "extra"], &UsageOptions::default());
        assert_eq!(extra.message, "Unexpected argument \"extra\".");
    }

    #[test]
    fn help_and_version_are_answers_not_errors() {
        for args in [
            &["demo", "--help"][..],
            &["demo", "--version"],
            &["demo", "proxy"],
        ] {
            let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
            let error = cli().try_get_matches_from(&args).unwrap_err();
            assert!(
                usage_error(&error, &cli(), &args, &UsageOptions::default()).is_none(),
                "{args:?}"
            );
        }
    }

    #[test]
    fn json_is_asked_for_before_the_terminator() {
        let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        assert!(wants_json(&args(&["x", "status", "--json"])));
        assert!(!wants_json(&args(&["x", "run", "--", "--json"])));
        assert!(!wants_json(&args(&["--json"])), "argv[0] is the program");
    }

    #[test]
    fn distances_count_a_swap_once() {
        assert_eq!(edit_distance("stauts", "status"), 1);
        assert_eq!(edit_distance("status", "status"), 0);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("vault", "stauts"), 4);
    }

    #[test]
    fn help_width_check_reports_long_lines() {
        let wide = Command::new("wide").subcommand(Command::new("sub").about("x".repeat(120)));
        let over = help_lines_over(&wide, 100);
        assert_eq!(over.len(), 2, "{over:?}");
        assert!(over.iter().any(|(path, _)| path == "wide sub"));
        assert!(help_lines_over(&cli(), 100).is_empty());
    }
}

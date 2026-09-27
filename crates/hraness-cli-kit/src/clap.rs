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
/// subcommands: `["proxy", "serve"]`. An option's value (`--state <dir>`)
/// is skipped, not read as a subcommand.
pub fn command_path(root: &clap::Command, args: &[String]) -> Vec<String> {
    let mut path = Vec::new();
    // The commands entered so far: an option may belong to any of them
    // (global options live on the root).
    let mut stack = vec![root];
    let mut words = args.iter().skip(1);
    while let Some(arg) = words.next() {
        if arg == "--" {
            break;
        }
        if let Some(flag) = arg.strip_prefix('-') {
            if !arg.contains('=') && option_takes_value(&stack, flag) {
                words.next();
            }
            continue;
        }
        let current = stack[stack.len() - 1];
        match current.find_subcommand(arg) {
            Some(sub) => {
                path.push(sub.get_name().to_owned());
                stack.push(sub);
            }
            None => break,
        }
    }
    path
}

/// True when `flag` (`-state` for `--state`, `s` for `-s`) names an option
/// that takes its value from the next word.
fn option_takes_value(stack: &[&clap::Command], flag: &str) -> bool {
    let matches = |arg: &clap::Arg| match flag.strip_prefix('-') {
        Some(long) => {
            arg.get_long() == Some(long)
                || arg
                    .get_all_aliases()
                    .is_some_and(|aliases| aliases.contains(&long))
        }
        // `-s` alone; `-svalue` carries its value.
        None => {
            flag.chars().count() == 1 && arg.get_short().map(String::from).as_deref() == Some(flag)
        }
    };
    stack.iter().rev().any(|command| {
        command
            .get_arguments()
            .any(|arg| matches(arg) && arg.get_action().takes_values())
    })
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
        // A name and its alias lead to one suggestion, so both matching the
        // prefix is still one answer.
        let mut prefixed: Vec<&String> = candidates
            .iter()
            .filter(|(word, _)| word.starts_with(&lower))
            .map(|(_, suggestion)| suggestion)
            .collect();
        prefixed.sort();
        prefixed.dedup();
        if let [only] = prefixed.as_slice() {
            return Some((*only).clone());
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
            let mut message = if value.is_empty() {
                format!("{arg} needs a value.")
            } else {
                format!("\"{value}\" isn't a valid value for {arg}.")
            };
            // One line: the contract allows the sentence and the next step only.
            if !valid.is_empty() {
                message.push_str(&format!(" Choose one of: {}.", valid.join(", ")));
            }
            (message, None)
        }
        ErrorKind::ValueValidation => {
            let value = first(error.get(ContextKind::InvalidValue)).unwrap_or_default();
            let arg = first(error.get(ContextKind::InvalidArg))
                .map(|arg| arg_name(&arg))
                .unwrap_or_default();
            let why = std::error::Error::source(error)
                .map(|source| format!(": {}", source.to_string().trim_end_matches('.')))
                .unwrap_or_default();
            (
                format!("\"{value}\" isn't a valid value for {arg}{why}."),
                None,
            )
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

/// Cap help at `width` columns for `command` and every subcommand. clap
/// applies `max_term_width` to one command only, so a root setting leaves
/// subcommand help as wide as the terminal. Wrapping needs clap's
/// `wrap_help` feature in the product.
pub fn cap_help_width(command: clap::Command, width: usize) -> clap::Command {
    command
        .max_term_width(width)
        .mut_subcommands(move |sub| cap_help_width(sub, width))
}

/// Help lines wider than `width` columns in `command` and every visible
/// subcommand, as `(command path, line)`, when clap wraps help at `width`:
/// the lines wrapping can't fix (usage lines, `override_help` text, or every
/// long line when the product lacks clap's `wrap_help` feature). Pair it with
/// [`cap_help_width`] and a golden run under `COLUMNS=200` for the cap.
pub fn help_lines_over(command: &clap::Command, width: usize) -> Vec<(String, String)> {
    fn walk(command: &clap::Command, path: String, width: usize, out: &mut Vec<(String, String)>) {
        let help = command
            .clone()
            .term_width(width)
            .render_long_help()
            .to_string();
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
            .arg(Arg::new("state").long("state").short('s').global(true))
    }

    #[test]
    fn a_name_and_its_alias_are_one_suggestion() {
        let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect()
        };
        let candidates = pairs(&[("status", "status"), ("vault", "vault"), ("stat", "status")]);
        assert_eq!(closest("sta", &candidates).as_deref(), Some("status"));
        let two = pairs(&[("status", "status"), ("start", "start")]);
        assert_eq!(closest("sta", &two), None);
    }

    #[test]
    fn option_values_are_not_read_as_commands() {
        let path = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
            command_path(&cli(), &args)
        };
        assert_eq!(
            path(&["demo", "--state", "proxy", "proxy", "serve"]),
            ["proxy", "serve"]
        );
        assert_eq!(path(&["demo", "-s", "status", "proxy"]), ["proxy"]);
        assert_eq!(
            path(&["demo", "--state=x", "proxy", "--port", "80", "x"]),
            ["proxy"]
        );
        assert_eq!(
            path(&["demo", "--json", "proxy", "status"]),
            ["proxy", "status"]
        );
        assert_eq!(
            path(&["demo", "proxy", "--state", "serve", "status"]),
            ["proxy", "status"]
        );
        let error = fail(
            &["demo", "--state", "/tmp/x", "proxy", "stat"],
            &UsageOptions::default(),
        );
        assert_eq!(
            error.message,
            "Unknown command \"stat\". Did you mean \"status\"?"
        );
        assert_eq!(error.next.as_deref(), Some("demo proxy --help"));
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
        assert_eq!(
            value.message,
            "\"9\" isn't a valid value for --port. Choose one of: 80, 8260."
        );
        assert_eq!(value.detail, None);
        let ranged = Command::new("demo").arg(
            Arg::new("percent")
                .long("percent")
                .value_parser(clap::value_parser!(u8).range(0..=60)),
        );
        let args: Vec<String> = ["demo", "--percent", "61"]
            .iter()
            .map(|arg| (*arg).to_owned())
            .collect();
        let error = ranged.clone().try_get_matches_from(&args).unwrap_err();
        assert_eq!(
            usage_error(&error, &ranged, &args, &UsageOptions::default())
                .unwrap()
                .message,
            "\"61\" isn't a valid value for --percent: 61 is not in 0..=60."
        );
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
        // The cap keeps the tree intact.
        let capped = cap_help_width(wide, 100);
        assert!(capped.find_subcommand("sub").is_some());
    }
}

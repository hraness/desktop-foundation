//! Who is reading a CLI's output. One rule for every Hraness CLI; the
//! TypeScript twin is `@hraness/desktop-foundation/audience`. See
//! `docs/permissions.md` § Audience.

use std::io::IsTerminal as _;

/// Who reads this process's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Audience {
    /// A person at a terminal: text, color, hints and prompts.
    Human,
    /// A coding agent: JSON for commands that have a `--json` form, errors
    /// as JSON on stdout, no prompts.
    Agent,
    /// Anything else (a pipe, a service): plain text, no color, no hints,
    /// never waits for input.
    Quiet,
}

impl Audience {
    pub fn as_str(self) -> &'static str {
        match self {
            Audience::Human => "human",
            Audience::Agent => "agent",
            Audience::Quiet => "quiet",
        }
    }
}

/// Exact environment names that mark an agent session. Only exact names
/// count: a prefix such as `CODEX_` also matches human configuration like
/// `CODEX_HOME`, and `DEVIN_API_KEY` is a person's setting.
pub const AGENT_MARKERS: [&str; 6] = [
    "AI_AGENT",
    "CLAUDECODE",
    "CODEX_SANDBOX",
    "CODEX_SANDBOX_NETWORK_DISABLED",
    "CURSOR_AGENT",
    "GEMINI_CLI",
];

/// The rule, in order:
///
/// 1. `HRANESS_AUDIENCE` = `human` | `agent` | `quiet` (`off` means `quiet`)
///    wins. Other values are ignored.
/// 2. Any agent marker with a nonempty value → `Agent`.
/// 3. stderr is a terminal → `Human`.
/// 4. Otherwise → `Quiet`.
pub fn detect(env: &dyn Fn(&str) -> Option<String>, stderr_is_tty: bool) -> Audience {
    if let Some(value) = env("HRANESS_AUDIENCE") {
        match value.trim().to_ascii_lowercase().as_str() {
            "human" => return Audience::Human,
            "agent" => return Audience::Agent,
            "quiet" | "off" => return Audience::Quiet,
            _ => {}
        }
    }
    if AGENT_MARKERS
        .iter()
        .any(|name| env(name).is_some_and(|value| !value.is_empty()))
    {
        return Audience::Agent;
    }
    if stderr_is_tty {
        Audience::Human
    } else {
        Audience::Quiet
    }
}

/// The process environment, as the `env` argument the kit takes everywhere.
pub fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// [`detect`] for this process.
pub fn detect_current() -> Audience {
    detect(&process_env, std::io::stderr().is_terminal())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn follows_the_shared_rule() {
        assert_eq!(detect(&env_of(&[]), true), Audience::Human);
        assert_eq!(detect(&env_of(&[]), false), Audience::Quiet);
        for marker in AGENT_MARKERS {
            assert_eq!(
                detect(&env_of(&[(marker, "1")]), true),
                Audience::Agent,
                "{marker}"
            );
        }
        assert_eq!(
            detect(&env_of(&[("CLAUDECODE", "")]), true),
            Audience::Human
        );
        // Prefixes never count.
        for human in [
            "CODEX_HOME",
            "CODEX_",
            "DEVIN_API_KEY",
            "CLAUDE_CONFIG_DIR",
            "AI_AGENTS",
        ] {
            assert_eq!(
                detect(&env_of(&[(human, "x")]), false),
                Audience::Quiet,
                "{human}"
            );
        }
        assert_eq!(
            detect(
                &env_of(&[("HRANESS_AUDIENCE", "human"), ("AI_AGENT", "1")]),
                false
            ),
            Audience::Human
        );
        assert_eq!(
            detect(&env_of(&[("HRANESS_AUDIENCE", " Agent ")]), false),
            Audience::Agent
        );
        assert_eq!(
            detect(&env_of(&[("HRANESS_AUDIENCE", "off")]), true),
            Audience::Quiet
        );
        assert_eq!(
            detect(&env_of(&[("HRANESS_AUDIENCE", "quiet")]), true),
            Audience::Quiet
        );
        // Unknown overrides are ignored.
        assert_eq!(
            detect(&env_of(&[("HRANESS_AUDIENCE", "robot")]), true),
            Audience::Human
        );
        assert_eq!(Audience::Agent.as_str(), "agent");
    }
}

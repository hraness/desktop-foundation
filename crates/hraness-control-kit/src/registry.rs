//! Verbs with an operation class. A `decide` verb cannot be registered
//! without a human gate, and `commands --json` lists every verb with its
//! class so agents and reviewers can see what needs a person.

use serde::{Deserialize, Serialize};

use crate::envelope::{Audience, Envelope, ErrorBody, ErrorCode, NextStep};

/// The schema of `commands --json`.
pub const COMMANDS_SCHEMA: &str = "hraness.commands/1";

/// What a verb does, from `contract/op-classes.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpClass {
    /// Reads state. Never changes anything.
    Read,
    /// Changes state in a way an agent may do on its own.
    Operate,
    /// A decision that belongs to a person. Needs a gate.
    Decide,
    /// A decision whose current behaviour predates the gate.
    DecideLegacy,
}

impl OpClass {
    pub fn as_str(self) -> &'static str {
        match self {
            OpClass::Read => "read",
            OpClass::Operate => "operate",
            OpClass::Decide => "decide",
            OpClass::DecideLegacy => "decide-legacy",
        }
    }
}

/// The human gate a verb needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GateTier {
    /// A controlling terminal in the foreground and a one-time code.
    T1T2,
    /// Reserved for OS owner authentication. Answers `unsupported-platform`.
    T3,
}

/// One verb.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Verb {
    pub path: Vec<String>,
    pub op_class: OpClass,
    pub schema: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<GateTier>,
    /// For a gated verb whose class depends on its input, such as
    /// `approvals decide <id> --digest <d> deny|allow-once`: which inputs
    /// an agent may run on its own, as `operate` with no gate. The product
    /// checks the input with [`Registry::needs_gate`]; every other input
    /// keeps the gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operate_when: Option<String>,
}

impl Verb {
    pub fn new(path: &[&str], op_class: OpClass, schema: &str, summary: &str) -> Self {
        Self {
            path: path.iter().map(|p| p.to_string()).collect(),
            op_class,
            schema: schema.to_string(),
            summary: summary.to_string(),
            gate: None,
            operate_when: None,
        }
    }

    pub fn gated(mut self, gate: GateTier) -> Self {
        self.gate = Some(gate);
        self
    }

    /// Marks a gated verb whose `summary` inputs run as `operate`, such
    /// as "deny" on `approvals decide`.
    pub fn operate_when(mut self, summary: &str) -> Self {
        self.operate_when = Some(summary.to_string());
        self
    }

    /// The verb as typed on a command line, such as `approvals decide`.
    pub fn command(&self) -> String {
        self.path.join(" ")
    }
}

/// Why a verb was refused at registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    EmptyPath,
    InvalidSegment(String),
    Duplicate(String),
    DecideWithoutGate(String),
    GateOnUngatedClass(String),
    InvalidSchema(String),
    OperateWhenWithoutGate(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::EmptyPath => write!(f, "a verb needs a path"),
            RegistryError::InvalidSegment(s) => write!(f, "invalid verb segment {s:?}"),
            RegistryError::Duplicate(v) => write!(f, "verb {v:?} is registered twice"),
            RegistryError::DecideWithoutGate(v) => {
                write!(f, "decide verb {v:?} needs a gate tier")
            }
            RegistryError::GateOnUngatedClass(v) => {
                write!(f, "verb {v:?} has a gate but its class needs none")
            }
            RegistryError::InvalidSchema(s) => write!(f, "invalid schema {s:?}"),
            RegistryError::OperateWhenWithoutGate(v) => {
                write!(f, "verb {v:?} has operate_when without a gate or a summary")
            }
        }
    }
}

impl std::error::Error for RegistryError {}

/// Every verb a product exposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    product: String,
    verbs: Vec<Verb>,
}

#[derive(Serialize)]
struct CommandsData<'a> {
    product: &'a str,
    verbs: &'a [Verb],
}

/// A verb word: `[a-z][a-z0-9-]{0,31}`, from `contract/names.json`.
pub fn valid_segment(s: &str) -> bool {
    crate::envelope::valid_product_name(s)
}

fn valid_schema_tail(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// A schema id such as `example.status/1` or `example.2fa/1`, from
/// `contract/names.json`.
pub fn valid_schema(s: &str) -> bool {
    let Some((name, version)) = s.rsplit_once('/') else {
        return false;
    };
    let mut parts = name.split('.');
    let first = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    !version.is_empty()
        && version.len() <= 9
        && version.bytes().all(|b| b.is_ascii_digit())
        && crate::envelope::valid_product_name(first)
        && !rest.is_empty()
        && rest.iter().all(|p| valid_schema_tail(p))
}

impl Registry {
    pub fn new(product: &str) -> Self {
        assert!(
            crate::envelope::valid_product_name(product),
            "invalid product name {product:?}"
        );
        Self {
            product: product.to_string(),
            verbs: Vec::new(),
        }
    }

    pub fn product(&self) -> &str {
        &self.product
    }

    /// Adds a verb, refusing a `decide` verb without a gate and a gate on
    /// a `read` or `operate` verb.
    pub fn register(&mut self, verb: Verb) -> Result<&mut Self, RegistryError> {
        if verb.path.is_empty() {
            return Err(RegistryError::EmptyPath);
        }
        if let Some(bad) = verb.path.iter().find(|s| !valid_segment(s)) {
            return Err(RegistryError::InvalidSegment(bad.clone()));
        }
        if !valid_schema(&verb.schema) {
            return Err(RegistryError::InvalidSchema(verb.schema.clone()));
        }
        let command = verb.command();
        if self.verbs.iter().any(|v| v.path == verb.path) {
            return Err(RegistryError::Duplicate(command));
        }
        match (verb.op_class, verb.gate) {
            (OpClass::Decide, None) => return Err(RegistryError::DecideWithoutGate(command)),
            (OpClass::Read | OpClass::Operate, Some(_)) => {
                return Err(RegistryError::GateOnUngatedClass(command))
            }
            _ => {}
        }
        if verb
            .operate_when
            .as_ref()
            .is_some_and(|s| verb.gate.is_none() || s.is_empty())
        {
            return Err(RegistryError::OperateWhenWithoutGate(command));
        }
        self.verbs.push(verb);
        Ok(self)
    }

    pub fn verbs(&self) -> &[Verb] {
        &self.verbs
    }

    /// Finds the verb with the longest path that prefixes `args`.
    pub fn lookup<S: AsRef<str>>(&self, args: &[S]) -> Option<&Verb> {
        self.verbs
            .iter()
            .filter(|v| {
                v.path.len() <= args.len() && v.path.iter().zip(args).all(|(p, a)| p == a.as_ref())
            })
            .max_by_key(|v| v.path.len())
    }

    /// Whether this run of `verb` needs the human gate. `operate_input`
    /// is the product's answer to the verb's `operate_when`, such as
    /// "the decision is deny"; it is ignored for a verb without one.
    pub fn needs_gate(&self, verb: &Verb, operate_input: bool) -> bool {
        verb.gate.is_some() && !(verb.operate_when.is_some() && operate_input)
    }

    /// The `commands --json` envelope.
    pub fn commands_json(&self) -> Envelope<serde_json::Value> {
        let data = serde_json::to_value(CommandsData {
            product: &self.product,
            verbs: &self.verbs,
        })
        .expect("verbs serialize");
        Envelope::ok(COMMANDS_SCHEMA, data)
    }

    /// The refusal for a gated verb run where no person can answer, such
    /// as `--json` from an agent. It never prompts. `command` is what a
    /// person should run at their own terminal.
    pub fn human_required(&self, verb: &Verb, command: &str) -> ErrorBody {
        ErrorBody::new(
            ErrorCode::HumanRequired,
            format!(
                "`{} {}` is a decision for a person, so nothing changed.",
                self.product,
                verb.command()
            ),
        )
        .with_next(NextStep::new(
            command,
            "Run this in your own terminal to decide.",
            Audience::Human,
        ))
    }
}

#[cfg(feature = "clap")]
impl Registry {
    /// A clap command tree with one subcommand per verb, each taking
    /// `--json`, plus `commands`. Products add their own arguments to the
    /// leaves with [`clap::Command::mut_subcommand`].
    pub fn clap_command(&self) -> clap::Command {
        fn json() -> clap::Arg {
            clap::Arg::new("json")
                .long("json")
                .action(clap::ArgAction::SetTrue)
                .help("Print one JSON envelope")
        }
        fn insert(parent: clap::Command, path: &[String], verb: &Verb) -> clap::Command {
            let (head, rest) = path.split_first().expect("non-empty path");
            let existing = parent.get_subcommands().any(|c| c.get_name() == head);
            let parent = if existing {
                parent
            } else {
                parent.subcommand(clap::Command::new(head.clone()))
            };
            parent.mut_subcommand(head.as_str(), |child| {
                if rest.is_empty() {
                    child.about(verb.summary.clone()).arg(json())
                } else {
                    insert(child.subcommand_required(true), rest, verb)
                }
            })
        }
        let mut root = clap::Command::new(self.product.clone())
            .subcommand_required(true)
            .subcommand(
                clap::Command::new("commands")
                    .about("List every verb and its operation class")
                    .arg(json()),
            );
        for verb in &self.verbs {
            root = insert(root, &verb.path, verb);
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> Registry {
        let mut r = Registry::new("example");
        r.register(Verb::new(
            &["status"],
            OpClass::Read,
            "example.status/1",
            "One-screen health",
        ))
        .unwrap();
        r.register(
            Verb::new(
                &["approvals", "decide"],
                OpClass::Decide,
                "example.approval/1",
                "Allow or deny a waiting request",
            )
            .gated(GateTier::T1T2),
        )
        .unwrap();
        r.register(Verb::new(
            &["control", "stop"],
            OpClass::Operate,
            "hraness.control/1",
            "Stop the owner",
        ))
        .unwrap();
        r
    }

    #[test]
    fn commands_json_matches_the_golden() {
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_millis(1_790_553_600_000);
        let got = serde_json::to_value(example().commands_json().at(at)).unwrap();
        let want: serde_json::Value =
            serde_json::from_str(include_str!("../../../contract/golden/commands.json")).unwrap();
        assert_eq!(got, want);
    }

    #[test]
    fn decide_needs_a_gate_and_read_refuses_one() {
        let mut r = Registry::new("example");
        assert_eq!(
            r.register(Verb::new(&["allow"], OpClass::Decide, "example.a/1", "x"))
                .unwrap_err(),
            RegistryError::DecideWithoutGate("allow".into())
        );
        assert_eq!(
            r.register(Verb::new(&["list"], OpClass::Read, "example.a/1", "x").gated(GateTier::T3))
                .unwrap_err(),
            RegistryError::GateOnUngatedClass("list".into())
        );
        r.register(Verb::new(
            &["old"],
            OpClass::DecideLegacy,
            "example.a/1",
            "x",
        ))
        .unwrap();
        assert!(matches!(
            r.register(Verb::new(&["old"], OpClass::Read, "example.a/1", "x")),
            Err(RegistryError::Duplicate(_))
        ));
        assert!(matches!(
            r.register(Verb::new(&["Bad"], OpClass::Read, "example.a/1", "x")),
            Err(RegistryError::InvalidSegment(_))
        ));
        assert!(matches!(
            r.register(Verb::new(&["ok"], OpClass::Read, "example.a", "x")),
            Err(RegistryError::InvalidSchema(_))
        ));
    }

    #[test]
    fn names_match_the_contract() {
        let c: serde_json::Value = serde_json::from_str(crate::contract::NAMES).unwrap();
        let check = |kind: &str, f: &dyn Fn(&str) -> bool| {
            for (list, want) in [("valid", true), ("invalid", false)] {
                for v in c["cases"][kind][list].as_array().unwrap() {
                    let v = v.as_str().unwrap();
                    assert_eq!(f(v), want, "{kind} {v:?}");
                }
            }
        };
        check("productName", &crate::envelope::valid_product_name);
        check("verbSegment", &valid_segment);
        check("schemaId", &valid_schema);
        check("productCode", &|code: &str| match code.split_once('.') {
            Some((product, _)) => {
                matches!(
                    ErrorCode::parse(code, Some(product)),
                    Some(ErrorCode::Product(_))
                )
            }
            None => false,
        });
    }

    #[test]
    fn operate_when_lets_deny_skip_the_gate() {
        let mut r = Registry::new("example");
        r.register(
            Verb::new(
                &["approvals", "decide"],
                OpClass::Decide,
                "example.2fa/1",
                "x",
            )
            .gated(GateTier::T1T2)
            .operate_when("deny"),
        )
        .unwrap();
        let verb = r.lookup(&["approvals", "decide"]).unwrap();
        assert!(!r.needs_gate(verb, true));
        assert!(r.needs_gate(verb, false));
        let listed = serde_json::to_value(r.commands_json()).unwrap();
        assert_eq!(listed["data"]["verbs"][0]["operateWhen"], "deny");
        assert_eq!(listed["data"]["verbs"][0]["opClass"], "decide");
        assert_eq!(
            r.register(
                Verb::new(&["legacy"], OpClass::DecideLegacy, "example.a/1", "x")
                    .operate_when("deny")
            )
            .unwrap_err(),
            RegistryError::OperateWhenWithoutGate("legacy".into())
        );
        let plain = Verb::new(&["x"], OpClass::Operate, "example.a/1", "x");
        assert!(!r.needs_gate(&plain, false));
    }

    #[test]
    fn lookup_prefers_the_longest_path() {
        let r = example();
        assert_eq!(
            r.lookup(&["approvals", "decide", "--json"]).unwrap().schema,
            "example.approval/1"
        );
        assert!(r.lookup(&["approvals"]).is_none());
        assert!(r.lookup::<&str>(&[]).is_none());
    }

    #[test]
    fn human_required_points_at_a_person() {
        let r = example();
        let verb = r.lookup(&["approvals", "decide"]).unwrap();
        let body = r.human_required(verb, "example approvals decide 7");
        assert_eq!(body.code.exit_code(), crate::envelope::EXIT_HUMAN_REQUIRED);
        assert_eq!(body.next[0].audience, Audience::Human);
    }

    #[cfg(feature = "clap")]
    #[test]
    fn clap_tree_has_every_verb() {
        let cmd = example().clap_command();
        let m = cmd
            .clone()
            .try_get_matches_from(["example", "approvals", "decide", "--json"])
            .unwrap();
        let (name, sub) = m.subcommand().unwrap();
        assert_eq!(name, "approvals");
        assert!(sub.subcommand_matches("decide").unwrap().get_flag("json"));
        assert!(cmd
            .clone()
            .try_get_matches_from(["example", "approvals"])
            .is_err());
        assert!(cmd
            .try_get_matches_from(["example", "commands", "--json"])
            .is_ok());
    }

    #[test]
    fn op_classes_match_the_contract() {
        let c: serde_json::Value = serde_json::from_str(crate::contract::OP_CLASSES).unwrap();
        let names: Vec<_> = c["classes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap().to_string())
            .collect();
        let ours: Vec<_> = [
            OpClass::Read,
            OpClass::Operate,
            OpClass::Decide,
            OpClass::DecideLegacy,
        ]
        .iter()
        .map(|c| c.as_str().to_string())
        .collect();
        assert_eq!(names, ours);
    }
}

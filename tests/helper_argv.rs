//! Runs `contract/helper-argv.v0.8.1.json` against both `hraness-helper`
//! and `hraness-companion`: the one-shot modes keep the v0.8.1 argv,
//! stdout and exit contract in both binaries. From 1.0 the companion is an
//! alias: `contract/companion-alias.v1.json` pins byte parity with the
//! helper and the `tray-removed` refusal of the old menu-bar argv.

use std::io::Write;
use std::process::{Command, Stdio};

const CONTRACT: &str = include_str!("../contract/helper-argv.v0.8.1.json");
const ALIAS: &str = include_str!("../contract/companion-alias.v1.json");

fn run(binary: &str, name: &str, case: &serde_json::Value) {
    let label = case["name"].as_str().unwrap();
    if let Some(platforms) = case["platforms"].as_array() {
        let here = if cfg!(unix) { "unix" } else { "windows" };
        if !platforms.iter().any(|p| p == here) {
            return;
        }
    }
    let home = std::env::temp_dir().join(format!(
        "hraness-helper-argv-{}-{}-{}",
        std::process::id(),
        name,
        label.replace(' ', "-")
    ));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let argv: Vec<&str> = case["argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    let mut child = Command::new(binary)
        .args(&argv)
        .env("HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(case["stdin"].as_str().unwrap().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&home);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        output.status.code(),
        Some(case["exit"].as_i64().unwrap() as i32),
        "{name} {label}: {stdout}"
    );
    assert!(stdout.ends_with('\n'), "{name} {label}: one line");
    let line = stdout.trim_end_matches('\n');
    assert!(!line.contains('\n'), "{name} {label}: one line");
    if let Some(exact) = case["stdout"].as_str() {
        assert_eq!(line, exact, "{name} {label}");
    } else {
        let pattern = case["stdoutPattern"]
            .as_str()
            .unwrap()
            .replace("{binary}", name);
        assert!(
            regex::Regex::new(&pattern).unwrap().is_match(line),
            "{name} {label}: {line}"
        );
    }
}

fn contract() -> serde_json::Value {
    serde_json::from_str(CONTRACT).unwrap()
}

#[test]
fn helper_keeps_the_v0_8_1_contract() {
    for case in contract()["cases"].as_array().unwrap() {
        run(env!("CARGO_BIN_EXE_hraness-helper"), "hraness-helper", case);
    }
}

/// The v0.8.1 cases the alias contract replaces for the companion: exactly
/// the ones whose argv is a removed tray mode.
fn superseded(case: &serde_json::Value) -> bool {
    let argv = strings(&case["argv"]);
    let tray = alias()["trayModes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|mode| strings(mode) == argv);
    let named = alias()["supersedesCompanionCases"]
        .as_array()
        .unwrap()
        .iter()
        .any(|name| name == &case["name"]);
    assert_eq!(
        tray, named,
        "{}: trayModes and supersedesCompanionCases disagree",
        case["name"]
    );
    tray
}

#[test]
fn companion_keeps_the_v0_8_1_contract() {
    let mut skipped = 0;
    for case in contract()["cases"].as_array().unwrap() {
        if superseded(case) {
            skipped += 1;
            continue;
        }
        run(
            env!("CARGO_BIN_EXE_hraness-companion"),
            "hraness-companion",
            case,
        );
    }
    assert_eq!(
        skipped,
        alias()["supersedesCompanionCases"]
            .as_array()
            .unwrap()
            .len()
    );
}

fn alias() -> serde_json::Value {
    serde_json::from_str(ALIAS).unwrap()
}

fn strings(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect()
}

struct Run {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Runs `binary` with a fresh HOME, `stdin` written then closed.
fn capture(binary: &str, argv: &[String], stdin: &str, tag: &str) -> Run {
    let home = std::env::temp_dir().join(format!("hraness-alias-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let mut child = Command::new(binary)
        .args(argv)
        .env("HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A refusing binary may exit before reading; a closed pipe is fine.
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    let output = child.wait_with_output().unwrap();
    let _ = std::fs::remove_dir_all(&home);
    Run {
        code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

const HELPER: &str = env!("CARGO_BIN_EXE_hraness-helper");
const COMPANION: &str = env!("CARGO_BIN_EXE_hraness-companion");

fn assert_parity(argv: &[String], stdin: &str, tag: &str) {
    let helper = capture(HELPER, argv, stdin, &format!("{tag}-h"));
    let companion = capture(COMPANION, argv, stdin, &format!("{tag}-c"));
    assert_eq!(helper.code, companion.code, "{argv:?}: exit status");
    let is_version = argv.len() == 1 && argv[0] == "--version";
    let expected = if is_version {
        String::from_utf8(helper.stdout.clone())
            .unwrap()
            .replacen("hraness-helper ", "hraness-companion ", 1)
            .into_bytes()
    } else {
        helper.stdout.clone()
    };
    assert_eq!(expected, companion.stdout, "{argv:?}: stdout bytes");
    assert_eq!(helper.stderr, companion.stderr, "{argv:?}: stderr bytes");
}

#[test]
fn companion_alias_is_byte_identical_to_the_helper() {
    let mut n = 0;
    for case in contract()["cases"].as_array().unwrap() {
        if let Some(platforms) = case["platforms"].as_array() {
            let here = if cfg!(unix) { "unix" } else { "windows" };
            if !platforms.iter().any(|p| p == here) {
                continue;
            }
        }
        if superseded(case) {
            continue;
        }
        // `--prompt` and `--notice` with a valid frame would show a dialog;
        // the frozen cases only use frames the modes reject.
        assert_parity(
            &strings(&case["argv"]),
            case["stdin"].as_str().unwrap(),
            &format!("c{n}"),
        );
        n += 1;
    }
    for argv in alias()["parityExtraArgv"].as_array().unwrap() {
        assert_parity(&strings(argv), "", &format!("x{n}"));
        n += 1;
    }
}

#[test]
fn companion_version_is_the_package_version() {
    let run = capture(COMPANION, &["--version".to_string()], "", "v");
    assert_eq!(run.code, Some(0));
    assert_eq!(
        String::from_utf8(run.stdout).unwrap(),
        format!(
            "hraness-companion {} protocol/1,2\n",
            env!("CARGO_PKG_VERSION")
        )
    );
}

#[test]
fn companion_refuses_the_removed_tray_modes() {
    let alias = alias();
    let exit = alias["refusalExit"].as_i64().unwrap() as i32;
    let stdout = format!("{}\n", alias["refusalStdout"].as_str().unwrap());
    let prefix = alias["refusalStderrPrefix"].as_str().unwrap();
    assert_eq!(exit, 2);
    for (n, argv) in alias["trayModes"].as_array().unwrap().iter().enumerate() {
        let argv = strings(argv);
        // A snapshot on stdin must not start a tray or be echoed back.
        let snapshot = "{\"version\":1,\"type\":\"snapshot\",\"appId\":\"org.example.a\",\"name\":\"A\",\"title\":\"A\",\"revision\":1,\"items\":[]}\n";
        let run = capture(COMPANION, &argv, snapshot, &format!("t{n}"));
        assert_eq!(run.code, Some(exit), "{argv:?}");
        assert_eq!(String::from_utf8(run.stdout).unwrap(), stdout, "{argv:?}");
        let stderr = String::from_utf8(run.stderr).unwrap();
        assert!(stderr.starts_with(prefix), "{argv:?}: {stderr}");
        assert!(
            stderr.contains("docs/migration-1.0.md"),
            "{argv:?}: {stderr}"
        );
        assert_eq!(stderr.lines().count(), 1, "{argv:?}: {stderr}");
    }
}

#[test]
fn helper_answers_the_removed_tray_modes_with_invalid_arguments() {
    let alias = alias();
    let expected = format!(
        "{}\n",
        alias["helperAnswersTrayModesWith"].as_str().unwrap()
    );
    // The frozen v0.8.1 rule still holds for the helper.
    assert_eq!(
        contract()["helperAnswersCompanionOnlyWith"]
            .as_str()
            .unwrap(),
        alias["helperAnswersTrayModesWith"].as_str().unwrap()
    );
    for (n, argv) in alias["trayModes"].as_array().unwrap().iter().enumerate() {
        let argv = strings(argv);
        let run = capture(HELPER, &argv, "", &format!("h{n}"));
        assert_eq!(run.code, Some(1), "{argv:?}");
        assert_eq!(String::from_utf8(run.stdout).unwrap(), expected, "{argv:?}");
        assert!(run.stderr.is_empty(), "{argv:?}");
    }
}

//! Runs `contract/helper-argv.v0.8.1.json` against both `hraness-helper`
//! and `hraness-companion`: the one-shot modes keep the v0.8.1 argv,
//! stdout and exit contract in both binaries.

use std::io::Write;
use std::process::{Command, Stdio};

const CONTRACT: &str = include_str!("../contract/helper-argv.v0.8.1.json");

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

#[test]
fn companion_keeps_the_v0_8_1_contract() {
    for case in contract()["cases"].as_array().unwrap() {
        run(
            env!("CARGO_BIN_EXE_hraness-companion"),
            "hraness-companion",
            case,
        );
    }
}

#[test]
fn helper_refuses_companion_only_modes() {
    let expected = contract()["helperAnswersCompanionOnlyWith"]
        .as_str()
        .unwrap()
        .to_string();
    let dir = std::env::temp_dir();
    for argv in [
        vec!["--check-protocol".to_string()],
        vec!["--state-dir".to_string(), dir.display().to_string()],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_hraness-helper"))
            .args(&argv)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{argv:?}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{expected}\n")
        );
    }
}

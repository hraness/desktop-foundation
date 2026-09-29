//! Ties the Rust kit to the contract: the same golden files the TypeScript
//! kit checks (`sdk/test/golden/permissions`), and the kind table, preset copy
//! and JSON sample in `docs/permissions.md`. A change to the copy updates the
//! doc, the TypeScript kit and this crate in the same pull request.

use hraness_cli_kit::audience::Audience;
use hraness_cli_kit::permissions::{self, presets, *};
use hraness_cli_kit::Style;
use std::path::PathBuf;
use std::time::Duration;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> String {
    std::fs::read_to_string(repo().join(path))
        .unwrap_or_else(|error| panic!("read {path}: {error}"))
        .replace("\r\n", "\n")
}

fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_owned())
    }
}

const UTF8: &[(&str, &str)] = &[("LANG", "en_US.UTF-8")];

fn cli(need: &PermissionNeed) -> String {
    let env = env_of(UTF8);
    format_notice(
        &render_pre_prompt(need, Surface::Cli, &env),
        NoticeKind::PrePrompt,
        true,
        Style::PLAIN,
    )
}

fn product(name: &str, command: &str) -> ProductRef {
    ProductRef::new(name, command).with_requester(name)
}

type Preset = fn(ProductRef) -> PermissionNeed;

/// The same presets and parameters as `sdk/test/permissions.test.ts`.
fn golden_presets() -> Vec<(&'static str, Preset)> {
    vec![
        ("login-item", presets::login_item),
        ("chrome-safe-storage", |r| {
            presets::chrome_safe_storage(r, None, "security", None)
        }),
        ("messages-fda", |r| presets::messages_fda(r, None)),
        ("automation", |r| {
            let why = format!("{} only sends replies in chats you turn on.", r.product);
            presets::automation(r, "Messages", &why)
        }),
        ("contacts", |r| presets::contacts(r, None)),
        ("local-network", |r| {
            presets::local_network(r, "Room members on your network connect to this Mac.")
        }),
        ("incoming-connections", |r| {
            presets::incoming_connections(
                r,
                "vhalla",
                "Choose Allow so room members can reach this Mac.",
            )
        }),
        ("xcode-tools", |r| {
            let effect = format!("{} runs without its native helper", r.product);
            presets::xcode_tools(r, &effect)
        }),
        ("screen-recording", |r| {
            let why = format!("{} only captures the window you pick.", r.product);
            presets::screen_recording(r, &why)
        }),
        ("local-signing", presets::local_signing),
    ]
}

fn golden(name: &str, make: Preset) -> String {
    let env_pairs: &[(&str, &str)] = &[("LANG", "en_US.UTF-8"), ("TERM_PROGRAM", "Apple_Terminal")];
    let env = env_of(env_pairs);
    let mut out: Vec<String> = Vec::new();
    for (variant, reference) in [
        (
            "local app",
            ProductRef::new("Example", "example").with_requester("Example"),
        ),
        ("terminal", ProductRef::new("Example", "example")),
    ] {
        let need = make(reference);
        out.push(format!("# {name} · requester: {variant}"));
        let pre = render_pre_prompt(&need, Surface::Cli, &env);
        out.push("## cli pre-prompt, interactive".into());
        out.push(
            format_notice(&pre, NoticeKind::PrePrompt, true, Style::PLAIN)
                .trim_end()
                .to_owned(),
        );
        out.push("## cli pre-prompt, ascii".into());
        out.push(
            format_notice(&pre, NoticeKind::PrePrompt, false, Style::ASCII)
                .trim_end()
                .to_owned(),
        );
        out.push("## dialog".into());
        out.push(
            notice_request(&need, &env)
                .map_or_else(|| "null".to_owned(), |request| request.to_json()),
        );
        for state in [
            RecoveryState::Denied,
            RecoveryState::Unknown,
            RecoveryState::Missing,
        ] {
            let label = state.as_str();
            out.push(format!("## cli recovery {label}"));
            out.push(
                format_notice(
                    &render_recovery(&need, state, Surface::Cli, &env),
                    NoticeKind::Recovery,
                    true,
                    Style::PLAIN,
                )
                .trim_end()
                .to_owned(),
            );
            out.push(format!("## dialog recovery {label}"));
            out.push(render_recovery(&need, state, Surface::Dialog, &env).to_json());
            out.push(format!("## json {label}"));
            out.push(permission_error_json(&need, state, &env));
        }
        out.push(String::new());
    }
    out.join("\n")
}

#[test]
fn golden_copy_matches_the_typescript_kit() {
    let presets = golden_presets();
    let dir = repo().join("sdk/test/golden/permissions");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("golden directory")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    let mut names: Vec<String> = presets
        .iter()
        .map(|(name, _)| format!("{name}.txt"))
        .collect();
    names.sort();
    assert_eq!(
        files, names,
        "every golden file has a Rust preset and the other way round"
    );
    for (name, make) in presets {
        let expected = read(&format!("sdk/test/golden/permissions/{name}.txt"));
        let actual = golden(name, make);
        if actual != expected {
            for (index, (a, e)) in actual.lines().zip(expected.lines()).enumerate() {
                assert_eq!(a, e, "{name}.txt line {}", index + 1);
            }
            assert_eq!(actual, expected, "{name}.txt");
        }
    }
}

#[test]
fn the_documented_kind_table_matches_the_kit() {
    let doc = read("docs/permissions.md");
    let section = doc
        .split("\n## Permission kinds\n")
        .nth(1)
        .unwrap()
        .split("\n## ")
        .next()
        .unwrap();
    let rows: Vec<&str> = section
        .lines()
        .filter(|row| row.starts_with("| `"))
        .collect();
    assert_eq!(rows.len(), PermissionKind::ALL.len());
    for (row, kind) in rows.iter().zip(PermissionKind::ALL) {
        let cells: Vec<String> = row
            .trim_matches('|')
            .split(" | ")
            .map(|cell| cell.trim().replace('`', ""))
            .collect();
        assert_eq!(cells[0], kind.as_str(), "documentation order");
        assert_eq!(PermissionKind::parse(&cells[0]), Some(kind));
        let behavior = match kind.behavior() {
            PromptBehavior::Asks => "asks",
            PromptBehavior::Notifies => "notifies",
            PromptBehavior::SettingsOnly => "settings-only",
        };
        assert!(
            cells[1].starts_with(behavior) || kind == PermissionKind::Gatekeeper,
            "{} behavior",
            kind.as_str()
        );
        assert_eq!(
            kind.pane_name().unwrap_or("none"),
            cells[2],
            "{} pane",
            kind.as_str()
        );
        assert_eq!(
            kind.settings_path().unwrap_or("none"),
            cells[3],
            "{} path",
            kind.as_str()
        );
        assert_eq!(
            kind.settings_url().unwrap_or("none"),
            cells[4],
            "{} url",
            kind.as_str()
        );
        assert_eq!(kind.has_settings_pane(), kind.settings_url().is_some());
    }
    assert!(is_allowed_settings_url(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"
    ));
    assert!(!is_allowed_settings_url("https://example.com"));
    assert!(!is_allowed_settings_url(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Anything"
    ));
}

/// Every ```text block in § Presets is the kit's rendering of that preset.
#[test]
fn documented_preset_copy_is_what_the_kit_prints() {
    let textbutler = product("Textbutler", "textbutler");
    let valhalla = product("Valhalla", "vhalla");
    let rendered = [
        cli(&presets::login_item(textbutler.clone())),
        cli(&presets::login_item(
            ProductRef::new("Textbutler", "textbutler").with_requester("bun"),
        )),
        cli(&presets::chrome_safe_storage(
            ProductRef::new("Ghostget", "ghostget"),
            None,
            "security",
            None,
        )),
        cli(&presets::messages_fda(textbutler.clone(), None)),
        cli(&presets::automation(
            textbutler.clone(),
            "Messages",
            "Textbutler only sends replies in chats you turn on.",
        )),
        cli(&presets::contacts(
            product("PeopleBlade", "peopleblade"),
            None,
        )),
        cli(&presets::local_network(
            valhalla.clone(),
            "Room members on your network connect to this Mac.",
        )),
        cli(&presets::incoming_connections(
            ProductRef::new("Valhalla", "vhalla"),
            "vhalla",
            "Choose Allow so room members can reach this Mac.",
        )),
        cli(&presets::xcode_tools(
            ProductRef::new("algal", "algal"),
            "{skipEffect}",
        )),
        cli(&presets::screen_recording(
            product("Slopcamera", "slopcamera"),
            "Slopcamera only captures the window you pick.",
        )),
        cli(&presets::local_signing(textbutler.clone())),
    ];
    let doc = read("docs/permissions.md");
    let section = doc
        .split("\n## Presets\n")
        .nth(1)
        .unwrap()
        .split("\n## JSON error shape")
        .next()
        .unwrap();
    let blocks: Vec<String> = section
        .split("```text\n")
        .skip(1)
        .map(|block| format!("{}\n", block.split("\n```").next().unwrap()))
        .collect();
    assert_eq!(
        blocks.len(),
        rendered.len(),
        "one rendering per documented block"
    );
    for (block, text) in blocks.iter().zip(&rendered) {
        assert_eq!(block, text);
    }
}

#[test]
fn the_documented_json_error_is_what_the_kit_prints() {
    let doc = read("docs/permissions.md");
    let sample = doc
        .split("\n## JSON error shape\n")
        .nth(1)
        .unwrap()
        .split("```json\n")
        .nth(1)
        .unwrap()
        .split("\n```")
        .next()
        .unwrap();
    let env = env_of(UTF8);
    let need = presets::messages_fda(product("Textbutler", "textbutler"), None);
    assert_eq!(
        permission_error_json(&need, RecoveryState::Denied, &env),
        sample
    );
}

#[test]
fn recovery_copy_follows_the_templates() {
    let env = env_of(UTF8);
    let recovery = |need: &PermissionNeed, state, interactive, style| {
        format_notice(
            &render_recovery(need, state, Surface::Cli, &env),
            NoticeKind::Recovery,
            interactive,
            style,
        )
    };
    let fda = presets::messages_fda(product("Textbutler", "textbutler"), None);
    assert_eq!(
        recovery(&fda, RecoveryState::Denied, true, Style::PLAIN),
        "✗ Textbutler can't read your Messages: macOS access is off for Textbutler.\n  Turn on Textbutler in System Settings › Privacy & Security › Full Disk Access.\n→ textbutler doctor · press o to open Settings\n"
    );
    assert_eq!(
        recovery(&fda, RecoveryState::Denied, false, Style::ASCII),
        "FAIL Textbutler can't read your Messages: macOS access is off for Textbutler.\n  Turn on Textbutler in System Settings › Privacy & Security › Full Disk Access.\n-> textbutler doctor\n"
    );
    let tools = presets::xcode_tools(ProductRef::new("algal", "algal"), "x");
    assert_eq!(
        recovery(&tools, RecoveryState::Missing, true, Style::PLAIN),
        "✗ algal needs Apple's command line tools. Nothing was installed.\n→ xcode-select --install\n"
    );
}

#[test]
fn responsible_app_names_the_terminal_then_the_product() {
    let name =
        |pairs: &[(&str, &str)], product: Option<&str>| responsible_app(&env_of(pairs), product);
    assert_eq!(
        name(&[("TERM_PROGRAM", "Apple_Terminal")], None),
        "Terminal"
    );
    assert_eq!(
        name(
            &[
                ("__CFBundleIdentifier", "com.googlecode.iterm2"),
                ("TERM_PROGRAM", "vscode")
            ],
            None
        ),
        "iTerm"
    );
    assert_eq!(name(&[("TERM_PROGRAM", "ghostty")], None), "Ghostty");
    assert_eq!(
        name(&[("__CFBundleIdentifier", "com.microsoft.VSCode")], None),
        "Visual Studio Code"
    );
    assert_eq!(name(&[("ZED_TERM", "true")], None), "Zed");
    assert_eq!(name(&[("TERM_PROGRAM", "WarpTerminal")], None), "Warp");
    assert_eq!(name(&[("TERM_PROGRAM", "WezTerm")], None), "WezTerm");
    assert_eq!(name(&[], None), "your terminal app");
    let app: &[(&str, &str)] = &[
        ("HRANESS_APP_BUNDLE_ID", "app.hraness.textbutler"),
        ("TERM_PROGRAM", "Apple_Terminal"),
    ];
    assert_eq!(name(app, Some("Textbutler")), "Textbutler");
    assert_eq!(
        name(app, None),
        "Terminal",
        "the bundle ID alone cannot name the app"
    );
    assert_eq!(
        name(app, Some("")),
        "Terminal",
        "an empty product name is unset"
    );
    // An empty requester falls back to the kind's default, like TS.
    let env = env_of(&[]);
    let reference = ProductRef::new("Thing", "thing").with_requester("");
    let keychain = PermissionNeed::new(reference, PermissionKind::Keychain, "x", "y.");
    assert_eq!(requester_of(&keychain, &env), "security");
}

#[test]
fn keychain_statuses_classify() {
    assert_eq!(classify_keychain_status(-128), Some(RecoveryState::Denied));
    assert_eq!(
        classify_keychain_status(-25293),
        Some(RecoveryState::Denied)
    );
    assert_eq!(classify_keychain_status(51), Some(RecoveryState::Denied));
    assert_eq!(
        classify_keychain_status(-25308),
        Some(RecoveryState::Unknown)
    );
    assert_eq!(classify_keychain_status(36), Some(RecoveryState::Unknown));
    assert_eq!(
        classify_keychain_status(-25300),
        Some(RecoveryState::Missing)
    );
    assert_eq!(classify_keychain_status(44), Some(RecoveryState::Missing));
    assert_eq!(classify_keychain_status(0), None);
    assert_eq!(classify_keychain_status(1), None);
}

/// A fake process for `pre_prompt`, the probes and Settings links.
struct Fake {
    env: Vec<(String, String)>,
    stdin_tty: bool,
    stderr_tty: bool,
    keys: Vec<Key>,
    written: String,
    opened: Vec<String>,
    files: Vec<(String, FileAccess)>,
    probed: std::cell::RefCell<Vec<String>>,
    ran: std::cell::RefCell<Vec<Vec<String>>>,
    status: Option<i32>,
}

impl Fake {
    fn new() -> Self {
        Self {
            env: vec![
                ("LANG".into(), "en_US.UTF-8".into()),
                ("HOME".into(), "/Users/test".into()),
            ],
            stdin_tty: true,
            stderr_tty: true,
            keys: vec![],
            written: String::new(),
            opened: vec![],
            files: vec![],
            probed: Default::default(),
            ran: Default::default(),
            status: Some(0),
        }
    }
    fn keys(mut self, keys: &[Key]) -> Self {
        self.keys = keys.to_vec();
        self
    }
    fn set(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

impl PermissionIo for Fake {
    fn env(&self, key: &str) -> Option<String> {
        self.env
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    }
    fn stdin_is_tty(&self) -> bool {
        self.stdin_tty
    }
    fn stderr_is_tty(&self) -> bool {
        self.stderr_tty
    }
    fn write(&mut self, text: &str) -> std::io::Result<()> {
        self.written.push_str(text);
        Ok(())
    }
    fn read_key(&mut self, _timeout: Duration) -> std::io::Result<Key> {
        Ok(if self.keys.is_empty() {
            Key::Timeout
        } else {
            self.keys.remove(0)
        })
    }
    fn open_url(&mut self, url: &str) -> std::io::Result<bool> {
        self.opened.push(url.to_owned());
        Ok(true)
    }
    fn file_access(&self, path: &std::path::Path) -> FileAccess {
        let path = path.to_string_lossy().into_owned();
        self.probed.borrow_mut().push(path.clone());
        self.files
            .iter()
            .find(|(suffix, _)| path.ends_with(suffix.as_str()))
            .map_or(FileAccess::Missing, |(_, access)| *access)
    }
    fn run_status(&self, argv: &[&str]) -> Option<i32> {
        self.ran
            .borrow_mut()
            .push(argv.iter().map(|arg| (*arg).to_owned()).collect());
        self.status
    }
}

#[test]
fn pre_prompt_shows_notices_by_audience_and_never_waits_without_two_terminals() {
    let textbutler = product("Textbutler", "textbutler");
    let fda = presets::messages_fda(textbutler.clone(), None);
    let automation = presets::automation(textbutler.clone(), "Messages", "x.");

    let mut io = Fake::new().keys(&[Key::Enter]);
    assert_eq!(
        pre_prompt(&fda, Some(Audience::Human), &mut io).unwrap(),
        PrePromptOutcome::Continue
    );
    assert!(io.written.contains("Press Enter to open Settings"));
    assert_eq!(
        io.opened,
        ["x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"]
    );

    let mut io = Fake::new().keys(&[Key::Skip]);
    assert_eq!(
        pre_prompt(&fda, Some(Audience::Human), &mut io).unwrap(),
        PrePromptOutcome::Skip
    );
    assert!(io.opened.is_empty());

    let mut io = Fake::new();
    assert_eq!(
        pre_prompt(&automation, Some(Audience::Human), &mut io).unwrap(),
        PrePromptOutcome::Skip,
        "timeout skips"
    );

    let mut io = Fake::new().keys(&[Key::Enter]);
    assert_eq!(
        pre_prompt(&automation, Some(Audience::Human), &mut io).unwrap(),
        PrePromptOutcome::Continue
    );
    assert!(
        io.opened.is_empty(),
        "asks: Enter continues without opening Settings"
    );

    let mut io = Fake::new().keys(&[Key::Enter]);
    io.stdin_tty = false;
    assert_eq!(
        pre_prompt(&fda, Some(Audience::Human), &mut io).unwrap(),
        PrePromptOutcome::UnattendedStop
    );
    assert_eq!(io.keys.len(), 1, "never read a key without two terminals");
    assert!(!io.written.contains("Press Enter"));

    let mut io = Fake::new();
    assert_eq!(
        pre_prompt(
            &presets::login_item(textbutler.clone()),
            Some(Audience::Human),
            &mut io
        )
        .unwrap(),
        PrePromptOutcome::Continue
    );
    assert!(!io.written.contains("Press Enter"));

    let mut io = Fake::new();
    assert_eq!(
        pre_prompt(&fda, Some(Audience::Quiet), &mut io).unwrap(),
        PrePromptOutcome::UnattendedStop
    );
    assert_eq!(
        pre_prompt(
            &presets::login_item(textbutler.clone()),
            Some(Audience::Quiet),
            &mut io
        )
        .unwrap(),
        PrePromptOutcome::UnattendedProceed
    );
    assert!(io.written.is_empty());

    let mut io = Fake::new();
    let proceed = fda.clone().with_unattended(Unattended::Proceed);
    assert_eq!(
        pre_prompt(&proceed, Some(Audience::Agent), &mut io).unwrap(),
        PrePromptOutcome::UnattendedProceed
    );
    assert!(io.written.starts_with(r#"{"type":"permission-notice","product":"Textbutler","kind":"full-disk-access","message":"Textbutler needs Full Disk Access"#));
    assert!(io.written.ends_with("}\n"));

    // The audience comes from the environment when not given.
    let mut io = Fake::new().set("CLAUDECODE", "1");
    pre_prompt(&fda, None, &mut io).unwrap();
    assert!(io.written.starts_with("{\"type\":\"permission-notice\""));

    let mut io = Fake::new().set("NO_COLOR", "1").set("TERM", "dumb");
    io.stdin_tty = false;
    pre_prompt(&fda, Some(Audience::Human), &mut io).unwrap();
    assert!(io
        .written
        .starts_with("NOTE Textbutler needs Full Disk Access"));
}

#[test]
fn recovery_offers_settings_only_at_a_terminal() {
    let fda = presets::messages_fda(product("Textbutler", "textbutler"), None);
    let mut io = Fake::new().keys(&[Key::Open]);
    report_permission_failure(&fda, RecoveryState::Denied, Some(Audience::Human), &mut io).unwrap();
    assert!(io.written.contains("press o to open Settings"));
    assert_eq!(io.opened.len(), 1);
    let mut agent = Fake::new().keys(&[Key::Open]);
    report_permission_failure(
        &fda,
        RecoveryState::Denied,
        Some(Audience::Agent),
        &mut agent,
    )
    .unwrap();
    assert!(agent.written.is_empty());
    let mut quiet = Fake::new().keys(&[Key::Open]);
    report_permission_failure(
        &fda,
        RecoveryState::Denied,
        Some(Audience::Quiet),
        &mut quiet,
    )
    .unwrap();
    assert!(!quiet.written.contains("press o"));
    assert!(quiet.opened.is_empty());
}

#[test]
fn probes_only_where_no_prompt_can_appear() {
    let mut io = Fake::new();
    io.files = vec![
        ("chat.db".into(), FileAccess::Denied),
        ("/Library/Safari".into(), FileAccess::Ok),
        ("/Library/Mail".into(), FileAccess::Ok),
    ];
    let fda = PermissionKind::FullDiskAccess;
    assert_eq!(
        permission_status(fda, Some("Messages"), &io),
        PermissionState::Denied
    );
    assert_eq!(
        permission_status(fda, Some("Safari"), &io),
        PermissionState::Granted
    );
    assert_eq!(
        permission_status(
            fda,
            Some("/Users/test/Library/Containers/com.apple.Safari/Data/x"),
            &io
        ),
        PermissionState::Unknown
    );
    assert_eq!(
        permission_status(fda, Some("relative/path"), &io),
        PermissionState::Unknown
    );
    for path in [
        "/Users/test/Documents/a",
        "/Users/test/Desktop",
        "/Volumes/USB/x",
        "/Users/test/Library/Mobile Documents/x",
        "/Users/test/Library/Group Containers/x",
        "/Users/test/Library/containers/x",
        "/Users/test/Library/cloudstorage/x",
        "/Users/test/Library/CloudStorage/x",
        "/Users/test/Library/../Documents/x",
        "/Users/test/Library",
    ] {
        assert_eq!(
            permission_status(fda, Some(path), &io),
            PermissionState::Unknown,
            "{path}"
        );
    }
    assert_eq!(
        permission_status(fda, Some("/Users/test/Library/Mail"), &io),
        PermissionState::Granted
    );
    assert_eq!(permission_status(fda, None, &io), PermissionState::Unknown);
    assert_eq!(
        *io.probed.borrow(),
        [
            "/Users/test/Library/Messages/chat.db",
            "/Users/test/Library/Safari",
            "/Users/test/Library/Mail"
        ]
    );
    for kind in [
        PermissionKind::Automation,
        PermissionKind::Contacts,
        PermissionKind::Keychain,
        PermissionKind::Camera,
    ] {
        assert_eq!(permission_status(kind, None, &io), PermissionState::Unknown);
    }
    let mut tools = Fake::new();
    tools.status = Some(2);
    assert_eq!(
        permission_status(PermissionKind::DeveloperTools, None, &tools),
        PermissionState::NotDetermined
    );
    assert_eq!(
        *tools.ran.borrow(),
        [vec!["/usr/bin/xcode-select".to_owned(), "-p".to_owned()]]
    );
    // A probe that cannot run reads not-determined, like the TS kit's 127.
    let mut failed = Fake::new();
    failed.status = None;
    assert_eq!(
        permission_status(PermissionKind::DeveloperTools, None, &failed),
        PermissionState::NotDetermined
    );
    let mut login = Fake::new();
    login.files = vec![("app.hraness.companion.sponge.plist".into(), FileAccess::Ok)];
    assert_eq!(
        permission_status(PermissionKind::LoginItem, Some("sponge"), &login),
        PermissionState::Granted
    );
    assert_eq!(
        permission_status(PermissionKind::LoginItem, Some("textbutler"), &login),
        PermissionState::NotDetermined
    );
    assert_eq!(
        permission_status(PermissionKind::LoginItem, Some("../evil"), &login),
        PermissionState::Unknown
    );
    // A probe that errors says nothing, like a throw in the TS kit.
    let mut unreadable = Fake::new();
    unreadable.files = vec![("app.hraness.sponge.plist".into(), FileAccess::Unreadable)];
    assert_eq!(
        permission_status(PermissionKind::LoginItem, Some("sponge"), &unreadable),
        PermissionState::Unknown
    );
    // Without HOME the real home stands in, like `homedir()` in the TS kit.
    let mut homeless = Fake::new();
    homeless.env.retain(|(key, _)| key != "HOME");
    assert_eq!(
        permission_status(PermissionKind::LoginItem, Some("sponge"), &homeless),
        PermissionState::NotDetermined
    );
}

#[test]
fn settings_open_only_allowlisted_panes() {
    let mut io = Fake::new();
    assert!(!open_settings(PermissionKind::Keychain, &mut io));
    assert!(!open_settings(PermissionKind::DeveloperTools, &mut io));
    assert!(open_settings(PermissionKind::LoginItem, &mut io));
    assert_eq!(
        io.opened,
        ["x-apple.systempreferences:com.apple.LoginItems-Settings.extension"]
    );
}

#[test]
fn permission_errors_are_cli_errors() {
    let env = env_of(UTF8);
    let fda = presets::messages_fda(product("Textbutler", "textbutler"), None);
    let error = permissions::permission_cli_error(&fda, RecoveryState::Unknown, &env);
    assert_eq!(error.code, "permission-unknown");
    assert_eq!(error.exit_code, 1);
    assert_eq!(
        error.render_human(Style::PLAIN),
        "✗ Textbutler couldn't read your Messages. macOS may be blocking Textbutler.\n→ textbutler doctor\n"
    );
    let info = permission_error(&fda, RecoveryState::Denied, &env);
    assert_eq!(
        info.settings_url,
        Some("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
    );
}

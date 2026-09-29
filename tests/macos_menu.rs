//! Runs the macOS menu post-pass against real AppKit objects on the main
//! thread (this test has its own `main`). It builds no status item and
//! shows nothing on screen.

#[cfg(target_os = "macos")]
fn main() {
    use desktop_foundation::{
        native_menu_dump, Alternate, ItemState, MenuItem, MenuNode, Opens, Symbol,
    };
    let nodes = vec![
        MenuNode::header("Textbutler"),
        MenuNode::status(Symbol::StatusRunning, "Running", Some("3 chats on".into())),
        MenuNode::Separator,
        MenuNode::interactive(
            MenuItem::action("open", "Open dashboard")
                .with_symbol(Symbol::ActionOpen)
                .with_shortcut("CmdOrCtrl+O")
                .opens(Opens::Browser)
                .primary(),
        ),
        MenuNode::interactive(
            MenuItem::action("chat", "Mom")
                .with_symbol(Symbol::ItemChat)
                .with_subtitle("Reply waiting for approval")
                .with_badge("2")
                .with_tooltip("Last message 2 minutes ago")
                .with_alternate(
                    Alternate::new("chat.copy", "Copy chat ID").with_symbol(Symbol::ActionCopy),
                ),
        ),
        MenuNode::interactive(MenuItem::state(
            "pause",
            "Pause automatic replies",
            ItemState::Mixed,
        )),
        MenuNode::Submenu {
            title: "More".into(),
            symbol: Some(Symbol::ActionSettings),
            items: vec![MenuNode::interactive(MenuItem::action(
                "plain",
                "Plain row",
            ))],
        },
        MenuNode::quit("Quit Textbutler"),
    ];
    let dump = native_menu_dump(&nodes).expect("the test binary runs on the main thread");
    print!("{dump}");
    let version = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_default();
    let major: u32 = version
        .split('.')
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let lines: Vec<&str> = dump.lines().collect();
    if major >= 15 {
        // macOS 15 and later have every native API the pass uses.
        let expected = [
            "[header] Textbutler",
            "[image] Running | subtitle: 3 chats on",
            "---",
            "[image] Open dashboard ↗ | key: ⌘o",
            "[image] Mom | subtitle: Reply waiting for approval | badge: 2 | tooltip: Last message 2 minutes ago",
            "[⌥] [image] Copy chat ID | key: ⌥⌘",
            "[mixed] Pause automatic replies",
            "[image] More",
            "  Plain row",
            "Quit Textbutler",
        ];
        assert_eq!(lines, expected);
    } else {
        // Older systems keep the text forms; only check the pairing.
        assert!(lines.iter().any(|line| line.starts_with("[⌥]")));
    }
    println!("macos_menu: ok (macOS {version})");
}

#[cfg(not(target_os = "macos"))]
fn main() {}

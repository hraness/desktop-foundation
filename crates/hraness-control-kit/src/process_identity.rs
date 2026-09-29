//! Identifies a process across PID reuse and reboots without signalling
//! it: `bootId` changes on every boot and `processStartId` changes when a
//! PID is reused. Both are SHA-256 digests of what the OS reports.

use std::process::Command;

use crate::crypto::sha256_hex;

/// The digest of this boot.
pub fn boot_id() -> Option<String> {
    raw_boot_id().map(|raw| sha256_hex(format!("boot:{}", raw.trim()).as_bytes()))
}

/// The digest of the start of process `pid` in this boot, or `None` when
/// no such process exists.
pub fn process_start_id(pid: u32) -> Option<String> {
    raw_start(pid).map(|raw| sha256_hex(format!("start:{pid}:{}", raw.trim()).as_bytes()))
}

#[cfg(target_os = "linux")]
fn raw_boot_id() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()
}

#[cfg(target_os = "linux")]
fn raw_start(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The command name is in parentheses and may hold spaces; fields
    // resume after the last ')'. starttime is field 22, the 20th after it.
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(19).map(str::to_string)
}

#[cfg(not(target_os = "linux"))]
fn raw_boot_id() -> Option<String> {
    run("sysctl", &["-n", "kern.boottime"]).map(|raw| boot_time_key(&raw))
}

/// The part of `sysctl -n kern.boottime` that names the boot:
/// `sec=…,usec=…`. The date text after it is local time and changes with
/// TZ, so it is dropped. Same key as the TypeScript `bootTimeKey`.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn boot_time_key(raw: &str) -> String {
    let number_after = |label: &str| -> Option<&str> {
        let at = raw.find(label)? + label.len();
        let rest = raw[at..].trim_start().strip_prefix('=')?.trim_start();
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        (end > 0).then(|| &rest[..end])
    };
    // The first "sec" is the seconds field; "usec" comes after it.
    match (number_after("sec"), number_after("usec")) {
        (Some(sec), Some(usec)) => format!("sec={sec},usec={usec}"),
        _ => raw.trim().to_string(),
    }
}

#[cfg(not(target_os = "linux"))]
fn raw_start(pid: u32) -> Option<String> {
    run("ps", &["-o", "lstart=", "-p", &pid.to_string()])
}

#[cfg(not(target_os = "linux"))]
fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        // `ps -o lstart=` prints local time; pin it so an owner started
        // under one TZ does not look dead to a checker under another.
        .env("TZ", "UTC")
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    (out.status.success() && !text.trim().is_empty()).then_some(text)
}

/// Whether `pid` is the process whose start was recorded as `start_id`
/// in boot `boot`. Never signals it.
pub fn matches(pid: u32, boot: &str, start_id: &str) -> bool {
    boot_id().as_deref() == Some(boot) && process_start_id(pid).as_deref() == Some(start_id)
}

/// The process group and the terminal's foreground process group of
/// `pid`, from `ps`. Used where no terminal fd is at hand.
#[allow(dead_code)]
pub fn ps_groups(pid: u32) -> Option<(i64, i64)> {
    let out = Command::new("ps")
        .args(["-o", "pgid=,tpgid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let mut it = text.split_whitespace().map(|n| n.parse::<i64>().ok());
    Some((it.next()??, it.next()??))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_identity_is_stable_and_matches() {
        let pid = std::process::id();
        let boot = boot_id().expect("boot id");
        let start = process_start_id(pid).expect("start id");
        assert_eq!(boot.len(), 64);
        assert_eq!(process_start_id(pid).unwrap(), start);
        assert!(matches(pid, &boot, &start));
        assert!(!matches(pid, &boot, &"0".repeat(64)));
    }

    #[test]
    fn boot_time_key_drops_the_local_time_text() {
        let local = "{ sec = 1790535348, usec = 479171 } Sun Sep 27 14:55:48 2026\n";
        let utc = "{ sec = 1790535348, usec = 479171 } Sun Sep 27 18:55:48 2026\n";
        assert_eq!(boot_time_key(local), "sec=1790535348,usec=479171");
        assert_eq!(boot_time_key(local), boot_time_key(utc));
        assert_ne!(
            boot_time_key(local),
            boot_time_key("{ sec = 1790535349, usec = 479171 } Sun Sep 27 14:55:49 2026")
        );
        assert_eq!(boot_time_key(" other \n"), "other");
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn start_ids_ignore_the_callers_time_zone() {
        let pid = std::process::id().to_string();
        let ps = |tz: &str| {
            let out = Command::new("ps")
                .args(["-o", "lstart=", "-p", &pid])
                .env("LC_ALL", "C")
                .env("TZ", tz)
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap()
        };
        // Kolkata is UTC+5:30 all year, so its local text always differs.
        assert_ne!(ps("UTC"), ps("Asia/Kolkata"));
        assert_eq!(
            raw_start(std::process::id()).as_deref(),
            Some(ps("UTC").as_str())
        );
        let raw = run("sysctl", &["-n", "kern.boottime"]).unwrap();
        assert!(raw_boot_id().unwrap().starts_with("sec="), "{raw}");
    }

    #[test]
    fn a_child_has_a_different_start_and_vanishes() {
        let mut child = Command::new("sleep").arg("5").spawn().unwrap();
        let pid = child.id();
        let start = process_start_id(pid).expect("child start");
        assert_ne!(Some(start), process_start_id(std::process::id()));
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(process_start_id(pid).is_none());
    }
}

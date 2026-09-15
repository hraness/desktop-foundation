//! Shared "outputs directory" menu section.
//!
//! Products point an [`OutputsSection`] at a directory their agents drop files
//! into. The section renders one menu item per file — newest first, bounded,
//! with a thumbnail preview for common image types — and answers `open` /
//! `reveal` dispatches. The directory stays the authority; this module only
//! reads it.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Cursor, Read};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::{MenuItem, MenuNode, RgbaIcon};

const DEFAULT_LIMIT: usize = 20;
const MAX_LABEL: usize = 80;
const MAX_DECODE_BYTES: u64 = 24 * 1024 * 1024;
const THUMB_SIZE: u32 = 32;
const MAX_SCAN: usize = 10_000;
const MAX_PIXELS_PER_AXIS: u32 = 8_192;
const MAX_IMAGE_ALLOCATION: u64 = 64 * 1024 * 1024;

pub const OPEN_PREFIX: &str = "outputs.open.";
pub const REVEAL_PREFIX: &str = "outputs.reveal.";
pub const FOLDER_ID: &str = "outputs.folder";

/// One file observed in the outputs directory.
#[derive(Debug, Clone)]
pub struct OutputEntry {
    /// File name inside the directory — the agent's description.
    pub name: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub icon: Option<RgbaIcon>,
}

/// Renders and answers a directory-listing menu section.
pub struct OutputsSection {
    dir: PathBuf,
    limit: usize,
    thumbs: Mutex<HashMap<String, (Fingerprint, RgbaIcon)>>,
    offered: Mutex<HashMap<String, (PathBuf, Fingerprint)>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Fingerprint {
    size: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl Fingerprint {
    fn of(meta: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            size: meta.len(), modified: meta.modified().ok(),
            #[cfg(unix)]
            identity: (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()),
        }
    }
}

struct ListedFile { name: String, path: PathBuf, fingerprint: Fingerprint }
struct Listing { files: Vec<ListedFile>, partial: bool, available: bool }

impl OutputsSection {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        OutputsSection { dir: dir.into(), limit: DEFAULT_LIMIT, thumbs: Mutex::new(HashMap::new()), offered: Mutex::new(HashMap::new()) }
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.clamp(1, 100);
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Bounded newest-first listing. Directories, hidden files, and symlinks
    /// are skipped.
    pub fn entries(&self) -> Vec<OutputEntry> {
        self.scan().files.into_iter().map(|file| self.entry(file)).collect()
    }

    fn entry(&self, file: ListedFile) -> OutputEntry {
        let icon = self.thumbnail(&file.path, &file.fingerprint);
        OutputEntry { name: file.name, size: file.fingerprint.size, modified: file.fingerprint.modified, icon }
    }

    fn scan(&self) -> Listing {
        let mut listing = Listing { files: Vec::new(), partial: false, available: false };
        // Do not follow a redirected output root or offer links as files.
        if !fs::symlink_metadata(&self.dir).is_ok_and(|m| m.is_dir()) { return listing; }
        let Ok(read) = fs::read_dir(&self.dir) else { return listing };
        listing.available = true;
        for (index, entry) in read.take(MAX_SCAN + 1).enumerate() {
            if index == MAX_SCAN { listing.partial = true; break; }
            let Ok(entry) = entry else { listing.partial = true; continue };
            let Ok(name) = entry.file_name().into_string() else { continue };
            if name.starts_with('.') { continue; }
            let Ok(meta) = fs::symlink_metadata(entry.path()) else { continue };
            if !meta.is_file() { continue; }
            listing.files.push(ListedFile { name, path: entry.path(), fingerprint: Fingerprint::of(&meta) });
        }
        listing.files.sort_by(|a, b| b.fingerprint.modified.cmp(&a.fingerprint.modified).then_with(|| a.name.cmp(&b.name)));
        listing.files.truncate(self.limit);
        listing
    }

    /// Menu nodes for the section: one item per entry plus a folder item.
    /// Empty directories retain the folder action so users can find the destination.
    pub fn nodes(&self) -> Vec<MenuNode> {
        let listing = self.scan();
        let mut offered = HashMap::new();
        let mut nodes = Vec::new();
        let mut reveal = Vec::new();
        if !listing.available {
            if let Ok(mut old) = self.offered.lock() { old.clear(); }
            return vec![MenuNode::disabled("Outputs folder unavailable")];
        }
        if listing.files.is_empty() { nodes.push(MenuNode::disabled("No outputs yet")); }
        if listing.partial { nodes.push(MenuNode::disabled("Showing outputs from a partial folder scan")); }
        for file in listing.files {
            let key = file_key(&file.name, &file.fingerprint);
            offered.insert(key.clone(), (file.path.clone(), file.fingerprint.clone()));
            let entry = self.entry(file);
            let label = label_of(&entry.name);
            let mut item = MenuItem::action(format!("{OPEN_PREFIX}{key}"), label.clone())
                .with_badge(file_detail(&entry.name, entry.size));
            if let Some(icon) = entry.icon { item = item.with_icon(icon); }
            nodes.push(MenuNode::interactive(item));
            reveal.push(MenuNode::item(format!("{REVEAL_PREFIX}{key}"), label));
        }
        if let Ok(mut old) = self.offered.lock() { *old = offered; }
        if !reveal.is_empty() { nodes.push(MenuNode::Submenu { title: reveal_label().into(), items: reveal }); }
        nodes.push(MenuNode::Separator);
        nodes.push(MenuNode::item(FOLDER_ID, "Reveal Outputs Folder"));
        nodes
    }

    /// Handles this section's action ids. Returns true when the id was ours.
    pub fn dispatch(&self, id: &str) -> bool {
        if id == FOLDER_ID {
            if fs::symlink_metadata(&self.dir).is_ok_and(|m| m.is_dir()) { self.open_path(&self.dir, false); }
            return true;
        }
        let (prefix, reveal) = if let Some(key) = id.strip_prefix(REVEAL_PREFIX) {
            (key, true)
        } else if let Some(key) = id.strip_prefix(OPEN_PREFIX) {
            (key, false)
        } else {
            return false;
        };
        if let Some(path) = self.find_by_key(prefix) {
            self.open_path(&path, reveal);
        }
        true
    }

    fn find_by_key(&self, key: &str) -> Option<PathBuf> {
        let (path, expected) = self.offered.lock().ok()?.get(key)?.clone();
        let observed = fs::symlink_metadata(&path).ok()?;
        (observed.is_file() && Fingerprint::of(&observed) == expected).then_some(path)
    }

    fn open_path(&self, path: &Path, reveal: bool) {
        #[cfg(target_os = "macos")]
        {
            let mut command = std::process::Command::new("/usr/bin/open");
            if reveal {
                command.arg("-R");
            }
            command.arg(path).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            if let Ok(mut child) = command.spawn() { std::thread::spawn(move || { let _ = child.wait(); }); }
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let target = if reveal { path.parent().unwrap_or(path) } else { path };
            let mut command = std::process::Command::new("xdg-open");
            command.arg(target).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
            if let Ok(mut child) = command.spawn() { std::thread::spawn(move || { let _ = child.wait(); }); }
        }
        #[cfg(windows)]
        {
            let path = path.to_owned();
            std::thread::spawn(move || {
                if reveal {
                    let mut selection = std::ffi::OsString::from("/select,");
                    selection.push(path.as_os_str());
                    let _ = std::process::Command::new("explorer.exe")
                        .arg(selection)
                        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
                        .status();
                } else {
                    use std::os::windows::ffi::OsStrExt;
                    #[link(name = "shell32")]
                    extern "system" {
                        fn ShellExecuteW(window: *mut std::ffi::c_void, operation: *const u16, file: *const u16,
                            parameters: *const u16, directory: *const u16, show: i32) -> *mut std::ffi::c_void;
                    }
                    let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                    let open: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
                    // An OS file association open, never cmd.exe or a shell
                    // command constructed from an output filename.
                    unsafe { ShellExecuteW(std::ptr::null_mut(), open.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), 1); }
                }
            });
        }
    }

    fn thumbnail(&self, path: &Path, expected: &Fingerprint) -> Option<RgbaIcon> {
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico") {
            return None;
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        { use std::os::unix::fs::OpenOptionsExt; options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK); }
        let file = options.open(path).ok()?;
        let meta = file.metadata().ok()?;
        if !meta.is_file() || Fingerprint::of(&meta) != *expected || meta.len() > MAX_DECODE_BYTES { return None; }
        let name = path.file_name()?.to_string_lossy().into_owned();
        if let Ok(cache) = self.thumbs.lock() {
            if let Some((cached, icon)) = cache.get(&name) {
                if cached == expected {
                    return Some(icon.clone());
                }
            }
        }
        let bytes = read_image(file, expected)?;
        let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_PIXELS_PER_AXIS);
        limits.max_image_height = Some(MAX_PIXELS_PER_AXIS);
        limits.max_alloc = Some(MAX_IMAGE_ALLOCATION);
        reader.limits(limits);
        let image = reader.decode().ok()?;
        let thumb = image.thumbnail(THUMB_SIZE, THUMB_SIZE).to_rgba8();
        let (width, height) = (thumb.width(), thumb.height());
        let icon = RgbaIcon { rgba: thumb.into_raw(), width, height };
        if let Ok(mut cache) = self.thumbs.lock() {
            if cache.len() > self.limit * 2 {
                cache.clear();
            }
            cache.insert(name, (expected.clone(), icon.clone()));
        }
        Some(icon)
    }
}

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") { "Reveal Output in Finder" } else { "Show Output in Folder" }
}

fn read_image(mut file: File, expected: &Fingerprint) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    (&mut file).take(MAX_DECODE_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_DECODE_BYTES || Fingerprint::of(&file.metadata().ok()?) != *expected { return None; }
    Some(bytes)
}

fn file_detail(name: &str, size: u64) -> String {
    let ext = Path::new(name).extension().and_then(|s| s.to_str()).unwrap_or("file");
    let kind: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).take(10).collect::<String>().to_uppercase();
    let size = if size < 1024 { format!("{size} B") } else if size < 1024 * 1024 { format!("{} KB", size / 1024) } else { format!("{} MB", size / (1024 * 1024)) };
    format!("{kind} · {size}")
}

fn label_of(name: &str) -> String {
    let stem = Path::new(name).file_stem().unwrap_or_default().to_string_lossy();
    let safe: String = stem.chars().filter(|c| !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')).collect();
    let mut label: String = safe.chars().take(MAX_LABEL).collect();
    if label.len() < safe.len() {
        label.push('…');
    }
    if label.is_empty() { "Untitled output".into() } else { label }
}

fn file_key(name: &str, fingerprint: &Fingerprint) -> String {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    fingerprint.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "foundation-outputs-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 1×1 red PNG.
    fn tiny_png() -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(1, 1, image::Rgba([200, 30, 30, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn entries_list_newest_first_and_skip_hidden() {
        let dir = fixture();
        fs::write(dir.join("older.txt"), b"a").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(dir.join("newer.txt"), b"b").unwrap();
        fs::write(dir.join(".hidden"), b"h").unwrap();
        fs::create_dir_all(dir.join("sub")).unwrap();
        let section = OutputsSection::new(&dir);
        let names: Vec<String> = section.entries().into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["newer.txt", "older.txt"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nodes_use_stem_and_stable_ids() {
        let dir = fixture();
        fs::write(dir.join("chart of options.png"), tiny_png()).unwrap();
        let section = OutputsSection::new(&dir);
        let nodes = section.nodes();
        assert!(matches!(&nodes[0], MenuNode::Interactive { item } if item.title == "chart of options" && item.icon.is_some() && item.badge.as_ref().is_some_and(|s| s.starts_with("PNG"))));
        assert!(matches!(&nodes[1], MenuNode::Submenu { title, items } if title == reveal_label() && items.len() == 1));
        assert!(matches!(&nodes[2], MenuNode::Separator));
        assert!(matches!(&nodes[3], MenuNode::Item { title, .. } if title == "Reveal Outputs Folder"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_dir_retains_folder_action() {
        let dir = fixture();
        let section = OutputsSection::new(&dir);
        assert_eq!(section.nodes(), vec![MenuNode::disabled("No outputs yet"), MenuNode::Separator, MenuNode::item(FOLDER_ID, "Reveal Outputs Folder")]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dispatch_owns_only_prefixed_ids() {
        let dir = fixture();
        let section = OutputsSection::new(&dir);
        assert!(section.dispatch(&format!("{}abc", OPEN_PREFIX)));
        assert!(!section.dispatch("daemon.start"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn newest_file_is_selected_beyond_the_old_four_times_limit() {
        let dir = fixture();
        for i in 0..120 { fs::write(dir.join(format!("older-{i}.txt")), b"old").unwrap(); }
        let newest = dir.join("newest.txt");
        fs::write(&newest, b"new").unwrap();
        OpenOptions::new().write(true).open(&newest).unwrap().set_modified(SystemTime::now() + std::time::Duration::from_secs(60)).unwrap();
        let entries = OutputsSection::new(&dir).with_limit(1).entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "newest.txt");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replaced_or_removed_output_cannot_be_dispatched_from_an_old_menu() {
        let dir = fixture();
        let path = dir.join("report.txt");
        fs::write(&path, b"old").unwrap();
        let section = OutputsSection::new(&dir);
        let key = file_key("report.txt", &Fingerprint::of(&fs::metadata(&path).unwrap()));
        assert!(section.find_by_key(&key).is_none());
        section.nodes();
        assert_eq!(section.find_by_key(&key), Some(path.clone()));
        fs::write(&path, b"new content").unwrap();
        assert!(section.find_by_key(&key).is_none());
        // A newer worker snapshot may finish before the old menu is replaced.
        // Its new fingerprint must never make an old action target new content.
        section.nodes();
        assert!(section.find_by_key(&key).is_none());
        let replacement_key = file_key("report.txt", &Fingerprint::of(&fs::metadata(&path).unwrap()));
        assert_ne!(key, replacement_key);
        assert_eq!(section.find_by_key(&replacement_key), Some(path.clone()));
        section.nodes();
        assert_eq!(section.find_by_key(&replacement_key), Some(path.clone()));
        fs::remove_file(path).unwrap();
        assert!(section.find_by_key(&replacement_key).is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_listed_or_admitted_after_rendering() {
        use std::os::unix::fs::symlink;
        let dir = fixture();
        fs::write(dir.join("private.txt"), b"private").unwrap();
        symlink(dir.join("private.txt"), dir.join("linked.txt")).unwrap();
        let section = OutputsSection::new(&dir);
        assert_eq!(section.entries().len(), 1);
        section.nodes();
        let key = file_key("private.txt", &Fingerprint::of(&fs::metadata(dir.join("private.txt")).unwrap()));
        fs::remove_file(dir.join("private.txt")).unwrap();
        symlink("linked.txt", dir.join("private.txt")).unwrap();
        assert!(section.find_by_key(&key).is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn thumbnail_rejects_excessive_dimensions_and_refreshes_replacements() {
        let dir = fixture();
        let path = dir.join("preview.png");
        fs::write(&path, tiny_png()).unwrap();
        let section = OutputsSection::new(&dir);
        assert!(section.entries()[0].icon.is_some());
        let mut data = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(MAX_PIXELS_PER_AXIS + 1, 1, image::Rgba([0, 0, 0, 255]))
            .write_to(&mut data, image::ImageFormat::Png).unwrap();
        fs::write(dir.join("replacement.png"), data.into_inner()).unwrap();
        fs::rename(dir.join("replacement.png"), &path).unwrap();
        assert!(section.entries()[0].icon.is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn labels_are_safe_and_file_types_disambiguate_equal_stems() {
        assert_eq!(label_of("a\n\u{202e}b.png"), "ab");
        assert!(file_detail("chart.png", 2048).starts_with("PNG · 2 KB"));
        assert!(file_detail("chart.svg", 2048).starts_with("SVG · 2 KB"));
        assert!(label_of(&"x".repeat(200)).chars().count() <= MAX_LABEL + 1);
    }
}

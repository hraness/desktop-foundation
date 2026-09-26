//! Shared "outputs directory" menu section.
//!
//! Products point an [`OutputsSection`] at a directory their agents drop files
//! into. The section renders one menu item per file — newest first, bounded,
//! with a thumbnail preview for common image types — and answers `open` /
//! `reveal` dispatches. The directory stays the authority; this module only
//! reads it.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
#[cfg(not(unix))]
use std::fs::{File, OpenOptions};
use std::fs::{self, Metadata};
use std::hash::{Hash, Hasher};
use std::io::Cursor;
#[cfg(not(unix))]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::{Alternate, MenuItem, MenuNode, Opens, RgbaIcon, Symbol};

const DEFAULT_LIMIT: usize = 5;
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
struct Listing { files: Vec<ListedFile>, total: usize, partial: bool, available: bool }

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
        let mut listing = Listing { files: Vec::new(), total: 0, partial: false, available: false };
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
        listing.total = listing.files.len();
        listing.files.truncate(self.limit);
        listing
    }

    /// Menu nodes for the section: the newest files (5 by default), each
    /// with its type as a badge, size and age as a subtitle and an Option-key
    /// alternate that shows it in the folder; then one folder row, which
    /// reads "Show all N outputs" when more files exist. Empty and
    /// unavailable folders keep a plain explanation.
    pub fn nodes(&self) -> Vec<MenuNode> {
        self.nodes_at(SystemTime::now())
    }

    fn nodes_at(&self, now: SystemTime) -> Vec<MenuNode> {
        let listing = self.scan();
        let mut offered = HashMap::new();
        let mut nodes = Vec::new();
        if !listing.available {
            if let Ok(mut old) = self.offered.lock() { old.clear(); }
            return vec![MenuNode::disabled("Outputs folder unavailable")];
        }
        if listing.files.is_empty() {
            nodes.push(MenuNode::interactive(
                MenuItem::inert("No outputs yet").with_subtitle("Agents save finished files here"),
            ));
        }
        let shown = listing.files.len();
        for file in listing.files {
            let key = file_key(&file.name, &file.fingerprint);
            offered.insert(key.clone(), (file.path.clone(), file.fingerprint.clone()));
            let entry = self.entry(file);
            let mut item = MenuItem::action(format!("{OPEN_PREFIX}{key}"), label_of(&entry.name))
                .with_symbol(if is_image(&entry.name) { Symbol::ItemImage } else { Symbol::ItemFile })
                .with_badge(file_kind(&entry.name))
                .with_subtitle(file_detail(entry.size, entry.modified, now))
                .with_alternate(
                    Alternate::new(format!("{REVEAL_PREFIX}{key}"), reveal_label()).with_symbol(Symbol::ActionFolder),
                );
            if let Some(icon) = entry.icon { item = item.with_icon(icon); }
            nodes.push(MenuNode::interactive(item));
        }
        if listing.partial {
            nodes.push(MenuNode::disabled("Some outputs couldn't be listed"));
        }
        if let Ok(mut old) = self.offered.lock() { *old = offered; }
        let folder = if listing.total > shown {
            let more = if listing.partial { format!("{}+", listing.total) } else { listing.total.to_string() };
            MenuItem::action(FOLDER_ID, format!("Show all {more} outputs"))
        } else {
            MenuItem::action(FOLDER_ID, "Open outputs folder").with_symbol(Symbol::ActionFolder)
        };
        nodes.push(MenuNode::interactive(folder.opens(Opens::Finder)));
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
        let name = path.file_name()?.to_string_lossy().into_owned();
        #[cfg(unix)]
        let bytes = {
            // The listed identity must still own the path before the cache
            // is consulted or a byte is read.
            let meta = fs::symlink_metadata(path).ok()?;
            if !meta.is_file() || Fingerprint::of(&meta) != *expected {
                return None;
            }
            if let Some(icon) = self.cached_thumbnail(&name, expected) {
                return Some(icon);
            }
            // Shared stable-read custody: `O_NOFOLLOW | O_NONBLOCK` open,
            // fstat-vs-lstat identity proof, then a post-read recheck. The
            // crate additionally requires the file to be owned by the
            // current user and single-linked — bounds the previous
            // open+fstat path never asserted, accepted as custody hardening.
            let result = local_custody::stable_read(
                path,
                &local_custody::StableReadOptions {
                    maximum_bytes: MAX_DECODE_BYTES,
                    nonblocking: true,
                    ..Default::default()
                },
            )
            .ok()?;
            // The read spanned one stable object; that object must still be
            // the one this menu listed.
            let meta = fs::symlink_metadata(path).ok()?;
            if !meta.is_file() || Fingerprint::of(&meta) != *expected {
                return None;
            }
            result.bytes
        };
        #[cfg(not(unix))]
        let bytes = {
            let file = OpenOptions::new().read(true).open(path).ok()?;
            let meta = file.metadata().ok()?;
            if !meta.is_file() || Fingerprint::of(&meta) != *expected || meta.len() > MAX_DECODE_BYTES { return None; }
            if let Some(icon) = self.cached_thumbnail(&name, expected) {
                return Some(icon);
            }
            read_image(file, expected)?
        };
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

    /// A cached thumbnail is served only while the file still carries the
    /// fingerprint it was decoded from.
    fn cached_thumbnail(&self, name: &str, expected: &Fingerprint) -> Option<RgbaIcon> {
        let cache = self.thumbs.lock().ok()?;
        let (cached, icon) = cache.get(name)?;
        (cached == expected).then(|| icon.clone())
    }
}

fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in folder" }
}

#[cfg(not(unix))]
fn read_image(mut file: File, expected: &Fingerprint) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    (&mut file).take(MAX_DECODE_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_DECODE_BYTES || Fingerprint::of(&file.metadata().ok()?) != *expected { return None; }
    Some(bytes)
}

fn is_image(name: &str) -> bool {
    Path::new(name).extension().and_then(|s| s.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "heic" | "svg"))
}

/// The file type as a short badge: `PNG`, `PDF`, `MD`, or `FILE`.
fn file_kind(name: &str) -> String {
    let ext = Path::new(name).extension().and_then(|s| s.to_str()).unwrap_or("");
    let kind: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).take(4).collect::<String>().to_uppercase();
    if kind.is_empty() { "FILE".into() } else { kind }
}

/// Size and age, for example `42 KB · 2 min ago`.
fn file_detail(size: u64, modified: Option<SystemTime>, now: SystemTime) -> String {
    let size = if size < 1024 { format!("{size} B") } else if size < 1024 * 1024 { format!("{} KB", size / 1024) } else { format!("{} MB", size / (1024 * 1024)) };
    let Some(age) = modified.and_then(|modified| now.duration_since(modified).ok()) else { return size };
    let minutes = age.as_secs() / 60;
    let age = match minutes {
        0 => "just now".to_owned(),
        1..=59 => format!("{minutes} min ago"),
        60..=1439 => format!("{} h ago", minutes / 60),
        1440..=2879 => "yesterday".to_owned(),
        _ => format!("{} days ago", minutes / 1440),
    };
    format!("{size} · {age}")
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
    use std::fs::OpenOptions;

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
    fn nodes_use_stem_badge_subtitle_and_an_option_key_reveal() {
        let dir = fixture();
        fs::write(dir.join("chart of options.png"), tiny_png()).unwrap();
        let section = OutputsSection::new(&dir);
        let nodes = section.nodes();
        assert_eq!(nodes.len(), 2);
        let MenuNode::Interactive { item } = &nodes[0] else { panic!("file row") };
        assert_eq!(item.title, "chart of options");
        assert!(item.icon.is_some());
        assert_eq!(item.symbol, Some(Symbol::ItemImage));
        assert_eq!(item.badge.as_deref(), Some("PNG"));
        assert!(item.subtitle.as_deref().is_some_and(|s| s.ends_with(" B · just now")), "{:?}", item.subtitle);
        let alternate = item.alternate.as_ref().unwrap();
        assert!(alternate.id.starts_with(REVEAL_PREFIX));
        assert_eq!(alternate.title, reveal_label());
        let MenuNode::Interactive { item } = &nodes[1] else { panic!("folder row") };
        assert_eq!((item.id.as_deref(), item.title.as_str()), (Some(FOLDER_ID), "Open outputs folder"));
        assert_eq!(item.opens, Some(Opens::Finder));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn more_files_than_the_limit_end_in_show_all() {
        let dir = fixture();
        for index in 0..8 { fs::write(dir.join(format!("report {index}.md")), b"x").unwrap(); }
        let nodes = OutputsSection::new(&dir).nodes();
        assert_eq!(nodes.len(), 6, "five newest files and one folder row");
        let MenuNode::Interactive { item } = &nodes[5] else { panic!("folder row") };
        assert_eq!(item.title, "Show all 8 outputs");
        assert_eq!(item.id.as_deref(), Some(FOLDER_ID));
        let mut model = crate::MenuModel { nodes, ..Default::default() };
        model.nodes.push(MenuNode::quit("Quit"));
        assert!(model.validate().is_ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_dir_explains_itself_and_keeps_the_folder_row() {
        let dir = fixture();
        let nodes = OutputsSection::new(&dir).nodes();
        assert_eq!(nodes, vec![
            MenuNode::interactive(MenuItem::inert("No outputs yet").with_subtitle("Agents save finished files here")),
            MenuNode::interactive(MenuItem::action(FOLDER_ID, "Open outputs folder").with_symbol(Symbol::ActionFolder).opens(Opens::Finder)),
        ]);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(OutputsSection::new(&dir).nodes(), vec![MenuNode::disabled("Outputs folder unavailable")]);
    }

    #[test]
    fn details_read_as_size_and_age() {
        let now = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(10 * 86_400);
        let ago = |seconds: u64| Some(now - std::time::Duration::from_secs(seconds));
        assert_eq!(file_detail(2048, ago(10), now), "2 KB · just now");
        assert_eq!(file_detail(512, ago(120), now), "512 B · 2 min ago");
        assert_eq!(file_detail(3 << 20, ago(3 * 3600), now), "3 MB · 3 h ago");
        assert_eq!(file_detail(1, ago(30 * 3600), now), "1 B · yesterday");
        assert_eq!(file_detail(1, ago(5 * 86_400), now), "1 B · 5 days ago");
        assert_eq!(file_detail(1, None, now), "1 B");
        assert_eq!(file_kind("chart.png"), "PNG");
        assert_eq!(file_kind("notes.markdown"), "MARK");
        assert_eq!(file_kind("README"), "FILE");
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
        assert_eq!(file_kind("chart.png"), "PNG");
        assert_eq!(file_kind("chart.svg"), "SVG");
        assert!(label_of(&"x".repeat(200)).chars().count() <= MAX_LABEL + 1);
    }
}

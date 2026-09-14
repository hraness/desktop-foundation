//! Shared "outputs directory" menu section.
//!
//! Products point an [`OutputsSection`] at a directory their agents drop files
//! into. The section renders one menu item per file — newest first, bounded,
//! with a thumbnail preview for common image types — and answers `open` /
//! `reveal` dispatches. The directory stays the authority; this module only
//! reads it.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::{MenuNode, RgbaIcon};

const DEFAULT_LIMIT: usize = 20;
const MAX_LABEL: usize = 80;
const MAX_DECODE_BYTES: u64 = 24 * 1024 * 1024;
const THUMB_SIZE: u32 = 32;

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
    thumbs: Mutex<HashMap<String, (Option<SystemTime>, RgbaIcon)>>,
}

impl OutputsSection {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        OutputsSection { dir: dir.into(), limit: DEFAULT_LIMIT, thumbs: Mutex::new(HashMap::new()) }
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
        let mut files: Vec<(String, PathBuf, u64, Option<SystemTime>)> = Vec::new();
        if let Ok(read) = fs::read_dir(&self.dir) {
            for entry in read.flatten().take(self.limit * 4) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                if !meta.is_file() {
                    continue;
                }
                files.push((name, entry.path(), meta.len(), meta.modified().ok()));
            }
        }
        files.sort_by(|a, b| b.3.cmp(&a.3));
        files.truncate(self.limit);
        files
            .into_iter()
            .map(|(name, path, size, modified)| {
                OutputEntry { name, size, modified, icon: self.thumbnail(&path, modified) }
            })
            .collect()
    }

    /// Menu nodes for the section: one item per entry plus a folder item.
    /// Empty directories yield a single inert label.
    pub fn nodes(&self) -> Vec<MenuNode> {
        let entries = self.entries();
        if entries.is_empty() {
            return vec![MenuNode::disabled("No outputs yet")];
        }
        let mut nodes = Vec::with_capacity(entries.len() + 1);
        for entry in entries {
            let label = label_of(&entry.name);
            let id = format!("{}{}", OPEN_PREFIX, file_key(&entry.name));
            nodes.push(match entry.icon {
                Some(icon) => MenuNode::item_with_icon(id, label, icon),
                None => MenuNode::item(id, label),
            });
        }
        nodes.push(MenuNode::Separator);
        nodes.push(MenuNode::item(FOLDER_ID, "Reveal Outputs Folder"));
        nodes
    }

    /// Handles this section's action ids. Returns true when the id was ours.
    pub fn dispatch(&self, id: &str) -> bool {
        if id == FOLDER_ID {
            self.open_path(&self.dir, false);
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
        self.entries()
            .into_iter()
            .find(|entry| file_key(&entry.name) == key)
            .map(|entry| self.dir.join(&entry.name))
    }

    fn open_path(&self, path: &Path, reveal: bool) {
        #[cfg(target_os = "macos")]
        {
            let mut command = std::process::Command::new("open");
            if reveal {
                command.arg("-R");
            }
            let _ = command.arg(path).spawn();
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let _ = reveal;
            let _ = std::process::Command::new("xdg-open").arg(path).spawn();
        }
        #[cfg(not(unix))]
        {
            let _ = (path, reveal);
        }
    }

    fn thumbnail(&self, path: &Path, modified: Option<SystemTime>) -> Option<RgbaIcon> {
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico") {
            return None;
        }
        let meta = fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() > MAX_DECODE_BYTES {
            return None;
        }
        let name = path.file_name()?.to_string_lossy().into_owned();
        if let Ok(cache) = self.thumbs.lock() {
            if let Some((cached_mtime, icon)) = cache.get(&name) {
                if *cached_mtime == modified {
                    return Some(icon.clone());
                }
            }
        }
        let bytes = fs::read(path).ok()?;
        let image = image::load_from_memory(&bytes).ok()?;
        let thumb = image.thumbnail(THUMB_SIZE, THUMB_SIZE).to_rgba8();
        let (width, height) = (thumb.width(), thumb.height());
        let icon = RgbaIcon { rgba: thumb.into_raw(), width, height };
        if let Ok(mut cache) = self.thumbs.lock() {
            if cache.len() > self.limit * 2 {
                cache.clear();
            }
            cache.insert(name, (modified, icon.clone()));
        }
        Some(icon)
    }
}

fn label_of(name: &str) -> String {
    let stem = Path::new(name).file_stem().unwrap_or_default().to_string_lossy();
    let mut label: String = stem.chars().take(MAX_LABEL).collect();
    if label.len() < stem.len() {
        label.push('…');
    }
    if label.is_empty() { name.to_string() } else { label }
}

fn file_key(name: &str) -> String {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
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
        assert!(matches!(&nodes[0], MenuNode::Item { title, icon: Some(_), .. } if title == "chart of options"));
        assert!(matches!(&nodes[1], MenuNode::Separator));
        assert!(matches!(&nodes[2], MenuNode::Item { title, .. } if title == "Reveal Outputs Folder"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_dir_yields_inert_label() {
        let dir = fixture();
        let section = OutputsSection::new(&dir);
        assert_eq!(section.nodes(), vec![MenuNode::disabled("No outputs yet")]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn dispatch_owns_only_prefixed_ids() {
        let dir = fixture();
        let section = OutputsSection::new(&dir);
        assert!(section.dispatch(FOLDER_ID));
        assert!(section.dispatch(&format!("{}abc", OPEN_PREFIX)));
        assert!(!section.dispatch("daemon.start"));
        fs::remove_dir_all(&dir).unwrap();
    }
}

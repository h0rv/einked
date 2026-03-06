use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use epub_stream_render::PaginationProfileId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CachedTextRun {
    pub x: i32,
    pub baseline_y: i32,
    pub text: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CachedEpubPage {
    pub text_runs: Vec<CachedTextRun>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedPageRecord {
    pub chapter_index: usize,
    pub page_index: usize,
    pub total_pages: usize,
    pub page: CachedEpubPage,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedChapterEntry {
    pub index: usize,
    pub href: String,
    pub media_type: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedBookSnapshot {
    pub source_path: String,
    pub title: String,
    pub author: String,
    pub language: String,
    pub chapter_count: usize,
    pub chapters: Vec<CachedChapterEntry>,
}

#[derive(Clone, Debug)]
pub struct EpubPageCache {
    root: PathBuf,
    profile_key: String,
}

impl EpubPageCache {
    pub fn for_native_book_path(
        native_path: &str,
        profile: PaginationProfileId,
        font_family_idx: usize,
    ) -> Self {
        Self {
            root: cache_root_for_book(native_path),
            profile_key: profile_cache_key(profile, font_family_idx),
        }
    }

    pub fn load_page(&self, chapter_index: usize, page_index: usize) -> Option<CachedPageRecord> {
        let path = self.page_path(chapter_index, page_index);
        let file = fs::File::open(&path).ok()?;
        match serde_json::from_reader(BufReader::new(file)) {
            Ok(record) => Some(record),
            Err(err) => {
                log::warn!(
                    "epub page cache decode failed at {}: {}",
                    path.display(),
                    err
                );
                None
            }
        }
    }

    pub fn serialize_page(record: &CachedPageRecord) -> Option<Vec<u8>> {
        serde_json::to_vec(record).ok()
    }

    pub fn store_page_bytes(&self, record: &CachedPageRecord, bytes: &[u8]) {
        let path = self.page_path(record.chapter_index, record.page_index);
        let Some(parent) = path.parent() else {
            log::warn!("epub page cache path missing parent: {}", path.display());
            return;
        };
        if let Err(err) = fs::create_dir_all(parent) {
            log::warn!(
                "epub page cache create_dir_all failed at {}: {}",
                parent.display(),
                err
            );
            return;
        }
        let Ok(file) = fs::File::create(path) else {
            log::warn!("epub page cache create failed");
            return;
        };
        let mut writer = BufWriter::new(file);
        if let Err(err) = writer.write_all(bytes) {
            log::warn!("epub page cache write failed: {}", err);
            return;
        }
        if let Err(err) = writer.flush() {
            log::warn!("epub page cache flush failed: {}", err);
        }
    }

    pub fn load_book_snapshot(&self) -> Option<CachedBookSnapshot> {
        let path = self.book_snapshot_path();
        let file = fs::File::open(&path).ok()?;
        match serde_json::from_reader(BufReader::new(file)) {
            Ok(snapshot) => Some(snapshot),
            Err(err) => {
                log::warn!(
                    "epub book snapshot decode failed at {}: {}",
                    path.display(),
                    err
                );
                None
            }
        }
    }

    pub fn store_book_snapshot(&self, snapshot: &CachedBookSnapshot) {
        if let Err(err) = fs::create_dir_all(&self.root) {
            log::warn!(
                "epub book snapshot create_dir_all failed at {}: {}",
                self.root.display(),
                err
            );
            return;
        }
        let path = self.book_snapshot_path();
        let Ok(file) = fs::File::create(path) else {
            log::warn!("epub book snapshot create failed");
            return;
        };
        let mut writer = BufWriter::new(file);
        if let Err(err) = serde_json::to_writer_pretty(&mut writer, snapshot) {
            log::warn!("epub book snapshot write failed: {}", err);
            return;
        }
        if let Err(err) = writer.flush() {
            log::warn!("epub book snapshot flush failed: {}", err);
        }
    }

    fn book_snapshot_path(&self) -> PathBuf {
        self.root.join("book.json")
    }

    fn page_path(&self, chapter_index: usize, page_index: usize) -> PathBuf {
        self.root
            .join(&self.profile_key)
            .join(format!("chapter-{}", chapter_index))
            .join(format!("page-{}.json", page_index))
    }
}

fn cache_root_for_book(native_path: &str) -> PathBuf {
    let path = Path::new(native_path);
    let storage_root = if native_path == "/sd" || native_path.starts_with("/sd/") {
        PathBuf::from("/sd")
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    storage_root
        .join(".xteink")
        .join("epub-cache")
        .join(book_cache_key(native_path))
}

fn book_cache_key(native_path: &str) -> String {
    let mut hasher = DefaultHasher::new();
    native_path.hash(&mut hasher);
    if let Ok(meta) = fs::metadata(native_path) {
        meta.len().hash(&mut hasher);
        if let Ok(modified) = meta.modified()
            && let Ok(duration) = modified.duration_since(UNIX_EPOCH)
        {
            duration.as_secs().hash(&mut hasher);
            duration.subsec_nanos().hash(&mut hasher);
        }
    }
    format!("{:016x}", hasher.finish())
}

fn profile_cache_key(profile: PaginationProfileId, font_family_idx: usize) -> String {
    let profile_hex = profile_hex(profile);
    format!("{}-font-{}", profile_hex, font_family_idx)
}

fn profile_hex(profile: PaginationProfileId) -> String {
    let mut out = String::with_capacity(profile.0.len() * 2);
    for byte in profile.0 {
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{:02x}", byte));
    }
    out
}

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicUsize, Ordering};

use einked::input::{Button, InputEvent};
use einked::refresh::RefreshHint;
use einked::render_ir::DrawCmd;
use einked::storage::{FileStore, FileStoreError, SettingsStore};
use einked_ereader::{DeviceConfig, EreaderRuntime, FrameSink};
use epub_stream::book::OpenConfig;
use epub_stream::metadata::MetadataLimits;
use epub_stream::navigation::NavigationLimits;
use epub_stream::{EpubBook, EpubBookOptions, ValidationMode, ZipLimits};

const EPUB_OPEN_AND_NAV_BUDGET_BYTES: usize = 2 * 1024 * 1024;
const FEED_NAV_BUDGET_BYTES: usize = 512 * 1024;
const EPUB_TEMP_OPEN_MAX_SINGLE_ALLOC_BYTES: usize = 48 * 1024;
const EPUB_OPEN_FIRST_PAGE_BUDGET_BYTES: usize = 1024 * 1024;
const EPUB_OPEN_FIRST_PAGE_MAX_SINGLE_ALLOC_BYTES: usize = 320 * 1024;
const EPUB_PAGE_TURN_BUDGET_BYTES: usize = 512 * 1024;
const EPUB_PAGE_TURN_MAX_SINGLE_ALLOC_BYTES: usize = 128 * 1024;
static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    TEST_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

#[global_allocator]
static ALLOC: BudgetAlloc = BudgetAlloc::new();

struct BudgetAlloc {
    current: AtomicUsize,
    peak: AtomicUsize,
    count: AtomicUsize,
    max_single: AtomicUsize,
}

impl BudgetAlloc {
    const fn new() -> Self {
        Self {
            current: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
            max_single: AtomicUsize::new(0),
        }
    }

    fn reset(&self) {
        self.current.store(0, Ordering::SeqCst);
        self.peak.store(0, Ordering::SeqCst);
        self.count.store(0, Ordering::SeqCst);
        self.max_single.store(0, Ordering::SeqCst);
    }

    fn peak_bytes(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }

    fn alloc_count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    fn max_single_alloc(&self) -> usize {
        self.max_single.load(Ordering::SeqCst)
    }

    fn add_current(&self, bytes: usize) {
        let old = self.current.fetch_add(bytes, Ordering::SeqCst);
        let new = old + bytes;
        let mut max_single = self.max_single.load(Ordering::SeqCst);
        while bytes > max_single {
            match self.max_single.compare_exchange_weak(
                max_single,
                bytes,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(actual) => max_single = actual,
            }
        }
        let mut peak = self.peak.load(Ordering::SeqCst);
        while new > peak {
            match self
                .peak
                .compare_exchange_weak(peak, new, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => break,
                Err(actual) => peak = actual,
            }
        }
    }

    fn sub_current(&self, bytes: usize) {
        let mut current = self.current.load(Ordering::SeqCst);
        loop {
            let next = current.saturating_sub(bytes);
            match self.current.compare_exchange_weak(
                current,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }
}

unsafe impl GlobalAlloc for BudgetAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            self.add_current(layout.size());
            self.count.fetch_add(1, Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        self.sub_current(layout.size());
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            self.add_current(layout.size());
            self.count.fetch_add(1, Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            if new_size >= layout.size() {
                self.add_current(new_size - layout.size());
            } else {
                self.sub_current(layout.size() - new_size);
            }
            self.count.fetch_add(1, Ordering::SeqCst);
        }
        new_ptr
    }
}

struct CaptureSink {
    cmds: Vec<DrawCmd<'static>>,
}

impl CaptureSink {
    fn new() -> Self {
        Self { cmds: Vec::new() }
    }
}

impl FrameSink for CaptureSink {
    fn render_and_flush(&mut self, cmds: &[DrawCmd<'static>], _hint: RefreshHint) -> bool {
        self.cmds = cmds.to_vec();
        true
    }
}

struct TestSettings {
    slots: [u8; 64],
}

impl Default for TestSettings {
    fn default() -> Self {
        Self {
            slots: [u8::MAX; 64],
        }
    }
}

impl SettingsStore for TestSettings {
    fn load_raw(&self, key: u8, buf: &mut [u8]) -> usize {
        let idx = key as usize;
        if idx >= self.slots.len() || buf.is_empty() {
            return 0;
        }
        buf[0] = self.slots[idx];
        1
    }

    fn save_raw(&mut self, key: u8, data: &[u8]) {
        let idx = key as usize;
        if idx < self.slots.len() && !data.is_empty() {
            self.slots[idx] = data[0];
        }
    }
}

struct TestFiles {
    files: BTreeMap<String, Vec<u8>>,
}

impl TestFiles {
    fn from_map(files: BTreeMap<String, Vec<u8>>) -> Self {
        Self { files }
    }
}

impl FileStore for TestFiles {
    fn list(&self, path: &str, out: &mut dyn FnMut(&str)) {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            for key in self.files.keys() {
                if !key.contains('/') {
                    out(key);
                }
            }
            return;
        }
        let prefix = format!("{path}/");
        for key in self.files.keys() {
            if let Some(name) = key.strip_prefix(&prefix)
                && !name.contains('/')
            {
                out(name);
            }
        }
    }

    fn read<'a>(&self, path: &str, buf: &'a mut [u8]) -> Result<&'a [u8], FileStoreError> {
        let key = path.trim_start_matches('/');
        let bytes = self.files.get(key).ok_or(FileStoreError::Io)?;
        let n = bytes.len().min(buf.len());
        buf[..n].copy_from_slice(&bytes[..n]);
        Ok(&buf[..n])
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path.trim_start_matches('/'))
    }

    fn open_read_seek(
        &self,
        path: &str,
    ) -> Result<Box<dyn einked::storage::ReadSeek>, FileStoreError> {
        let key = path.trim_start_matches('/');
        let bytes = self.files.get(key).ok_or(FileStoreError::Io)?;
        Ok(Box::new(Cursor::new(bytes.clone())))
    }
}

fn load_fixture(name: &str) -> Vec<u8> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../sample_books")
        .join(name);
    fs::read(base).expect("fixture book should exist")
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../sample_books")
        .join(name)
}

fn step(runtime: &mut EreaderRuntime, sink: &mut CaptureSink, input: Option<InputEvent>) -> String {
    let _ = runtime.tick(input, sink);
    let mut text = String::new();
    for cmd in &sink.cmds {
        if let DrawCmd::DrawText { text: t, .. } = cmd {
            text.push_str(t.as_str());
            text.push('\n');
        }
    }
    text
}

fn runtime_with_books() -> EreaderRuntime {
    let mut files = BTreeMap::new();
    files.insert(
        "books/Fundamental-Accessibility-Tests-Basic-Functionality-v2.0.0.epub".to_string(),
        load_fixture("Fundamental-Accessibility-Tests-Basic-Functionality-v2.0.0.epub"),
    );
    files.insert(
        "books/pg84-frankenstein.epub".to_string(),
        load_fixture("pg84-frankenstein.epub"),
    );
    files.insert("books/sample.txt".to_string(), load_fixture("sample.txt"));
    EreaderRuntime::with_backends(
        DeviceConfig::xteink_x4(),
        Box::new(TestSettings::default()),
        Box::new(TestFiles::from_map(files)),
    )
}

fn fragment_heap() -> Vec<Box<[u8]>> {
    let mut fragments: Vec<Option<Box<[u8]>>> = Vec::with_capacity(192);
    for idx in 0..192usize {
        let size = match idx % 6 {
            0 => 64,
            1 => 192,
            2 => 512,
            3 => 1024,
            4 => 1536,
            _ => 2048,
        };
        fragments.push(Some(vec![0xA5; size].into_boxed_slice()));
    }
    for (idx, slot) in fragments.iter_mut().enumerate() {
        if idx % 2 == 1 || idx % 7 == 0 {
            *slot = None;
        }
    }
    fragments.into_iter().flatten().collect()
}

#[test]
fn epub_open_and_navigation_within_budget() {
    let _guard = test_guard();
    let mut runtime = runtime_with_books();
    let mut sink = CaptureSink::new();
    let _ = step(&mut runtime, &mut sink, None);

    ALLOC.reset();
    let s1 = step(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Confirm)),
    );
    assert!(
        !s1.contains("No readable text produced by renderer."),
        "epub render failed on open"
    );
    let _ = step(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Right)),
    );
    let _ = step(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Right)),
    );
    let _ = step(&mut runtime, &mut sink, Some(InputEvent::Press(Button::Aux2)));
    let peak = ALLOC.peak_bytes();
    let max_single = ALLOC.max_single_alloc();
    assert!(
        peak <= EPUB_OPEN_AND_NAV_BUDGET_BYTES,
        "epub open/navigation peak over budget: {} bytes ({:.1} KiB), allocs={}, max_single={}",
        peak,
        peak as f64 / 1024.0,
        ALLOC.alloc_count(),
        max_single
    );
    eprintln!(
        "[ui-harness] epub_open_nav peak={} max_single={} allocs={}",
        peak,
        max_single,
        ALLOC.alloc_count()
    );
}

#[test]
fn feed_navigation_within_budget() {
    let _guard = test_guard();
    let mut runtime = runtime_with_books();
    let mut sink = CaptureSink::new();
    let _ = step(&mut runtime, &mut sink, None);

    ALLOC.reset();
    let _ = step(&mut runtime, &mut sink, Some(InputEvent::Press(Button::Right)));
    let _ = step(&mut runtime, &mut sink, Some(InputEvent::Press(Button::Right)));
    let s = step(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Confirm)),
    );
    assert!(s.contains("Feed"));
    let peak = ALLOC.peak_bytes();
    let max_single = ALLOC.max_single_alloc();
    assert!(
        peak <= FEED_NAV_BUDGET_BYTES,
        "feed navigation peak over budget: {} bytes ({:.1} KiB), allocs={}, max_single={}",
        peak,
        peak as f64 / 1024.0,
        ALLOC.alloc_count(),
        max_single
    );
}

#[test]
fn epub_open_survives_fragmented_allocator() {
    let _guard = test_guard();
    let kept_fragments = fragment_heap();
    ALLOC.reset();

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let mut runtime = runtime_with_books();
        let mut sink = CaptureSink::new();
        let _ = step(&mut runtime, &mut sink, None);
        let _ = step(
            &mut runtime,
            &mut sink,
            Some(InputEvent::Press(Button::Confirm)),
        );
        let _ = step(
            &mut runtime,
            &mut sink,
            Some(InputEvent::Press(Button::Right)),
        );
    }));

    drop(kept_fragments);

    assert!(
        result.is_ok(),
        "epub open/page-turn panicked under fragmented allocator pressure"
    );
}

#[test]
fn epub_temp_open_embedded_limits_has_bounded_single_alloc() {
    let _guard = test_guard();
    let book_path = fixture_path("Fundamental-Accessibility-Tests-Basic-Functionality-v2.0.0.epub");
    let temp_dir = std::env::temp_dir().join("xteink-epub-temp");
    let _ = fs::create_dir_all(&temp_dir);

    let open_cfg = OpenConfig {
        options: EpubBookOptions {
            zip_limits: Some(ZipLimits::new(256 * 1024, 128).with_max_eocd_scan(8 * 1024)),
            validation_mode: ValidationMode::Lenient,
            max_nav_bytes: Some(32 * 1024),
            navigation_limits: NavigationLimits::embedded(),
            metadata_limits: MetadataLimits::embedded(),
        },
        lazy_navigation: true,
    };

    ALLOC.reset();
    let book = EpubBook::open_with_temp_storage(&book_path, &temp_dir, open_cfg)
        .expect("temp-storage open should succeed");
    assert!(book.chapter_count() > 0, "book should expose chapters");
    let max_single = ALLOC.max_single_alloc();
    eprintln!(
        "[ui-harness] temp_open max_single={} peak={} allocs={}",
        max_single,
        ALLOC.peak_bytes(),
        ALLOC.alloc_count()
    );
    assert!(
        max_single <= EPUB_TEMP_OPEN_MAX_SINGLE_ALLOC_BYTES,
        "temp open single allocation too large: {} bytes (limit {})",
        max_single,
        EPUB_TEMP_OPEN_MAX_SINGLE_ALLOC_BYTES
    );
}

#[test]
fn epub_open_and_first_page_have_bounded_phase_allocations() {
    let _guard = test_guard();
    let mut runtime = runtime_with_books();
    let mut sink = CaptureSink::new();
    let _ = step(&mut runtime, &mut sink, None);

    ALLOC.reset();
    let open_text = step(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Confirm)),
    );
    assert!(
        !open_text.contains("Failed to parse EPUB")
            && !open_text.contains("No readable text produced by renderer."),
        "epub open should produce a usable reader state"
    );
    let peak = ALLOC.peak_bytes();
    let max_single = ALLOC.max_single_alloc();
    eprintln!(
        "[ui-harness] epub_open_first_page peak={} max_single={} allocs={}",
        peak,
        max_single,
        ALLOC.alloc_count()
    );
    assert!(
        peak <= EPUB_OPEN_FIRST_PAGE_BUDGET_BYTES,
        "epub first-page open peak too high: {} bytes (limit {})",
        peak,
        EPUB_OPEN_FIRST_PAGE_BUDGET_BYTES
    );
    assert!(
        max_single <= EPUB_OPEN_FIRST_PAGE_MAX_SINGLE_ALLOC_BYTES,
        "epub first-page single allocation too high: {} bytes (limit {})",
        max_single,
        EPUB_OPEN_FIRST_PAGE_MAX_SINGLE_ALLOC_BYTES
    );
}

#[test]
fn epub_page_turn_has_bounded_phase_allocations() {
    let _guard = test_guard();
    let mut runtime = runtime_with_books();
    let mut sink = CaptureSink::new();
    let _ = step(&mut runtime, &mut sink, None);
    let _ = step(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Confirm)),
    );

    ALLOC.reset();
    let _ = step(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Right)),
    );
    let peak = ALLOC.peak_bytes();
    let max_single = ALLOC.max_single_alloc();
    eprintln!(
        "[ui-harness] epub_page_turn peak={} max_single={} allocs={}",
        peak,
        max_single,
        ALLOC.alloc_count()
    );
    assert!(
        peak <= EPUB_PAGE_TURN_BUDGET_BYTES,
        "epub page-turn peak too high: {} bytes (limit {})",
        peak,
        EPUB_PAGE_TURN_BUDGET_BYTES
    );
    assert!(
        max_single <= EPUB_PAGE_TURN_MAX_SINGLE_ALLOC_BYTES,
        "epub page-turn single allocation too high: {} bytes (limit {})",
        max_single,
        EPUB_PAGE_TURN_MAX_SINGLE_ALLOC_BYTES
    );
}

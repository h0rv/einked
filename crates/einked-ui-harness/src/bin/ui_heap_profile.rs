use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use einked::input::{Button, InputEvent};
use einked::refresh::RefreshHint;
use einked::render_ir::DrawCmd;
use einked::storage::{FileStore, FileStoreError, SettingsStore};
use einked_ereader::{DeviceConfig, EreaderRuntime, FrameSink};
use einked_ui_harness::shared_bytes_reader::SharedBytesReader;

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
    files: BTreeMap<String, Arc<[u8]>>,
}

impl TestFiles {
    fn from_map(files: BTreeMap<String, Arc<[u8]>>) -> Self {
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
        Ok(Box::new(SharedBytesReader::new(Arc::clone(bytes))))
    }
}

fn load_fixture(name: &str) -> Arc<[u8]> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../sample_books")
        .join(name);
    Arc::<[u8]>::from(fs::read(base).expect("fixture book should exist"))
}

fn runtime_with_books() -> EreaderRuntime {
    let mut files = BTreeMap::new();
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

fn step(runtime: &mut EreaderRuntime, sink: &mut CaptureSink, input: Option<InputEvent>) {
    let _ = runtime.tick(input, sink);
}

fn write_profile(
    out_dir: &Path,
    name: &str,
    peak_bytes: usize,
    alloc_count: usize,
    max_single_alloc_bytes: usize,
) {
    let path = out_dir.join(format!("ui-mem-{name}.json"));
    let body = format!(
        "{{\"phase\":\"{}\",\"peak_bytes\":{},\"peak_kib\":{:.1},\"alloc_count\":{},\"max_single_alloc_bytes\":{}}}\n",
        name,
        peak_bytes,
        peak_bytes as f64 / 1024.0,
        alloc_count,
        max_single_alloc_bytes
    );
    let _ = fs::write(path, body);
}

fn profile_phase<F>(out_dir: &Path, name: &str, f: F)
where
    F: FnOnce(),
{
    ALLOC.reset();
    f();
    let peak = ALLOC.peak_bytes();
    let allocs = ALLOC.alloc_count();
    let max_single = ALLOC.max_single_alloc();
    write_profile(out_dir, name, peak, allocs, max_single);
    eprintln!(
        "profile phase={} peak={} bytes ({:.1} KiB) allocs={} max_single={}",
        name,
        peak,
        peak as f64 / 1024.0,
        allocs,
        max_single
    );
}

fn profile_runtime_phase<F>(out_dir: &Path, name: &str, f: F)
where
    F: FnOnce(&mut EreaderRuntime, &mut CaptureSink),
{
    let mut runtime = runtime_with_books();
    let mut sink = CaptureSink::new();
    step(&mut runtime, &mut sink, None);

    ALLOC.reset();
    f(&mut runtime, &mut sink);

    let peak = ALLOC.peak_bytes();
    let allocs = ALLOC.alloc_count();
    let max_single = ALLOC.max_single_alloc();
    write_profile(out_dir, name, peak, allocs, max_single);
    eprintln!(
        "profile phase={} peak={} bytes ({:.1} KiB) allocs={} max_single={}",
        name,
        peak,
        peak as f64 / 1024.0,
        allocs,
        max_single
    );
}

fn parse_out_dir() -> PathBuf {
    let mut args = std::env::args().skip(1);
    let mut out_dir = PathBuf::from("target/ui-memory");
    while let Some(arg) = args.next() {
        if arg == "--out-dir"
            && let Some(value) = args.next()
        {
            out_dir = PathBuf::from(value);
        }
    }
    out_dir
}

fn main() {
    let out_dir = parse_out_dir();
    let _ = fs::create_dir_all(&out_dir);

    profile_phase(&out_dir, "boot", || {
        let mut runtime = runtime_with_books();
        let mut sink = CaptureSink::new();
        step(&mut runtime, &mut sink, None);
    });

    profile_runtime_phase(&out_dir, "epub_open_nav", |runtime, sink| {
        step(runtime, sink, Some(InputEvent::Press(Button::Confirm)));
        step(runtime, sink, Some(InputEvent::Press(Button::Right)));
        step(runtime, sink, Some(InputEvent::Press(Button::Right)));
        step(runtime, sink, Some(InputEvent::Press(Button::Aux2)));
    });

    profile_runtime_phase(&out_dir, "epub_open_first_page", |runtime, sink| {
        step(runtime, sink, Some(InputEvent::Press(Button::Confirm)));
    });

    profile_runtime_phase(&out_dir, "epub_page_turn_forward", |runtime, sink| {
        step(runtime, sink, Some(InputEvent::Press(Button::Confirm)));
        step(runtime, sink, Some(InputEvent::Press(Button::Right)));
    });

    profile_runtime_phase(&out_dir, "feed_nav", |runtime, sink| {
        step(runtime, sink, Some(InputEvent::Press(Button::Right)));
        step(runtime, sink, Some(InputEvent::Press(Button::Right)));
        step(runtime, sink, Some(InputEvent::Press(Button::Confirm)));
        step(runtime, sink, Some(InputEvent::Press(Button::Confirm)));
    });

    profile_runtime_phase(&out_dir, "settings_nav", |runtime, sink| {
        step(runtime, sink, Some(InputEvent::Press(Button::Right)));
        step(runtime, sink, Some(InputEvent::Press(Button::Right)));
        step(runtime, sink, Some(InputEvent::Press(Button::Right)));
        step(runtime, sink, Some(InputEvent::Press(Button::Confirm)));
        step(runtime, sink, Some(InputEvent::Press(Button::Down)));
        step(runtime, sink, Some(InputEvent::Press(Button::Confirm)));
    });

    eprintln!(
        "wrote profiles to {} (ui-mem-*.json)",
        out_dir.to_string_lossy()
    );
}

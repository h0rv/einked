use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};

use einked::storage::{FileStore, FileStoreError, SettingsStore};
use einked_ereader::{DeviceConfig, EreaderRuntime, default_feed_client};

#[global_allocator]
static ALLOC: TrackingAlloc = TrackingAlloc::new();

struct TrackingAlloc {
    current: AtomicUsize,
    peak: AtomicUsize,
    count: AtomicUsize,
    max_single: AtomicUsize,
}

impl TrackingAlloc {
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

    fn current_bytes(&self) -> usize {
        self.current.load(Ordering::SeqCst)
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

unsafe impl GlobalAlloc for TrackingAlloc {
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

struct TestSettings {
    slots: [u8; 64],
}

impl Default for TestSettings {
    fn default() -> Self {
        Self { slots: [0; 64] }
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
    fn empty() -> Self {
        Self {
            files: BTreeMap::new(),
        }
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

#[derive(Clone, Debug)]
struct PhaseRecord {
    label: String,
    current_bytes: usize,
    delta_bytes: isize,
    peak_bytes: usize,
    alloc_count: usize,
    max_single_alloc_bytes: usize,
}

fn snapshot(label: &str, previous_current: &mut usize) -> PhaseRecord {
    let current = ALLOC.current_bytes();
    let delta = current as isize - *previous_current as isize;
    *previous_current = current;
    PhaseRecord {
        label: label.to_string(),
        current_bytes: current,
        delta_bytes: delta,
        peak_bytes: ALLOC.peak_bytes(),
        alloc_count: ALLOC.alloc_count(),
        max_single_alloc_bytes: ALLOC.max_single_alloc(),
    }
}

fn main() {
    let mut phases = Vec::with_capacity(32);
    let mut previous_current = 0usize;

    ALLOC.reset();
    phases.push(snapshot("baseline", &mut previous_current));

    let mut probe = |label: &'static str| {
        phases.push(snapshot(label, &mut previous_current));
    };

    let runtime = EreaderRuntime::with_backends_and_feed_with_probe(
        DeviceConfig::xteink_x4(),
        Box::new(TestSettings::default()),
        Box::new(TestFiles::empty()),
        default_feed_client(),
        &mut probe,
    );

    phases.push(snapshot("ereader_runtime:constructed", &mut previous_current));
    drop(runtime);
    phases.push(snapshot("ereader_runtime:dropped", &mut previous_current));

    println!(
        "phase,current_bytes,delta_bytes,peak_bytes,alloc_count,max_single_alloc_bytes"
    );
    for phase in phases {
        println!(
            "{},{},{},{},{},{}",
            phase.label,
            phase.current_bytes,
            phase.delta_bytes,
            phase.peak_bytes,
            phase.alloc_count,
            phase.max_single_alloc_bytes
        );
    }
}

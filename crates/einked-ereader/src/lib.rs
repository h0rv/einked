#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use einked::activity_stack::{Activity, ActivityStack, Context, Transition, Ui};
use einked::core::{Color, DefaultTheme, Point, Rect};
use einked::dsl::UiDsl;
use einked::input::{Button, InputEvent};
use einked::pipeline::FramePipeline;
use einked::refresh::RefreshHint;
use einked::render_ir::{DrawCmd, ImageFormat};
use einked::storage::{FileStore, FileStoreError, SettingsStore};
use einked::ui::runtime::UiRuntime;
#[cfg(all(feature = "std", target_os = "espidf"))]
use esp_idf_svc::sys;
#[cfg(all(feature = "std", target_os = "espidf"))]
use std::fs;
#[cfg(all(feature = "std", target_os = "espidf"))]
use std::fs::File;

#[cfg(feature = "std")]
use epub_stream::book::ChapterEventsOptions;
#[cfg(all(feature = "std", target_os = "espidf"))]
use epub_stream::book::OpenConfig;
#[cfg(all(feature = "std", target_os = "espidf"))]
use epub_stream::metadata::MetadataLimits;
#[cfg(all(feature = "std", target_os = "espidf"))]
use epub_stream::navigation::NavigationLimits;
#[cfg(feature = "std")]
use epub_stream::{
    EpubBook, FontLimits, LayoutHints, MemoryBudget, RenderPrepOptions, ScratchBuffers,
    StyleConfig, StyleLimits,
};
#[cfg(all(feature = "std", target_os = "espidf"))]
use epub_stream::{EpubBookOptions, ValidationMode, ZipLimits};
#[cfg(feature = "std")]
use epub_stream_embedded_graphics::{
    EgRenderConfig, EgRenderer, MonoFontBackend, PackedBinaryFrameBuffer, StreamedImageOptions,
};
#[cfg(feature = "std")]
use epub_stream_render::{
    HyphenationMode, JustificationStrategy, RenderConfig, RenderEngine, RenderEngineOptions,
    RenderPage,
};

#[cfg(feature = "std")]
mod epub_cache;

pub mod embedded_fonts;
pub mod feed;
pub mod feed_browser;

pub use embedded_fonts::{
    BOOKERLY_BOLD, BOOKERLY_BOLD_ITALIC, BOOKERLY_ITALIC, BOOKERLY_REGULAR, BOOKERLY_SET,
    EmbeddedFont,
};
#[cfg(feature = "std")]
use epub_cache::{
    CachedBookSnapshot, CachedChapterEntry, CachedPageRecord, CachedRenderPage, EpubPageCache,
};
pub use feed::{
    FeedClient, FeedEntryData, FeedSource, FeedType, JINA_READER_BASE, NoopFeedClient, OpdsCatalog,
    OpdsEntry, OpdsLink, PRELOADED_OPDS_SOURCES, PRELOADED_RSS_SOURCES, all_preloaded_sources,
    default_feed_client, get_reader_url,
};
pub use feed_browser::{BrowserState, FeedBrowserActivity};

pub trait FrameSink {
    fn render_and_flush(&mut self, cmds: &[DrawCmd<'static>], hint: RefreshHint) -> bool;
}

fn boot_probe(probe: &mut dyn FnMut(&'static str), label: &'static str) {
    probe(label);
}

#[cfg(target_os = "espidf")]
const FRAME_CMD_CAPACITY: usize = 32;
#[cfg(not(target_os = "espidf"))]
const FRAME_CMD_CAPACITY: usize = 512;

#[cfg(target_os = "espidf")]
const FRAME_PREV_CAPACITY: usize = 8;
#[cfg(not(target_os = "espidf"))]
const FRAME_PREV_CAPACITY: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayType {
    Mono1Bpp,
    Gray4,
    TriColorBwr,
    Custom(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceConfig {
    pub name: &'static str,
    pub screen: Rect,
    pub display_type: DisplayType,
    pub partial_refresh_limit: u8,
    pub text_read_buffer_bytes: usize,
}

impl DeviceConfig {
    pub const fn xteink_x4() -> Self {
        Self {
            name: "xteink-x4",
            screen: Rect {
                x: 0,
                y: 0,
                width: 480,
                height: 800,
            },
            display_type: DisplayType::Mono1Bpp,
            partial_refresh_limit: 8,
            text_read_buffer_bytes: 64 * 1024,
        }
    }
}

pub struct EreaderRuntime {
    stack: ActivityStack<DefaultTheme, 8>,
    pipeline: FramePipeline<FRAME_CMD_CAPACITY, FRAME_PREV_CAPACITY>,
    theme: DefaultTheme,
    settings: Box<dyn SettingsStore>,
    files: Box<dyn FileStore>,
    config: DeviceConfig,
}

impl EreaderRuntime {
    pub fn new(config: DeviceConfig) -> Self {
        Self::with_backends(
            config,
            Box::new(NoopSettings::default()),
            Box::new(NoopFiles),
        )
    }

    pub fn with_backends(
        config: DeviceConfig,
        settings: Box<dyn SettingsStore>,
        files: Box<dyn FileStore>,
    ) -> Self {
        Self::with_backends_and_feed(config, settings, files, default_feed_client())
    }

    pub fn with_backends_and_feed(
        config: DeviceConfig,
        settings: Box<dyn SettingsStore>,
        files: Box<dyn FileStore>,
        feed_client: Box<dyn FeedClient>,
    ) -> Self {
        let mut noop_probe = |_| {};
        Self::with_backends_and_feed_with_probe(
            config,
            settings,
            files,
            feed_client,
            &mut noop_probe,
        )
    }

    pub fn with_backends_and_feed_with_probe(
        config: DeviceConfig,
        mut settings: Box<dyn SettingsStore>,
        mut files: Box<dyn FileStore>,
        feed_client: Box<dyn FeedClient>,
        probe: &mut dyn FnMut(&'static str),
    ) -> Self {
        boot_probe(probe, "ereader_runtime:start");
        let mut stack = ActivityStack::new();
        boot_probe(probe, "ereader_runtime:after_stack");
        let theme = DefaultTheme;
        let feed_client = Rc::new(RefCell::new(feed_client));
        boot_probe(probe, "ereader_runtime:after_feed_client_rc");
        let mut ctx = Context {
            theme: &theme,
            screen: config.screen,
            settings: settings.as_mut(),
            files: files.as_mut(),
        };
        boot_probe(probe, "ereader_runtime:after_context");
        let home =
            HomeActivity::new_with_device_and_feed_with_probe(config, feed_client.clone(), probe);
        boot_probe(probe, "ereader_runtime:after_home_activity");
        let _ = stack.push_root(Box::new(home), &mut ctx);
        boot_probe(probe, "ereader_runtime:after_push_root");

        let mut pipeline = FramePipeline::new(config.partial_refresh_limit);
        boot_probe(probe, "ereader_runtime:after_pipeline_new");
        pipeline.set_viewport_width(config.screen.width);
        boot_probe(probe, "ereader_runtime:after_pipeline_viewport");

        let runtime = Self {
            stack,
            pipeline,
            theme,
            settings,
            files,
            config,
        };
        boot_probe(probe, "ereader_runtime:ready");
        runtime
    }

    pub fn config(&self) -> DeviceConfig {
        self.config
    }

    pub fn tick(&mut self, input: Option<InputEvent>, sink: &mut impl FrameSink) -> bool {
        let mut ctx = Context {
            theme: &self.theme,
            screen: self.config.screen,
            settings: self.settings.as_mut(),
            files: self.files.as_mut(),
        };

        let hint;
        {
            let runtime = self.pipeline.begin_frame();
            let mut ui = RuntimeUi { runtime };
            let alive = self.stack.tick(input, &mut ui, &mut ctx);
            if !alive {
                return false;
            }
            hint = ui.runtime.take_refresh_hint();
        }

        sink.render_and_flush(self.pipeline.current_commands(), hint)
    }
}

impl Default for EreaderRuntime {
    fn default() -> Self {
        Self::new(DeviceConfig::xteink_x4())
    }
}

struct RuntimeUi<'a> {
    runtime: UiRuntime<'a, FRAME_CMD_CAPACITY>,
}

impl Ui<DefaultTheme> for RuntimeUi<'_> {
    fn clear(&mut self, _theme: &DefaultTheme) {}

    fn label(&mut self, text: &str) {
        self.runtime.label(text);
    }

    fn paragraph(&mut self, text: &str) {
        self.runtime.paragraph(text);
    }

    fn divider(&mut self) {
        self.runtime.draw_divider();
    }

    fn status_bar(&mut self, left: &str, right: &str) {
        self.runtime.draw_status_bar(left, right);
    }

    fn set_refresh_hint(&mut self, hint: RefreshHint) {
        self.runtime.set_refresh_hint(hint);
    }

    fn draw_text_at(&mut self, pos: Point, text: &str) {
        self.runtime.draw_text_at(pos, text);
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.runtime.fill_rect(rect, color);
    }

    fn draw_line(&mut self, start: Point, end: Point, color: Color, width: u8) {
        self.runtime.draw_line(start, end, color, width);
    }

    fn draw_image(
        &mut self,
        rect: Rect,
        data: &'static [u8],
        format: ImageFormat,
        generation: u32,
    ) {
        self.runtime.draw_image(rect, data, format, generation);
    }
}

struct NoopSettings {
    slots: [u8; 256],
}

impl Default for NoopSettings {
    fn default() -> Self {
        Self {
            slots: [u8::MAX; 256],
        }
    }
}

impl SettingsStore for NoopSettings {
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

struct NoopFiles;

impl FileStore for NoopFiles {
    fn list(&self, _path: &str, _out: &mut dyn FnMut(&str)) {}

    fn read<'a>(&self, _path: &str, _buf: &'a mut [u8]) -> Result<&'a [u8], FileStoreError> {
        Err(FileStoreError::Io)
    }

    fn exists(&self, _path: &str) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MainTab {
    Library,
    Files,
    Feed,
    Settings,
}

impl MainTab {
    fn next(self) -> Self {
        match self {
            Self::Library => Self::Files,
            Self::Files => Self::Feed,
            Self::Feed => Self::Settings,
            Self::Settings => Self::Library,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Library => Self::Settings,
            Self::Files => Self::Library,
            Self::Feed => Self::Files,
            Self::Settings => Self::Feed,
        }
    }

    fn dot_label(self) -> &'static str {
        match self {
            Self::Library => "O o o o",
            Self::Files => "o O o o",
            Self::Feed => "o o O o",
            Self::Settings => "o o o O",
        }
    }
}

enum ModalState {
    None,
    Transfer,
    Reader {
        title: String,
        lines: Vec<String>,
        scroll: usize,
    },
    EpubReader,
    FeedEntries {
        source_idx: usize,
        title: String,
        entries: Vec<FeedEntry>,
        selected_idx: usize,
    },
    FeedItem {
        source_idx: usize,
        title: String,
        entries: Vec<FeedEntry>,
        item_idx: usize,
        lines: Vec<String>,
        scroll: usize,
    },
    FeedArticle {
        source_idx: usize,
        title: String,
        entries: Vec<FeedEntry>,
        item_idx: usize,
        lines: Vec<String>,
        scroll: usize,
    },
    FeedOffline {
        source_idx: usize,
        title: String,
        message: String,
        requested_enable: bool,
        requires_wifi: bool,
    },
}

#[derive(Clone)]
struct FeedEntry {
    title: String,
    url: Option<String>,
    summary: Option<String>,
}

#[derive(Clone, Copy)]
struct EpubLoadConfig {
    font_size_idx: usize,
    auto_sleep_idx: usize,
    font_family_idx: usize,
    display_width: i32,
    display_height: i32,
}

#[cfg(feature = "std")]
struct EpubPageWindow {
    chapter_count: usize,
    chapter_idx: usize,
    total_pages: usize,
    page_window_start: usize,
    pages: Vec<RenderPage>,
}

#[cfg(feature = "std")]
struct EpubPageBitmap {
    width: u32,
    height: u32,
    generation: u32,
    bytes: *mut [u8],
}

#[cfg(feature = "std")]
impl EpubPageBitmap {
    fn try_new(width: u32, height: u32) -> Result<Self, String> {
        let len = (width as usize).div_ceil(8).saturating_mul(height as usize);
        let bytes = HomeActivity::try_alloc_zeroed(len)
            .map_err(|_| format!("Unable to allocate {} bytes for EPUB page bitmap", len))?
            .into_boxed_slice();
        Ok(Self {
            width,
            height,
            generation: 0,
            bytes: Box::into_raw(bytes),
        })
    }

    fn clear(&mut self) {
        self.bytes_mut().fill(0);
    }

    fn next_generation(&mut self) -> u32 {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.generation
    }

    fn bytes(&self) -> &'static [u8] {
        // SAFETY: the backing slice is leaked in `new`, so the pointer remains valid
        // for the process lifetime. Callers only take shared views from this method.
        unsafe { &*self.bytes }
    }

    fn bytes_mut(&mut self) -> &'static mut [u8] {
        // SAFETY: the backing slice is leaked in `new`, so the pointer remains valid
        // for the process lifetime. `&mut self` guarantees unique access while drawing.
        unsafe { &mut *self.bytes }
    }
}

#[cfg(feature = "std")]
struct EpubResources {
    page_window: Vec<RenderPage>,
    page_bitmap: Option<EpubPageBitmap>,
}

#[cfg(feature = "std")]
#[derive(Clone, Debug, Default)]
struct EpubReaderState {
    chapter_idx: usize,
    chapter_count: usize,
    page_window_start: usize,
    total_pages: usize,
    page_idx: usize,
}

#[cfg(feature = "std")]
impl Default for EpubResources {
    fn default() -> Self {
        Self {
            page_window: Vec::new(),
            page_bitmap: None,
        }
    }
}

#[cfg(feature = "std")]
struct EpubSession {
    book: EpubSessionBook,
    engine: RenderEngine,
    font_family_idx: usize,
    resources: EpubResources,
    reader: EpubReaderState,
    cache: Option<EpubPageCache>,
}

#[cfg(feature = "std")]
enum EpubSessionBook {
    #[cfg(not(target_os = "espidf"))]
    Generic(EpubBook<Box<dyn einked::storage::ReadSeek>>),
    #[cfg(target_os = "espidf")]
    Temp(EpubBook<File>),
}

#[cfg(feature = "std")]
impl EpubSession {
    fn into_resources(mut self: Box<Self>) -> EpubResources {
        self.resources.page_window.clear();
        if let Some(bitmap) = self.resources.page_bitmap.as_mut() {
            bitmap.clear();
        }
        self.resources
    }

    fn replace_page_window(&mut self, pages: Vec<RenderPage>) {
        self.resources.page_window.clear();
        self.resources.page_window.extend(pages);
    }

    fn clear_page_window(&mut self) {
        self.resources.page_window.clear();
        if let Some(bitmap) = self.resources.page_bitmap.as_mut() {
            bitmap.clear();
            bitmap.next_generation();
        }
    }

    fn apply_reader_window(&mut self, window: EpubPageWindow, page_idx: usize) {
        self.reader.chapter_idx = window.chapter_idx;
        self.reader.chapter_count = window.chapter_count;
        self.reader.page_window_start = window.page_window_start;
        self.reader.total_pages = window.total_pages;
        self.reader.page_idx = page_idx;
        self.replace_page_window(window.pages);
    }

    fn page_window_len(&self) -> usize {
        self.resources.page_window.len()
    }
}

struct HomeActivity {
    tab: MainTab,
    library_idx: usize,
    files_idx: usize,
    feed_idx: usize,
    settings_idx: usize,
    transfer_menu_idx: usize,
    files: Vec<String>,
    feed_sources: Vec<(String, String, FeedType)>,
    font_size_idx: usize,
    font_family_idx: usize,
    auto_sleep_idx: usize,
    refresh_idx: usize,
    invert_colors_idx: usize,
    text_read_buffer_bytes: usize,
    epub_display_width: i32,
    epub_display_height: i32,
    #[cfg(feature = "std")]
    epub_session: Option<Box<EpubSession>>,
    #[cfg(feature = "std")]
    epub_resources: EpubResources,
    feed_client: Rc<RefCell<Box<dyn FeedClient>>>,
    modal: ModalState,
}

impl HomeActivity {
    #[cfg(target_os = "espidf")]
    const EPUB_PAGE_WINDOW: usize = 1;
    #[cfg(not(target_os = "espidf"))]
    const EPUB_PAGE_WINDOW: usize = 6;
    #[cfg(target_os = "espidf")]
    const EPUB_MAX_CHAPTER_EVENTS: usize = 8_192;
    #[cfg(not(target_os = "espidf"))]
    const EPUB_MAX_CHAPTER_EVENTS: usize = 65_536;
    #[cfg(target_os = "espidf")]
    const EPUB_MAX_CHAPTER_BUF_CAPACITY_BYTES: usize = 64 * 1024;
    #[cfg(not(target_os = "espidf"))]
    const EPUB_MAX_CHAPTER_BUF_CAPACITY_BYTES: usize = 512 * 1024;
    #[cfg(target_os = "espidf")]
    const EPUB_MAX_CACHE_PAGE_BYTES: usize = 64 * 1024;
    #[cfg(not(target_os = "espidf"))]
    const EPUB_MAX_CACHE_PAGE_BYTES: usize = 256 * 1024;

    fn new_with_device_and_feed(
        config: DeviceConfig,
        feed_client: Rc<RefCell<Box<dyn FeedClient>>>,
    ) -> Self {
        let mut noop_probe = |_| {};
        Self::new_with_device_and_feed_with_probe(config, feed_client, &mut noop_probe)
    }

    fn new_with_device_and_feed_with_probe(
        config: DeviceConfig,
        feed_client: Rc<RefCell<Box<dyn FeedClient>>>,
        probe: &mut dyn FnMut(&'static str),
    ) -> Self {
        boot_probe(probe, "home_activity:start");
        let feed_sources = Self::build_feed_sources();
        boot_probe(probe, "home_activity:after_feed_sources");
        let activity = Self {
            tab: MainTab::Library,
            library_idx: 0,
            files_idx: 0,
            feed_idx: 0,
            settings_idx: 0,
            transfer_menu_idx: 0,
            files: Vec::new(),
            feed_sources,
            font_size_idx: 1,
            font_family_idx: 0,
            auto_sleep_idx: 1,
            refresh_idx: 0,
            invert_colors_idx: 0,
            text_read_buffer_bytes: config.text_read_buffer_bytes,
            epub_display_width: config.screen.width as i32,
            epub_display_height: config.screen.height as i32,
            #[cfg(feature = "std")]
            epub_session: None,
            #[cfg(feature = "std")]
            epub_resources: EpubResources::default(),
            feed_client,
            modal: ModalState::None,
        };
        #[cfg(feature = "std")]
        boot_probe(probe, "home_activity:epub_resources_deferred");
        boot_probe(probe, "home_activity:ready");
        activity
    }

    fn build_feed_sources() -> Vec<(String, String, FeedType)> {
        let mut feed_sources = Vec::new();
        for (name, url, ty) in all_preloaded_sources() {
            feed_sources.push((name.to_string(), url.to_string(), ty));
        }
        feed_sources
    }

    #[cfg(feature = "std")]
    fn preallocated_epub_resources() -> Result<EpubResources, String> {
        let mut noop_probe = |_| {};
        Self::preallocated_epub_resources_with_probe(&mut noop_probe)
    }

    #[cfg(feature = "std")]
    fn preallocated_epub_resources_with_probe(
        probe: &mut dyn FnMut(&'static str),
    ) -> Result<EpubResources, String> {
        boot_probe(probe, "epub_resources:start");
        let page_window = Self::try_alloc_vec(Self::EPUB_PAGE_WINDOW.max(1))
            .map_err(|_| "Unable to allocate EPUB page window.".to_string())?;
        boot_probe(probe, "epub_resources:after_page_window");
        let page_bitmap = Some(EpubPageBitmap::try_new(480, 800)?);
        boot_probe(probe, "epub_resources:after_page_bitmap");
        let resources = EpubResources {
            page_window,
            page_bitmap,
        };
        boot_probe(probe, "epub_resources:ready");
        Ok(resources)
    }

    #[cfg(feature = "std")]
    fn ensure_epub_resources_ready(&mut self) -> Result<(), String> {
        if self.epub_resources.page_bitmap.is_none() {
            Self::log_epub_event("resource_alloc_begin");
            self.epub_resources = Self::preallocated_epub_resources()?;
            Self::log_epub_event("resource_alloc_ready");
        }
        Ok(())
    }

    #[cfg(feature = "std")]
    fn release_epub_session(&mut self) {
        if let Some(session) = self.epub_session.take() {
            self.epub_resources = session.into_resources();
        }
    }

    #[cfg(feature = "std")]
    fn release_non_reader_state(&mut self) {
        self.files = Vec::new();
        self.feed_sources = Vec::new();
    }

    fn ensure_feed_sources_loaded(&mut self) {
        if self.feed_sources.is_empty() {
            self.feed_sources = Self::build_feed_sources();
        }
    }

    const FONT_SIZE_OPTIONS: [&'static str; 3] = ["Small", "Medium", "Large"];
    const FONT_FAMILY_OPTIONS: [&'static str; 3] = ["Serif", "Sans", "Mono"];
    const AUTO_SLEEP_OPTIONS: [&'static str; 4] = ["5m", "10m", "15m", "Never"];
    const REFRESH_OPTIONS: [&'static str; 3] = ["Never", "Every Page", "Every Chapter"];
    const INVERT_COLORS_OPTIONS: [&'static str; 2] = ["Off", "On"];
    const SETTINGS_ITEM_COUNT: usize = 5;
    const TRANSFER_ITEMS: [&'static str; 3] = ["Edit AP SSID", "Edit AP Password", "Start/Restart"];
    const SETTING_KEY_FONT_SIZE: u8 = 1;
    const SETTING_KEY_FONT_FAMILY: u8 = 2;
    const SETTING_KEY_AUTO_SLEEP: u8 = 3;
    const SETTING_KEY_REFRESH: u8 = 4;
    const SETTING_KEY_INVERT_COLORS: u8 = 5;
    const SETTING_KEY_WIFI_ACTIVE: u8 = 240;
    const SETTING_KEY_WIFI_ENABLE_REQUEST: u8 = 241;
    const MAX_LIBRARY_SCAN_DEPTH: usize = 16;

    fn move_up(idx: &mut usize) {
        *idx = idx.saturating_sub(1);
    }

    fn move_down(idx: &mut usize, len: usize) {
        if *idx + 1 < len {
            *idx += 1;
        }
    }

    fn load_settings(&mut self, ctx: &mut Context<'_, DefaultTheme>) {
        self.font_size_idx = Self::load_setting_idx(
            ctx,
            Self::SETTING_KEY_FONT_SIZE,
            Self::FONT_SIZE_OPTIONS.len(),
            self.font_size_idx,
        );
        self.font_family_idx = Self::load_setting_idx(
            ctx,
            Self::SETTING_KEY_FONT_FAMILY,
            Self::FONT_FAMILY_OPTIONS.len(),
            self.font_family_idx,
        );
        self.auto_sleep_idx = Self::load_setting_idx(
            ctx,
            Self::SETTING_KEY_AUTO_SLEEP,
            Self::AUTO_SLEEP_OPTIONS.len(),
            self.auto_sleep_idx,
        );
        self.refresh_idx = Self::load_setting_idx(
            ctx,
            Self::SETTING_KEY_REFRESH,
            Self::REFRESH_OPTIONS.len(),
            self.refresh_idx,
        );
        self.invert_colors_idx = Self::load_setting_idx(
            ctx,
            Self::SETTING_KEY_INVERT_COLORS,
            Self::INVERT_COLORS_OPTIONS.len(),
            self.invert_colors_idx,
        );
    }

    fn load_setting_idx(
        ctx: &mut Context<'_, DefaultTheme>,
        key: u8,
        len: usize,
        default: usize,
    ) -> usize {
        let mut buf = [0u8; 1];
        let loaded = ctx.settings.load_raw(key, &mut buf);
        if loaded == 1 {
            let idx = buf[0] as usize;
            if idx < len {
                return idx;
            }
        }
        default
    }

    fn save_setting_idx(ctx: &mut Context<'_, DefaultTheme>, key: u8, idx: usize) {
        ctx.settings.save_raw(key, &[idx as u8]);
    }

    fn wifi_is_active(ctx: &mut Context<'_, DefaultTheme>) -> bool {
        let mut buf = [0u8; 1];
        ctx.settings
            .load_raw(Self::SETTING_KEY_WIFI_ACTIVE, &mut buf)
            == 1
            && buf[0] != 0
    }

    fn request_wifi_enable(ctx: &mut Context<'_, DefaultTheme>) {
        ctx.settings
            .save_raw(Self::SETTING_KEY_WIFI_ENABLE_REQUEST, &[1]);
    }

    fn settings_items(&self) -> Vec<String> {
        vec![
            format!("Font Size: {}", Self::FONT_SIZE_OPTIONS[self.font_size_idx]),
            format!(
                "Font Family: {}",
                Self::FONT_FAMILY_OPTIONS[self.font_family_idx]
            ),
            format!(
                "Auto Sleep: {}",
                Self::AUTO_SLEEP_OPTIONS[self.auto_sleep_idx]
            ),
            format!("Refresh: {}", Self::REFRESH_OPTIONS[self.refresh_idx]),
            format!(
                "Invert Colors: {}",
                Self::INVERT_COLORS_OPTIONS[self.invert_colors_idx]
            ),
        ]
    }

    fn cycle_setting(&mut self, ctx: &mut Context<'_, DefaultTheme>) {
        match self.settings_idx {
            0 => {
                self.font_size_idx = (self.font_size_idx + 1) % Self::FONT_SIZE_OPTIONS.len();
                Self::save_setting_idx(ctx, Self::SETTING_KEY_FONT_SIZE, self.font_size_idx);
            }
            1 => {
                self.font_family_idx = (self.font_family_idx + 1) % Self::FONT_FAMILY_OPTIONS.len();
                Self::save_setting_idx(ctx, Self::SETTING_KEY_FONT_FAMILY, self.font_family_idx);
            }
            2 => {
                self.auto_sleep_idx = (self.auto_sleep_idx + 1) % Self::AUTO_SLEEP_OPTIONS.len();
                Self::save_setting_idx(ctx, Self::SETTING_KEY_AUTO_SLEEP, self.auto_sleep_idx);
            }
            3 => {
                self.refresh_idx = (self.refresh_idx + 1) % Self::REFRESH_OPTIONS.len();
                Self::save_setting_idx(ctx, Self::SETTING_KEY_REFRESH, self.refresh_idx);
            }
            4 => {
                self.invert_colors_idx =
                    (self.invert_colors_idx + 1) % Self::INVERT_COLORS_OPTIONS.len();
                Self::save_setting_idx(
                    ctx,
                    Self::SETTING_KEY_INVERT_COLORS,
                    self.invert_colors_idx,
                );
            }
            _ => {}
        }
    }

    fn library_item_count(&self) -> usize {
        self.files.len().min(4) + 1
    }

    fn refresh_files(&mut self, ctx: &mut Context<'_, DefaultTheme>) {
        let mut entries = Vec::new();
        ctx.files.list("/", &mut |name| {
            if Self::is_supported_book(name) {
                entries.push(name.to_string());
            }
        });
        self.collect_books_recursive(ctx, &mut entries);

        entries.sort();
        entries.dedup();
        self.files = entries;
        self.library_idx = self
            .library_idx
            .min(self.library_item_count().saturating_sub(1));
        self.files_idx = self.files_idx.min(self.files.len().saturating_sub(1));
    }

    fn collect_books_recursive(
        &self,
        ctx: &mut Context<'_, DefaultTheme>,
        entries: &mut Vec<String>,
    ) {
        let mut stack = vec![("/books".to_string(), "books".to_string(), 0usize)];
        let mut visited_dirs: Vec<String> = Vec::new();

        while let Some((abs_dir, rel_dir, depth)) = stack.pop() {
            if depth > Self::MAX_LIBRARY_SCAN_DEPTH
                || visited_dirs.iter().any(|visited| visited == &abs_dir)
            {
                continue;
            }
            visited_dirs.push(abs_dir.clone());

            ctx.files.list(&abs_dir, &mut |name| {
                let abs_path = Self::join_path(&abs_dir, name);
                let rel_path = Self::join_path(&rel_dir, name);

                if Self::is_supported_book(name) {
                    entries.push(rel_path.clone());
                }
                if depth < Self::MAX_LIBRARY_SCAN_DEPTH && ctx.files.is_dir(&abs_path) == Some(true)
                {
                    stack.push((abs_path, rel_path, depth + 1));
                }
            });
        }
    }

    fn is_supported_book(name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        lower.ends_with(".epub")
            || lower.ends_with(".txt")
            || lower.ends_with(".md")
            || lower.ends_with(".epu")
    }

    fn join_path(base: &str, entry: &str) -> String {
        if base.is_empty() {
            return entry.to_string();
        }
        if base.ends_with('/') {
            return format!("{}{}", base, entry);
        }
        format!("{}/{}", base, entry)
    }

    fn library_item_label(&self, idx: usize) -> String {
        if idx >= self.files.len().min(4) {
            "File Transfer".to_string()
        } else {
            let name = &self.files[idx];
            if idx == 0 {
                format!("Continue: {}", name)
            } else {
                name.clone()
            }
        }
    }

    fn read_file_lines(
        &self,
        path: &str,
        ctx: &mut Context<'_, DefaultTheme>,
    ) -> Result<Vec<String>, FileStoreError> {
        let mut buf =
            Self::try_alloc_zeroed(self.text_read_buffer_bytes).map_err(|_| FileStoreError::Io)?;
        let bytes = ctx.files.read(path, &mut buf)?;
        let text = String::from_utf8_lossy(bytes);
        let mut lines = Vec::new();
        for line in text.lines() {
            lines.push(line.to_string());
        }
        if lines.is_empty() {
            lines.push("(empty file)".to_string());
        }
        Ok(Self::wrap_reader_lines(lines))
    }

    fn wrap_reader_lines(lines: Vec<String>) -> Vec<String> {
        let max_chars = 56usize;
        let mut out = Vec::new();
        for line in lines {
            Self::wrap_single_line(&line, max_chars, &mut out);
        }
        out
    }

    fn wrap_single_line(line: &str, max_chars: usize, out: &mut Vec<String>) {
        if line.is_empty() {
            out.push(String::new());
            return;
        }

        let mut current = String::new();
        let mut current_len = 0usize;

        for word in line.split_whitespace() {
            let word_len = word.chars().count();
            let sep = if current_len == 0 { 0 } else { 1 };
            if current_len + sep + word_len <= max_chars {
                if sep == 1 {
                    current.push(' ');
                }
                current.push_str(word);
                current_len += sep + word_len;
            } else {
                if !current.is_empty() {
                    out.push(current.clone());
                    current.clear();
                    current_len = 0;
                }
                if word_len <= max_chars {
                    current.push_str(word);
                    current_len = word_len;
                } else {
                    let mut chunk = String::new();
                    let mut chunk_len = 0usize;
                    for ch in word.chars() {
                        if chunk_len >= max_chars {
                            out.push(chunk.clone());
                            chunk.clear();
                            chunk_len = 0;
                        }
                        chunk.push(ch);
                        chunk_len += 1;
                    }
                    if !chunk.is_empty() {
                        current = chunk;
                        current_len = chunk_len;
                    }
                }
            }
        }

        if !current.is_empty() {
            out.push(current);
        }
    }

    fn epub_layout_margins(font_size_idx: usize) -> (i32, i32, i32, i32) {
        let side = match font_size_idx {
            0 => 8,
            1 => 10,
            2 => 12,
            _ => 10,
        };
        (side, side, 10, 20)
    }

    fn epub_line_spacing_px(auto_sleep_idx: usize) -> (i32, i32) {
        match auto_sleep_idx {
            0 => (1, 6),
            1 => (2, 8),
            2 => (3, 10),
            _ => (2, 8),
        }
    }

    fn epub_base_font_px(font_size_idx: usize) -> f32 {
        match font_size_idx {
            0 => 16.0,
            1 => 18.0,
            2 => 20.0,
            _ => 18.0,
        }
    }

    fn epub_forced_font_family(font_family_idx: usize) -> &'static str {
        match font_family_idx {
            0 => "serif",
            1 => "sans-serif",
            2 => "monospace",
            _ => "serif",
        }
    }

    #[cfg(feature = "std")]
    fn cacheable_epub_page(page: &RenderPage) -> CachedRenderPage {
        CachedRenderPage::from(page)
    }

    #[cfg(feature = "std")]
    fn cacheable_book_snapshot(book: &EpubSessionBook, source_path: &str) -> CachedBookSnapshot {
        let (title, author, language, chapter_count, chapters) = match book {
            #[cfg(not(target_os = "espidf"))]
            EpubSessionBook::Generic(inner) => (
                inner.title().to_string(),
                inner.author().to_string(),
                inner.language().to_string(),
                inner.chapter_count(),
                inner
                    .chapters()
                    .map(|chapter| CachedChapterEntry {
                        index: chapter.index,
                        href: chapter.href,
                        media_type: chapter.media_type,
                    })
                    .collect(),
            ),
            #[cfg(target_os = "espidf")]
            EpubSessionBook::Temp(inner) => (
                inner.title().to_string(),
                inner.author().to_string(),
                inner.language().to_string(),
                inner.chapter_count(),
                inner
                    .chapters()
                    .map(|chapter| CachedChapterEntry {
                        index: chapter.index,
                        href: chapter.href,
                        media_type: chapter.media_type,
                    })
                    .collect(),
            ),
        };
        CachedBookSnapshot::new(
            source_path.to_string(),
            title,
            author,
            language,
            chapter_count,
            chapters,
        )
    }

    #[cfg(feature = "std")]
    fn log_epub_event(event: &str) {
        #[cfg(target_os = "espidf")]
        {
            let free_heap = unsafe { sys::esp_get_free_heap_size() };
            let largest_8bit =
                unsafe { sys::heap_caps_get_largest_free_block(sys::MALLOC_CAP_8BIT) };
            log::info!(
                "[EPUB] {} free_heap={} largest_8bit={}",
                event,
                free_heap,
                largest_8bit
            );
        }
        #[cfg(not(target_os = "espidf"))]
        {
            log::info!("[EPUB] {}", event);
        }
    }

    #[cfg(feature = "std")]
    fn create_epub_engine(
        font_size_idx: usize,
        auto_sleep_idx: usize,
        display_width: i32,
        display_height: i32,
    ) -> RenderEngine {
        let mut opts = RenderEngineOptions::for_display(display_width, display_height);
        let (margin_left, margin_right, margin_top, margin_bottom) =
            Self::epub_layout_margins(font_size_idx);
        let (line_gap_px, paragraph_gap_px) = Self::epub_line_spacing_px(auto_sleep_idx);
        opts.layout.margin_left = margin_left;
        opts.layout.margin_right = margin_right;
        opts.layout.margin_top = margin_top;
        opts.layout.margin_bottom = margin_bottom;
        opts.layout.first_line_indent_px = 0;
        opts.layout.line_gap_px = line_gap_px;
        opts.layout.paragraph_gap_px = paragraph_gap_px;
        opts.layout.typography.justification.enabled = false;
        opts.layout.typography.justification.strategy = JustificationStrategy::AlignLeft;
        opts.layout.typography.hyphenation.soft_hyphen_policy = HyphenationMode::Discretionary;
        opts.prep = Self::epub_render_prep_options(font_size_idx);
        RenderEngine::new(opts)
    }

    #[cfg(feature = "std")]
    fn epub_render_prep_options(font_size_idx: usize) -> RenderPrepOptions {
        let hints = LayoutHints {
            base_font_size_px: Self::epub_base_font_px(font_size_idx),
            text_scale: 1.0,
            min_line_height: 1.05,
            max_line_height: 1.25,
            ..LayoutHints::default()
        };
        if cfg!(target_os = "espidf") {
            RenderPrepOptions {
                style: StyleConfig {
                    limits: StyleLimits {
                        max_selectors: 128,
                        max_css_bytes: 16 * 1024,
                        max_nesting: 8,
                    },
                    hints,
                },
                fonts: FontLimits {
                    max_faces: 2,
                    max_bytes_per_font: 48 * 1024,
                    max_total_font_bytes: 96 * 1024,
                },
                layout_hints: hints,
                memory: MemoryBudget {
                    max_entry_bytes: 64 * 1024,
                    max_css_bytes: 16 * 1024,
                    max_nav_bytes: 32 * 1024,
                    max_inline_style_bytes: 1024,
                    max_pages_in_memory: 1,
                },
            }
        } else {
            RenderPrepOptions {
                style: StyleConfig {
                    limits: StyleLimits::default(),
                    hints,
                },
                fonts: FontLimits::default(),
                layout_hints: hints,
                memory: MemoryBudget::default(),
            }
        }
    }

    #[cfg(feature = "std")]
    fn book_chapter_count(book: &EpubSessionBook) -> usize {
        match book {
            #[cfg(not(target_os = "espidf"))]
            EpubSessionBook::Generic(inner) => inner.chapter_count(),
            #[cfg(target_os = "espidf")]
            EpubSessionBook::Temp(inner) => inner.chapter_count(),
        }
    }

    #[cfg(feature = "std")]
    fn session_chapter_uncompressed_size(
        session: &mut EpubSession,
        chapter_idx: usize,
    ) -> Result<usize, epub_stream::EpubError> {
        match &mut session.book {
            #[cfg(not(target_os = "espidf"))]
            EpubSessionBook::Generic(inner) => inner.chapter_uncompressed_size(chapter_idx),
            #[cfg(target_os = "espidf")]
            EpubSessionBook::Temp(inner) => inner.chapter_uncompressed_size(chapter_idx),
        }
    }

    #[cfg(feature = "std")]
    fn session_page_cache(session: &EpubSession) -> Option<&EpubPageCache> {
        session.cache.as_ref()
    }

    #[cfg(feature = "std")]
    fn ensure_epub_chapter_capacity(
        session: &mut EpubSession,
        chapter_idx: usize,
    ) -> Result<Vec<u8>, String> {
        let Ok(required_bytes) = Self::session_chapter_uncompressed_size(session, chapter_idx)
        else {
            let fallback = Self::EPUB_MAX_CHAPTER_BUF_CAPACITY_BYTES.min(32 * 1024);
            Self::log_epub_event(&format!(
                "chapter_size_unknown chapter={} allocating_fallback={}",
                chapter_idx, fallback
            ));
            return Self::try_epub_chapter_buf(fallback);
        };
        Self::log_epub_event(&format!(
            "chapter_size chapter={} required={} cap={}",
            chapter_idx,
            required_bytes,
            Self::EPUB_MAX_CHAPTER_BUF_CAPACITY_BYTES
        ));
        if required_bytes > Self::EPUB_MAX_CHAPTER_BUF_CAPACITY_BYTES {
            return Err(format!(
                "Unable to stream EPUB chapter: required {} bytes exceeds chapter buffer cap {} bytes",
                required_bytes,
                Self::EPUB_MAX_CHAPTER_BUF_CAPACITY_BYTES
            ));
        }
        Self::try_epub_chapter_buf(required_bytes)
    }

    #[cfg(feature = "std")]
    fn load_epub_page(
        session: &mut EpubSession,
        chapter_idx: usize,
        page_idx: usize,
        cfg: EpubLoadConfig,
    ) -> Result<Option<(RenderPage, usize)>, String> {
        if let Some(cache) = Self::session_page_cache(session)
            && let Some(record) = cache.load_page(chapter_idx, page_idx)
        {
            Self::log_epub_event(&format!(
                "page_cache_hit chapter={} page={} total_pages={}",
                chapter_idx, page_idx, record.total_pages
            ));
            return Ok(Some((record.page.into(), record.total_pages.max(1))));
        }

        let mut chapter_buf = Self::ensure_epub_chapter_capacity(session, chapter_idx)?;
        let mut chapter_scratch = Self::try_embedded_scratch_buffers()?;
        let chapter_opts = ChapterEventsOptions {
            render: Self::epub_render_prep_options(cfg.font_size_idx),
            max_items: Self::EPUB_MAX_CHAPTER_EVENTS,
        };
        let config = RenderConfig::default()
            .with_forced_font_family(Self::epub_forced_font_family(session.font_family_idx))
            .with_page_range(page_idx..page_idx + 1);
        let mut render_session = session.engine.begin(chapter_idx, config);
        let mut target_page: Option<RenderPage> = None;
        let mut layout_error: Option<String> = None;
        let mut on_item = |item| {
            if layout_error.is_some() || target_page.is_some() {
                return Ok::<(), epub_stream::EpubError>(());
            }
            if let Err(err) = render_session.push(item) {
                layout_error = Some(err.to_string());
                return Ok::<(), epub_stream::EpubError>(());
            }
            render_session.drain_pages(|page| {
                if target_page.is_none() {
                    target_page = Some(page);
                }
            });
            Ok::<(), epub_stream::EpubError>(())
        };
        let stream_result = match &mut session.book {
            #[cfg(not(target_os = "espidf"))]
            EpubSessionBook::Generic(inner) => inner
                .chapter_events_with_scratch(
                    chapter_idx,
                    chapter_opts,
                    &mut chapter_buf,
                    &mut chapter_scratch,
                    &mut on_item,
                )
                .map(|_| ()),
            #[cfg(target_os = "espidf")]
            EpubSessionBook::Temp(inner) => inner
                .chapter_events_with_scratch(
                    chapter_idx,
                    chapter_opts,
                    &mut chapter_buf,
                    &mut chapter_scratch,
                    &mut on_item,
                )
                .map(|_| ()),
        };
        if let Err(err) = stream_result {
            return Err(format!("Unable to stream EPUB chapter: {}", err));
        }
        if let Some(err) = layout_error {
            return Err(format!("Unable to layout EPUB chapter: {}", err));
        }
        render_session
            .finish()
            .map_err(|e| format!("Unable to finalize EPUB chapter layout: {}", e))?;
        render_session.drain_pages(|page| {
            if target_page.is_none() {
                target_page = Some(page);
            }
        });

        let Some(page) = target_page else {
            return Ok(None);
        };
        let total_pages = page
            .metrics
            .chapter_page_count
            .unwrap_or(page_idx + 1)
            .max(1);
        let cached_page = Self::cacheable_epub_page(&page);
        if let Some(cache) = Self::session_page_cache(session) {
            let record =
                CachedPageRecord::new(chapter_idx, page_idx, total_pages, cached_page.clone());
            if let Some(bytes) = EpubPageCache::serialize_page(&record)
                && bytes.len() <= Self::EPUB_MAX_CACHE_PAGE_BYTES
            {
                cache.store_page_bytes(&record, &bytes);
            }
        }
        Self::log_epub_event(&format!(
            "page_cache_miss chapter={} page={} total_pages={}",
            chapter_idx, page_idx, total_pages
        ));
        Ok(Some((page, total_pages)))
    }

    #[cfg(feature = "std")]
    fn open_epub_session(
        path: &str,
        resources: &mut EpubResources,
        cfg: EpubLoadConfig,
        ctx: &mut Context<'_, DefaultTheme>,
    ) -> Result<Box<EpubSession>, String> {
        #[cfg(target_os = "espidf")]
        let book = {
            let options = EpubBookOptions {
                zip_limits: Some(ZipLimits::new(256 * 1024, 128)),
                validation_mode: ValidationMode::Lenient,
                max_nav_bytes: Some(32 * 1024),
                navigation_limits: NavigationLimits::embedded(),
                metadata_limits: MetadataLimits::embedded(),
            };
            let native_path = ctx
                .files
                .native_path(path)
                .ok_or_else(|| "Failed to resolve EPUB file path.".to_string())?;
            fs::create_dir_all("/sd/.tmp")
                .map_err(|err| format!("Failed to prepare EPUB temp dir: {}", err))?;
            EpubSessionBook::Temp(
                EpubBook::open_with_temp_storage(
                    native_path,
                    "/sd/.tmp",
                    OpenConfig {
                        options,
                        lazy_navigation: true,
                    },
                )
                .map_err(|err| format!("Failed to parse EPUB: {}", err))?,
            )
        };
        #[cfg(not(target_os = "espidf"))]
        let book = {
            let reader = ctx
                .files
                .open_read_seek(path)
                .map_err(|_| "Failed to open EPUB file.".to_string())?;
            EpubSessionBook::Generic(
                EpubBook::builder()
                    .from_reader(reader)
                    .map_err(|err| format!("Failed to parse EPUB: {}", err))?,
            )
        };
        if Self::book_chapter_count(&book) == 0 {
            return Err("EPUB has no chapters.".to_string());
        }
        let engine = Self::create_epub_engine(
            cfg.font_size_idx,
            cfg.auto_sleep_idx,
            cfg.display_width,
            cfg.display_height,
        );
        let cache = ctx.files.native_path(path).map(|native_path| {
            let cache = EpubPageCache::for_native_book_path(
                &native_path,
                engine.pagination_profile_id(),
                cfg.font_family_idx,
            );
            let snapshot_event = if let Some(snapshot) = cache.load_book_snapshot() {
                format!(
                    "book_snapshot_cache_hit chapters={} title={}",
                    snapshot.chapter_count, snapshot.title
                )
            } else {
                "book_snapshot_cache_miss".to_string()
            };
            Self::log_epub_event(&snapshot_event);
            cache.store_book_snapshot(&Self::cacheable_book_snapshot(&book, &native_path));
            cache
        });
        Ok(Box::new(EpubSession {
            book,
            engine,
            font_family_idx: cfg.font_family_idx,
            resources: core::mem::take(resources),
            reader: EpubReaderState::default(),
            cache,
        }))
    }

    #[cfg(feature = "std")]
    fn load_epub_chapter(
        session: &mut EpubSession,
        chapter_idx: usize,
        start_page: usize,
        max_pages: usize,
        cfg: EpubLoadConfig,
    ) -> Result<(usize, usize, Vec<RenderPage>), String> {
        let chapter_count = Self::book_chapter_count(&session.book);
        if chapter_count == 0 {
            return Err("EPUB has no chapters.".to_string());
        }
        if chapter_idx >= chapter_count {
            return Err("Chapter out of range.".to_string());
        }
        let mut pages: Vec<RenderPage> = Vec::with_capacity(max_pages.max(1));
        let mut total_pages = 1usize;
        for page in start_page..start_page.saturating_add(max_pages) {
            let Some((page_view, chapter_total_pages)) =
                Self::load_epub_page(session, chapter_idx, page, cfg)?
            else {
                break;
            };
            total_pages = chapter_total_pages.max(1);
            pages.push(page_view);
        }
        Ok((chapter_count, total_pages, pages))
    }

    fn try_alloc_zeroed(len: usize) -> Result<Vec<u8>, ()> {
        let mut buf = Vec::new();
        buf.try_reserve_exact(len).map_err(|_| ())?;
        buf.resize(len, 0);
        Ok(buf)
    }

    #[cfg(feature = "std")]
    fn try_alloc_vec<T>(capacity: usize) -> Result<Vec<T>, ()> {
        let mut buf = Vec::new();
        buf.try_reserve_exact(capacity).map_err(|_| ())?;
        Ok(buf)
    }

    #[cfg(feature = "std")]
    fn try_alloc_string(capacity: usize) -> Result<String, ()> {
        let mut buf = String::new();
        buf.try_reserve_exact(capacity).map_err(|_| ())?;
        Ok(buf)
    }

    #[cfg(feature = "std")]
    fn try_embedded_scratch_buffers() -> Result<ScratchBuffers, String> {
        Ok(ScratchBuffers {
            read_buf: Self::try_alloc_vec(8192)
                .map_err(|_| "Unable to allocate EPUB read scratch buffer.".to_string())?,
            xml_buf: Self::try_alloc_vec(4096)
                .map_err(|_| "Unable to allocate EPUB XML scratch buffer.".to_string())?,
            text_buf: Self::try_alloc_string(2048)
                .map_err(|_| "Unable to allocate EPUB text scratch buffer.".to_string())?,
        })
    }

    #[cfg(feature = "std")]
    fn try_epub_chapter_buf(capacity: usize) -> Result<Vec<u8>, String> {
        Self::try_alloc_vec(capacity).map_err(|_| {
            format!(
                "Unable to allocate {} bytes for EPUB chapter buffer",
                capacity
            )
        })
    }

    #[cfg(feature = "std")]
    fn load_epub_chapter_in_direction(
        session: &mut EpubSession,
        chapter_idx: usize,
        step: isize,
        cfg: EpubLoadConfig,
    ) -> Option<EpubPageWindow> {
        let (chapter_count, total_pages, initial_pages) =
            Self::load_epub_chapter(session, chapter_idx, 0, Self::EPUB_PAGE_WINDOW, cfg).ok()?;
        if !initial_pages.is_empty() {
            return Some(EpubPageWindow {
                chapter_count,
                chapter_idx,
                total_pages,
                page_window_start: 0,
                pages: initial_pages,
            });
        }

        let mut idx = chapter_idx as isize + step;
        while idx >= 0 && (idx as usize) < chapter_count {
            if let Ok((count, total_pages, pages)) =
                Self::load_epub_chapter(session, idx as usize, 0, Self::EPUB_PAGE_WINDOW, cfg)
                && !pages.is_empty()
            {
                return Some(EpubPageWindow {
                    chapter_count: count,
                    chapter_idx: idx as usize,
                    total_pages,
                    page_window_start: 0,
                    pages,
                });
            }
            idx += step;
        }
        None
    }

    #[cfg(feature = "std")]
    fn reload_epub_page_window(
        session: &mut EpubSession,
        chapter_idx: usize,
        page_idx: usize,
        cfg: EpubLoadConfig,
    ) -> Option<EpubPageWindow> {
        let start_page = page_idx.saturating_sub(Self::EPUB_PAGE_WINDOW / 2);
        let (chapter_count, total_pages, pages) = Self::load_epub_chapter(
            session,
            chapter_idx,
            start_page,
            Self::EPUB_PAGE_WINDOW,
            cfg,
        )
        .ok()?;
        if pages.is_empty() {
            None
        } else {
            Some(EpubPageWindow {
                chapter_count,
                chapter_idx,
                total_pages,
                page_window_start: start_page,
                pages,
            })
        }
    }

    #[cfg(feature = "std")]
    fn streamed_image_options() -> StreamedImageOptions {
        if cfg!(target_os = "espidf") {
            StreamedImageOptions {
                max_image_bytes: 256 * 1024,
                max_decoded_bytes: 384 * 1024,
                decode_png: true,
            }
        } else {
            StreamedImageOptions {
                max_image_bytes: 256 * 1024,
                max_decoded_bytes: 384 * 1024,
                decode_png: true,
            }
        }
    }

    #[cfg(feature = "std")]
    fn refresh_epub_page_bitmap(session: &mut EpubSession) -> Result<(), String> {
        let Some(bitmap) = session.resources.page_bitmap.as_mut() else {
            return Ok(());
        };
        bitmap.clear();

        let local_idx = session
            .reader
            .page_idx
            .saturating_sub(session.reader.page_window_start);
        let Some(page) = session.resources.page_window.get(local_idx) else {
            return Ok(());
        };
        let mut framebuffer =
            PackedBinaryFrameBuffer::new(bitmap.width, bitmap.height, bitmap.bytes_mut())
                .map_err(|err| format!("Unable to prepare EPUB framebuffer: {:?}", err))?;
        let renderer = EgRenderer::with_backend(EgRenderConfig::default(), MonoFontBackend);
        let diagnostics = match &mut session.book {
            #[cfg(not(target_os = "espidf"))]
            EpubSessionBook::Generic(inner) => renderer.render_page_with_streamed_images(
                inner,
                page,
                &mut framebuffer,
                Self::streamed_image_options(),
            ),
            #[cfg(target_os = "espidf")]
            EpubSessionBook::Temp(inner) => renderer.render_page_with_streamed_images(
                inner,
                page,
                &mut framebuffer,
                Self::streamed_image_options(),
            ),
        }
        .map_err(|_| "Unable to rasterize EPUB page.".to_string())?;
        let generation = bitmap.next_generation();
        Self::log_epub_event(&format!(
            "page_bitmap_rendered chapter={} page={} generation={} attempted_images={} decoded_png={} decoded_jpeg={} decoded_gif={} decoded_webp={} decode_failures={} unsupported_sources={} resource_errors={}",
            session.reader.chapter_idx,
            session.reader.page_idx,
            generation,
            diagnostics.attempted,
            diagnostics.decoded_png,
            diagnostics.decoded_jpeg,
            diagnostics.decoded_gif,
            diagnostics.decoded_webp,
            diagnostics.decode_failures,
            diagnostics.unsupported_sources,
            diagnostics.resource_errors
        ));
        Ok(())
    }

    #[inline(never)]
    fn open_epub_in_reader(&mut self, path: &str, ctx: &mut Context<'_, DefaultTheme>) {
        #[cfg(feature = "std")]
        {
            self.release_epub_session();
            self.release_non_reader_state();
            if let Err(message) = self.ensure_epub_resources_ready() {
                self.modal = ModalState::Reader {
                    title: path.to_string(),
                    lines: vec![message],
                    scroll: 0,
                };
                return;
            }
            Self::log_epub_event(&format!("open_begin path={}", path));
        }
        let cfg = EpubLoadConfig {
            font_size_idx: self.font_size_idx,
            auto_sleep_idx: self.auto_sleep_idx,
            font_family_idx: self.font_family_idx,
            display_width: self.epub_display_width,
            display_height: self.epub_display_height,
        };
        #[cfg(not(feature = "std"))]
        {
            let _ = (path, cfg, ctx);
            self.modal = ModalState::Reader {
                title: "EPUB".to_string(),
                lines: vec!["EPUB parsing requires std feature.".to_string()],
                scroll: 0,
            };
            return;
        }
        #[cfg(feature = "std")]
        let mut session = match Self::open_epub_session(path, &mut self.epub_resources, cfg, ctx) {
            Ok(session) => session,
            Err(message) => {
                self.modal = ModalState::Reader {
                    title: path.to_string(),
                    lines: vec![message],
                    scroll: 0,
                };
                return;
            }
        };
        #[cfg(feature = "std")]
        match Self::load_epub_chapter_in_direction(&mut session, 0, 1, cfg) {
            Some(window) => {
                session.apply_reader_window(window, 0);
                if let Err(err) = Self::refresh_epub_page_bitmap(&mut session) {
                    self.modal = ModalState::Reader {
                        title: path.to_string(),
                        lines: vec![err],
                        scroll: 0,
                    };
                    return;
                }
                Self::log_epub_event(&format!(
                    "open_ready chapter={} chapter_count={} total_pages={} window_start={}",
                    session.reader.chapter_idx,
                    session.reader.chapter_count,
                    session.reader.total_pages,
                    session.reader.page_window_start
                ));
                self.epub_session = Some(session);
                self.modal = ModalState::EpubReader;
            }
            None => {
                #[cfg(feature = "std")]
                {
                    self.release_epub_session();
                }
                self.modal = ModalState::Reader {
                    title: path.to_string(),
                    lines: vec!["No readable text produced by renderer.".to_string()],
                    scroll: 0,
                };
            }
        }
    }

    fn is_epub_path(path: &str) -> bool {
        let lower = path.as_bytes();
        lower.ends_with(b".epub")
            || lower.ends_with(b".EPUB")
            || lower.ends_with(b".ePub")
            || lower.ends_with(b".epu")
            || lower.ends_with(b".EPU")
    }

    #[inline(never)]
    fn open_file_in_reader(&mut self, path: &str, ctx: &mut Context<'_, DefaultTheme>) {
        #[cfg(target_os = "espidf")]
        eprintln!("[EINKED][OPEN] file path={}", path);
        let owned_path = path.to_string();
        if Self::is_epub_path(&owned_path) {
            #[cfg(target_os = "espidf")]
            eprintln!("[EINKED][OPEN] detected EPUB");
            self.open_epub_in_reader(&owned_path, ctx);
            return;
        }
        #[cfg(feature = "std")]
        {
            self.release_epub_session();
        }
        match self.read_file_lines(&owned_path, ctx) {
            Ok(lines) => {
                self.modal = ModalState::Reader {
                    title: owned_path,
                    lines,
                    scroll: 0,
                };
            }
            Err(_) => {
                #[cfg(target_os = "espidf")]
                eprintln!("[EINKED][OPEN] text file read failed");
                self.modal = ModalState::Reader {
                    title: owned_path,
                    lines: vec![
                        "Failed to open file.".to_string(),
                        "Check storage backend and path.".to_string(),
                    ],
                    scroll: 0,
                };
            }
        }
    }

    fn feed_entries_for_source(&self, source_idx: usize) -> Vec<FeedEntry> {
        let Some((name, url, source_type)) = self.feed_sources.get(source_idx) else {
            return vec![FeedEntry {
                title: "Invalid feed source".to_string(),
                url: None,
                summary: Some("Selected feed source was not found.".to_string()),
            }];
        };
        let result =
            self.feed_client
                .borrow_mut()
                .fetch_entries(name.as_str(), url.as_str(), *source_type);
        match result {
            Ok(entries) if !entries.is_empty() => entries
                .into_iter()
                .map(|entry| FeedEntry {
                    title: entry.title,
                    url: entry.url,
                    summary: entry.summary,
                })
                .collect(),
            Ok(_) => vec![FeedEntry {
                title: "No entries available".to_string(),
                url: None,
                summary: Some("Feed returned no entries.".to_string()),
            }],
            Err(message) => vec![FeedEntry {
                title: "Feed load failed".to_string(),
                url: None,
                summary: Some(message),
            }],
        }
    }

    fn show_feed_entries_modal(&mut self, source_idx: usize) {
        if let Some((title, _, _)) = self.feed_sources.get(source_idx) {
            let entries = self.feed_entries_for_source(source_idx);
            self.modal = ModalState::FeedEntries {
                source_idx,
                title: title.clone(),
                entries,
                selected_idx: 0,
            };
        }
    }

    fn show_feed_offline_modal(
        &mut self,
        _ctx: &mut Context<'_, DefaultTheme>,
        source_idx: usize,
        message: String,
        requires_wifi: bool,
        request_wifi: bool,
    ) {
        if let Some((title, _, _)) = self.feed_sources.get(source_idx) {
            self.modal = ModalState::FeedOffline {
                source_idx,
                title: title.clone(),
                message,
                requested_enable: request_wifi,
                requires_wifi,
            };
        }
    }

    fn draw_list_str(
        ui_ctx: &mut dyn Ui<DefaultTheme>,
        y_start: i16,
        selected: usize,
        items: &[String],
    ) {
        for (idx, item) in items.iter().enumerate() {
            let prefix = if idx == selected { "> " } else { "  " };
            let row = Self::truncate_single_line(item, 50);
            ui_ctx.draw_text_at(
                Point {
                    x: 18,
                    y: y_start + (idx as i16 * 22),
                },
                &format!("{}{}", prefix, row),
            );
        }
    }

    fn draw_feed_entries(
        ui_ctx: &mut dyn Ui<DefaultTheme>,
        y_start: i16,
        selected: usize,
        items: &[FeedEntry],
    ) {
        for (idx, item) in items.iter().enumerate() {
            let prefix = if idx == selected { "> " } else { "  " };
            let row = Self::truncate_single_line(&item.title, 50);
            ui_ctx.draw_text_at(
                Point {
                    x: 18,
                    y: y_start + (idx as i16 * 22),
                },
                &format!("{}{}", prefix, row),
            );
        }
    }

    fn truncate_single_line(text: &str, max_chars: usize) -> String {
        let count = text.chars().count();
        if count <= max_chars {
            return text.to_string();
        }
        if max_chars <= 3 {
            return "...".to_string();
        }
        let mut out = String::new();
        for ch in text.chars().take(max_chars - 3) {
            out.push(ch);
        }
        out.push_str("...");
        out
    }

    fn draw_reader_lines(ui_ctx: &mut dyn Ui<DefaultTheme>, lines: &[String], scroll: usize) {
        let start_y = 70i16;
        let visible = 30usize;
        let end = (scroll + visible).min(lines.len());
        for (idx, line) in lines[scroll..end].iter().enumerate() {
            ui_ctx.draw_text_at(
                Point {
                    x: 16,
                    y: start_y + (idx as i16 * 20),
                },
                line,
            );
        }
    }

    fn draw_epub_page(
        &self,
        ui_ctx: &mut dyn Ui<DefaultTheme>,
        bitmap: &EpubPageBitmap,
        page_idx: usize,
        total_pages: usize,
        chapter_idx: usize,
        chapter_count: usize,
    ) {
        let footer_y = 794i16;
        ui_ctx.draw_image(
            Rect {
                x: 0,
                y: 0,
                width: bitmap.width as u16,
                height: bitmap.height as u16,
            },
            bitmap.bytes(),
            ImageFormat::Mono1bpp,
            bitmap.generation,
        );

        ui_ctx.draw_text_at(
            Point { x: 8, y: footer_y },
            &format!(
                "ch {}/{}  p {}/{}",
                chapter_idx + 1,
                chapter_count.max(1),
                page_idx + 1,
                total_pages.max(1)
            ),
        );
    }

    fn render_library(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        let mut items = Vec::new();
        for i in 0..self.library_item_count() {
            items.push(self.library_item_label(i));
        }
        ui_ctx.draw_text_at(Point { x: 16, y: 26 }, "Library");
        ui_ctx.draw_line(
            Point { x: 16, y: 34 },
            Point { x: 464, y: 34 },
            Color::Black,
            1,
        );
        Self::draw_list_str(ui_ctx, 66, self.library_idx, &items);
    }

    fn render_files(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        ui_ctx.draw_text_at(Point { x: 16, y: 26 }, "Files");
        ui_ctx.draw_line(
            Point { x: 16, y: 34 },
            Point { x: 464, y: 34 },
            Color::Black,
            1,
        );
        Self::draw_list_str(ui_ctx, 66, self.files_idx, &self.files);
    }

    fn render_feed(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        let mut items = Vec::new();
        for (name, _, ty) in &self.feed_sources {
            let label = match ty {
                FeedType::Opds => format!("{} (OPDS)", name),
                FeedType::Rss => format!("{} (RSS)", name),
            };
            items.push(label);
        }
        ui_ctx.draw_text_at(Point { x: 16, y: 26 }, "Feed");
        ui_ctx.draw_line(
            Point { x: 16, y: 34 },
            Point { x: 464, y: 34 },
            Color::Black,
            1,
        );
        Self::draw_list_str(ui_ctx, 66, self.feed_idx, &items);
    }

    fn render_settings(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        let items = self.settings_items();
        ui_ctx.draw_text_at(Point { x: 16, y: 26 }, "Settings");
        ui_ctx.draw_line(
            Point { x: 16, y: 34 },
            Point { x: 464, y: 34 },
            Color::Black,
            1,
        );
        Self::draw_list_str(ui_ctx, 66, self.settings_idx, &items);
    }

    fn render_transfer_screen(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        let items: Vec<String> = Self::TRANSFER_ITEMS
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        ui_ctx.draw_text_at(Point { x: 16, y: 26 }, "File Transfer");
        ui_ctx.draw_line(
            Point { x: 16, y: 34 },
            Point { x: 464, y: 34 },
            Color::Black,
            1,
        );
        ui_ctx.draw_text_at(Point { x: 18, y: 60 }, "Status: Running");
        ui_ctx.draw_text_at(Point { x: 18, y: 84 }, "Mode: Hotspot");
        ui_ctx.draw_text_at(Point { x: 18, y: 108 }, "SSID: Xteink-X4");
        ui_ctx.draw_text_at(Point { x: 18, y: 132 }, "Password: xteink2026");
        ui_ctx.draw_text_at(Point { x: 18, y: 156 }, "http://192.168.4.1");
        Self::draw_list_str(ui_ctx, 210, self.transfer_menu_idx, &items);
    }

    fn render_reader(
        &self,
        ui_ctx: &mut dyn Ui<DefaultTheme>,
        title: &str,
        lines: &[String],
        scroll: usize,
    ) {
        ui_ctx.draw_text_at(Point { x: 16, y: 26 }, "Reader");
        ui_ctx.draw_line(
            Point { x: 16, y: 34 },
            Point { x: 464, y: 34 },
            Color::Black,
            1,
        );
        ui_ctx.draw_text_at(Point { x: 16, y: 54 }, title);
        Self::draw_reader_lines(ui_ctx, lines, scroll);
    }

    fn render_epub_reader(
        &self,
        ui_ctx: &mut dyn Ui<DefaultTheme>,
        chapter_idx: usize,
        chapter_count: usize,
        page_idx: usize,
        total_pages: usize,
        bitmap: &EpubPageBitmap,
    ) {
        self.draw_epub_page(
            ui_ctx,
            bitmap,
            page_idx,
            total_pages,
            chapter_idx,
            chapter_count,
        );
    }

    fn render_bottom_bar(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        if matches!(self.modal, ModalState::EpubReader) {
            return;
        }
        ui_ctx.draw_line(
            Point { x: 0, y: 772 },
            Point { x: 479, y: 772 },
            Color::Black,
            1,
        );
        let left_hint = match self.modal {
            ModalState::Transfer => "Back: Exit transfer",
            ModalState::Reader { .. } => "Back: Close",
            ModalState::FeedEntries { .. } => "Back: Sources",
            ModalState::FeedItem { .. } => "Back: Entries",
            ModalState::FeedArticle { .. } => "Back: Entry",
            ModalState::FeedOffline { .. } => "Back: Feed",
            ModalState::None => match self.tab {
                MainTab::Library => "Back: Refresh library",
                MainTab::Files => "Back: Up",
                MainTab::Feed => "Back: Sources",
                MainTab::Settings => "Back: No-op",
            },
            ModalState::EpubReader => "",
        };
        ui_ctx.draw_text_at(Point { x: 14, y: 792 }, left_hint);
        ui_ctx.draw_text_at(Point { x: 210, y: 792 }, self.tab.dot_label());
        ui_ctx.draw_text_at(Point { x: 432, y: 792 }, "100%");
    }
}

impl Activity<DefaultTheme> for HomeActivity {
    fn on_enter(&mut self, ctx: &mut Context<'_, DefaultTheme>) {
        self.load_settings(ctx);
        self.refresh_files(ctx);
    }

    fn on_input(
        &mut self,
        event: InputEvent,
        ctx: &mut Context<'_, DefaultTheme>,
    ) -> Transition<DefaultTheme> {
        if matches!(self.tab, MainTab::Feed) {
            self.ensure_feed_sources_loaded();
        }
        let epub_nav_cfg = EpubLoadConfig {
            font_size_idx: self.font_size_idx,
            auto_sleep_idx: self.auto_sleep_idx,
            font_family_idx: self.font_family_idx,
            display_width: self.epub_display_width,
            display_height: self.epub_display_height,
        };
        match &mut self.modal {
            ModalState::Transfer => {
                return match event {
                    InputEvent::Press(Button::Back) => {
                        self.modal = ModalState::None;
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Up) | InputEvent::Press(Button::Aux1) => {
                        Self::move_up(&mut self.transfer_menu_idx);
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Down) | InputEvent::Press(Button::Aux2) => {
                        Self::move_down(&mut self.transfer_menu_idx, Self::TRANSFER_ITEMS.len());
                        Transition::Stay
                    }
                    _ => Transition::Stay,
                };
            }
            ModalState::Reader { scroll, lines, .. } => {
                return match event {
                    InputEvent::Press(Button::Back) => {
                        self.modal = ModalState::None;
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Up) | InputEvent::Press(Button::Aux1) => {
                        Self::move_up(scroll);
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Down) | InputEvent::Press(Button::Aux2) => {
                        Self::move_down(scroll, lines.len());
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Confirm) => {
                        *scroll = (*scroll + 10).min(lines.len().saturating_sub(1));
                        Transition::Stay
                    }
                    _ => Transition::Stay,
                };
            }
            ModalState::EpubReader => {
                return match event {
                    InputEvent::Press(Button::Back) => {
                        #[cfg(feature = "std")]
                        {
                            self.release_epub_session();
                        }
                        self.modal = ModalState::None;
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Left) => {
                        #[cfg(feature = "std")]
                        if let Some(session) = self.epub_session.as_mut() {
                            if session.reader.page_idx > 0 {
                                let page_idx = session.reader.page_idx.saturating_sub(1);
                                session.reader.page_idx = page_idx;
                                if page_idx < session.reader.page_window_start
                                    && let Some(window) = Self::reload_epub_page_window(
                                        session,
                                        session.reader.chapter_idx,
                                        page_idx,
                                        epub_nav_cfg,
                                    )
                                {
                                    session.apply_reader_window(window, page_idx);
                                }
                                let _ = Self::refresh_epub_page_bitmap(session);
                                return Transition::Stay;
                            }

                            if session.reader.chapter_idx > 0 {
                                let next_chapter = session.reader.chapter_idx - 1;
                                if let Some(window) = Self::load_epub_chapter_in_direction(
                                    session,
                                    next_chapter,
                                    -1,
                                    epub_nav_cfg,
                                ) {
                                    let target_page = window.total_pages.saturating_sub(1);
                                    if let Some(final_window) = Self::reload_epub_page_window(
                                        session,
                                        window.chapter_idx,
                                        target_page,
                                        epub_nav_cfg,
                                    ) {
                                        session.apply_reader_window(final_window, target_page);
                                    } else {
                                        session.reader.chapter_idx = window.chapter_idx;
                                        session.reader.chapter_count = window.chapter_count;
                                        session.reader.total_pages = window.total_pages;
                                        session.reader.page_window_start = 0;
                                        session.reader.page_idx = 0;
                                        session.clear_page_window();
                                    }
                                    let _ = Self::refresh_epub_page_bitmap(session);
                                }
                            }
                        }
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Right) => {
                        #[cfg(feature = "std")]
                        if let Some(session) = self.epub_session.as_mut() {
                            if session.reader.page_idx + 1 < session.reader.total_pages {
                                let page_idx = session.reader.page_idx + 1;
                                session.reader.page_idx = page_idx;
                                let window_end =
                                    session.reader.page_window_start + session.page_window_len();
                                if page_idx >= window_end
                                    && let Some(window) = Self::reload_epub_page_window(
                                        session,
                                        session.reader.chapter_idx,
                                        page_idx,
                                        epub_nav_cfg,
                                    )
                                {
                                    session.apply_reader_window(window, page_idx);
                                }
                                let _ = Self::refresh_epub_page_bitmap(session);
                                return Transition::Stay;
                            }

                            if session.reader.chapter_idx + 1 < session.reader.chapter_count {
                                let next_chapter = session.reader.chapter_idx + 1;
                                if let Some(window) = Self::load_epub_chapter_in_direction(
                                    session,
                                    next_chapter,
                                    1,
                                    epub_nav_cfg,
                                ) {
                                    session.apply_reader_window(window, 0);
                                    let _ = Self::refresh_epub_page_bitmap(session);
                                }
                            }
                        }
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Aux1) => {
                        #[cfg(feature = "std")]
                        if let Some(session) = self.epub_session.as_mut()
                            && session.reader.chapter_idx > 0
                        {
                            let next_chapter = session.reader.chapter_idx - 1;
                            if let Some(window) = Self::load_epub_chapter_in_direction(
                                session,
                                next_chapter,
                                -1,
                                epub_nav_cfg,
                            ) {
                                session.apply_reader_window(window, 0);
                                let _ = Self::refresh_epub_page_bitmap(session);
                            }
                        }
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Aux2) => {
                        #[cfg(feature = "std")]
                        if let Some(session) = self.epub_session.as_mut()
                            && session.reader.chapter_idx + 1 < session.reader.chapter_count
                        {
                            let next_chapter = session.reader.chapter_idx + 1;
                            if let Some(window) = Self::load_epub_chapter_in_direction(
                                session,
                                next_chapter,
                                1,
                                epub_nav_cfg,
                            ) {
                                session.apply_reader_window(window, 0);
                                let _ = Self::refresh_epub_page_bitmap(session);
                            }
                        }
                        Transition::Stay
                    }
                    _ => Transition::Stay,
                };
            }
            ModalState::FeedEntries {
                source_idx,
                entries,
                selected_idx,
                ..
            } => {
                return match event {
                    InputEvent::Press(Button::Back) => {
                        self.modal = ModalState::None;
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Up) | InputEvent::Press(Button::Aux1) => {
                        Self::move_up(selected_idx);
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Down) | InputEvent::Press(Button::Aux2) => {
                        Self::move_down(selected_idx, entries.len());
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Confirm) => {
                        let item_idx = *selected_idx;
                        let item = entries[item_idx].clone();
                        let title = item.title.clone();
                        let mut lines = Vec::new();
                        lines.push(format!("Entry: {}", item.title));
                        if let Some(summary) = item.summary {
                            for line in summary.lines().take(24) {
                                lines.push(line.to_string());
                            }
                        }
                        if let Some(url) = item.url {
                            lines.push(String::new());
                            lines.push(format!("URL: {}", url));
                        }
                        lines.push(String::new());
                        lines.push("Back returns to entries.".to_string());
                        lines = Self::wrap_reader_lines(lines);
                        let restore_entries = entries.clone();
                        self.modal = ModalState::FeedItem {
                            source_idx: *source_idx,
                            title,
                            entries: restore_entries,
                            item_idx,
                            lines,
                            scroll: 0,
                        };
                        Transition::Stay
                    }
                    _ => Transition::Stay,
                };
            }
            ModalState::FeedItem {
                source_idx,
                title,
                entries,
                item_idx,
                scroll,
                lines,
                ..
            } => {
                return match event {
                    InputEvent::Press(Button::Back) => {
                        self.modal = ModalState::FeedEntries {
                            source_idx: *source_idx,
                            title: self.feed_sources[*source_idx].0.clone(),
                            entries: entries.clone(),
                            selected_idx: *item_idx,
                        };
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Confirm) => {
                        if let Some(url) = entries.get(*item_idx).and_then(|e| e.url.clone()) {
                            let article_lines =
                                match self.feed_client.borrow_mut().fetch_article_lines(&url) {
                                    Ok(lines) => Self::wrap_reader_lines(lines),
                                    Err(message) => Self::wrap_reader_lines(vec![
                                        message,
                                        format!("URL: {}", url),
                                    ]),
                                };
                            self.modal = ModalState::FeedArticle {
                                source_idx: *source_idx,
                                title: title.clone(),
                                entries: entries.clone(),
                                item_idx: *item_idx,
                                lines: article_lines,
                                scroll: 0,
                            };
                        } else {
                            lines.push(String::new());
                            lines.push("No article URL available for this entry.".to_string());
                        }
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Up) | InputEvent::Press(Button::Aux1) => {
                        Self::move_up(scroll);
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Down) | InputEvent::Press(Button::Aux2) => {
                        Self::move_down(scroll, lines.len());
                        Transition::Stay
                    }
                    _ => Transition::Stay,
                };
            }
            ModalState::FeedArticle {
                source_idx,
                title,
                entries,
                item_idx,
                lines,
                scroll,
            } => {
                return match event {
                    InputEvent::Press(Button::Back) => {
                        let mut item_lines = Vec::new();
                        if let Some(item) = entries.get(*item_idx).cloned() {
                            item_lines.push(format!("Entry: {}", item.title));
                            if let Some(summary) = item.summary {
                                for line in summary.lines().take(24) {
                                    item_lines.push(line.to_string());
                                }
                            }
                            if let Some(url) = item.url {
                                item_lines.push(String::new());
                                item_lines.push(format!("URL: {}", url));
                            }
                            item_lines.push(String::new());
                            item_lines.push("Back returns to entries.".to_string());
                        }
                        self.modal = ModalState::FeedItem {
                            source_idx: *source_idx,
                            title: title.clone(),
                            entries: entries.clone(),
                            item_idx: *item_idx,
                            lines: Self::wrap_reader_lines(item_lines),
                            scroll: 0,
                        };
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Up) | InputEvent::Press(Button::Aux1) => {
                        Self::move_up(scroll);
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Down) | InputEvent::Press(Button::Aux2) => {
                        Self::move_down(scroll, lines.len());
                        Transition::Stay
                    }
                    _ => Transition::Stay,
                };
            }
            ModalState::FeedOffline {
                source_idx: _,
                title: _,
                message: _,
                requires_wifi,
                requested_enable,
                ..
            } => {
                return match event {
                    InputEvent::Press(Button::Back) => {
                        self.modal = ModalState::None;
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Confirm) => {
                        if *requires_wifi && !*requested_enable {
                            Self::request_wifi_enable(ctx);
                            *requested_enable = true;
                        } else if !*requires_wifi {
                            self.modal = ModalState::None;
                        }
                        Transition::Stay
                    }
                    _ => Transition::Stay,
                };
            }
            ModalState::None => {}
        }

        match event {
            InputEvent::Press(Button::Left) => {
                self.tab = self.tab.prev();
                Transition::Stay
            }
            InputEvent::Press(Button::Right) => {
                self.tab = self.tab.next();
                Transition::Stay
            }
            InputEvent::Press(Button::Up) | InputEvent::Press(Button::Aux1) => {
                match self.tab {
                    MainTab::Library => Self::move_up(&mut self.library_idx),
                    MainTab::Files => Self::move_up(&mut self.files_idx),
                    MainTab::Feed => Self::move_up(&mut self.feed_idx),
                    MainTab::Settings => Self::move_up(&mut self.settings_idx),
                }
                Transition::Stay
            }
            InputEvent::Press(Button::Down) | InputEvent::Press(Button::Aux2) => {
                let library_count = self.library_item_count();
                match self.tab {
                    MainTab::Library => Self::move_down(&mut self.library_idx, library_count),
                    MainTab::Files => Self::move_down(&mut self.files_idx, self.files.len()),
                    MainTab::Feed => Self::move_down(&mut self.feed_idx, self.feed_sources.len()),
                    MainTab::Settings => {
                        Self::move_down(&mut self.settings_idx, Self::SETTINGS_ITEM_COUNT)
                    }
                }
                Transition::Stay
            }
            InputEvent::Press(Button::Back) => {
                if self.tab == MainTab::Library {
                    self.refresh_files(ctx);
                }
                Transition::Stay
            }
            InputEvent::Press(Button::Confirm) => match self.tab {
                MainTab::Library if self.library_idx + 1 == self.library_item_count() => {
                    self.modal = ModalState::Transfer;
                    self.transfer_menu_idx = 0;
                    Transition::Stay
                }
                MainTab::Library => {
                    let file_idx = self.library_idx.min(self.files.len().saturating_sub(1));
                    if let Some(path) = self.files.get(file_idx).cloned() {
                        self.open_file_in_reader(&path, ctx);
                    }
                    Transition::Stay
                }
                MainTab::Files => {
                    if let Some(path) = self.files.get(self.files_idx).cloned() {
                        self.open_file_in_reader(&path, ctx);
                    }
                    Transition::Stay
                }
                MainTab::Feed => {
                    let source_idx = self.feed_idx.min(self.feed_sources.len().saturating_sub(1));
                    if !Self::wifi_is_active(ctx) {
                        self.show_feed_offline_modal(
                            ctx,
                            source_idx,
                            "Wi-Fi is OFFLINE. Feeds require an active Wi-Fi connection."
                                .to_string(),
                            true,
                            false,
                        );
                        return Transition::Stay;
                    }
                    self.show_feed_entries_modal(source_idx);
                    Transition::Stay
                }
                MainTab::Settings => {
                    self.cycle_setting(ctx);
                    Transition::Stay
                }
            },
            _ => Transition::Stay,
        }
    }

    fn on_idle(&mut self, ctx: &mut Context<'_, DefaultTheme>) -> Transition<DefaultTheme> {
        let pending_feed_source = match &self.modal {
            ModalState::FeedOffline {
                source_idx,
                requires_wifi: true,
                ..
            } => Some(*source_idx),
            _ => None,
        };
        if let Some(source_idx) = pending_feed_source
            && Self::wifi_is_active(ctx)
        {
            self.show_feed_entries_modal(source_idx);
        }
        Transition::Stay
    }

    fn render(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        ui_ctx.fill_rect(
            Rect {
                x: 0,
                y: 0,
                width: 480,
                height: 800,
            },
            Color::White,
        );

        match &self.modal {
            ModalState::Transfer => self.render_transfer_screen(ui_ctx),
            ModalState::Reader {
                title,
                lines,
                scroll,
            } => self.render_reader(ui_ctx, title, lines, *scroll),
            ModalState::EpubReader => {
                if let Some(session) = self.epub_session.as_ref()
                    && let Some(bitmap) = session.resources.page_bitmap.as_ref()
                {
                    self.render_epub_reader(
                        ui_ctx,
                        session.reader.chapter_idx,
                        session.reader.chapter_count,
                        session.reader.page_idx,
                        session.reader.total_pages,
                        bitmap,
                    );
                }
            }
            ModalState::FeedEntries {
                title,
                entries,
                selected_idx,
                ..
            } => {
                ui_ctx.draw_text_at(Point { x: 16, y: 26 }, "Feed Entries");
                ui_ctx.draw_line(
                    Point { x: 16, y: 34 },
                    Point { x: 464, y: 34 },
                    Color::Black,
                    1,
                );
                ui_ctx.draw_text_at(Point { x: 18, y: 56 }, title);
                Self::draw_feed_entries(ui_ctx, 88, *selected_idx, entries);
            }
            ModalState::FeedItem {
                title,
                lines,
                scroll,
                ..
            } => self.render_reader(ui_ctx, title, lines, *scroll),
            ModalState::FeedArticle {
                title,
                lines,
                scroll,
                ..
            } => self.render_reader(ui_ctx, title, lines, *scroll),
            ModalState::FeedOffline {
                title,
                message,
                requested_enable,
                requires_wifi,
                ..
            } => {
                let mut lines = vec![
                    format!("Feed source: {}", title),
                    String::new(),
                    message.clone(),
                    String::new(),
                ];
                if *requires_wifi {
                    if *requested_enable {
                        lines.push(
                            "Wi-Fi enable requested. Waiting for the connection...".to_string(),
                        );
                    } else {
                        lines.push("Press Confirm to enable Wi-Fi now.".to_string());
                    }
                    lines
                        .push("The feed will open automatically once Wi-Fi is active.".to_string());
                } else {
                    lines.push("Press Confirm to recheck feed availability.".to_string());
                }
                lines.push(String::new());
                lines.push("Press Back to cancel.".to_string());
                self.render_reader(ui_ctx, "Feed Network Required", &lines, 0);
            }
            ModalState::None => match self.tab {
                MainTab::Library => self.render_library(ui_ctx),
                MainTab::Files => self.render_files(ui_ctx),
                MainTab::Feed => self.render_feed(ui_ctx),
                MainTab::Settings => self.render_settings(ui_ctx),
            },
        }
        self.render_bottom_bar(ui_ctx);
    }

    fn refresh_hint(&self) -> RefreshHint {
        RefreshHint::Fast
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyUi;

    impl Ui<DefaultTheme> for DummyUi {}

    fn test_ctx<'a>(
        settings: &'a mut NoopSettings,
        files: &'a mut NoopFiles,
    ) -> Context<'a, DefaultTheme> {
        Context {
            theme: &DefaultTheme,
            screen: DeviceConfig::xteink_x4().screen,
            settings,
            files,
        }
    }

    #[test]
    fn confirm_on_feed_opens_entries_and_back_returns() {
        let mut act = HomeActivity::new_with_device_and_feed(
            DeviceConfig::xteink_x4(),
            Rc::new(RefCell::new(Box::new(NoopFeedClient))),
        );
        let mut settings = NoopSettings::default();
        let mut files = NoopFiles;
        settings.save_raw(HomeActivity::SETTING_KEY_WIFI_ACTIVE, &[1]);
        let mut ctx = test_ctx(&mut settings, &mut files);
        act.on_enter(&mut ctx);

        let _ = act.on_input(InputEvent::Press(Button::Right), &mut ctx);
        let _ = act.on_input(InputEvent::Press(Button::Right), &mut ctx);
        let _ = act.on_input(InputEvent::Press(Button::Confirm), &mut ctx);
        assert!(matches!(act.modal, ModalState::FeedEntries { .. }));

        let _ = act.on_input(InputEvent::Press(Button::Confirm), &mut ctx);
        assert!(matches!(act.modal, ModalState::FeedItem { .. }));

        let _ = act.on_input(InputEvent::Press(Button::Back), &mut ctx);
        assert!(matches!(act.modal, ModalState::FeedEntries { .. }));

        let _ = act.on_input(InputEvent::Press(Button::Back), &mut ctx);
        assert!(matches!(act.modal, ModalState::None));

        let mut ui = DummyUi;
        act.render(&mut ui);
    }

    #[test]
    fn confirm_on_empty_library_opens_transfer_modal() {
        let mut act = HomeActivity::new_with_device_and_feed(
            DeviceConfig::xteink_x4(),
            Rc::new(RefCell::new(Box::new(NoopFeedClient))),
        );
        let mut settings = NoopSettings::default();
        let mut files = NoopFiles;
        let mut ctx = test_ctx(&mut settings, &mut files);
        act.on_enter(&mut ctx);

        let _ = act.on_input(InputEvent::Press(Button::Confirm), &mut ctx);
        assert!(matches!(act.modal, ModalState::Transfer));
        let _ = act.on_input(InputEvent::Press(Button::Back), &mut ctx);
        assert!(matches!(act.modal, ModalState::None));

        let _ = act.on_input(InputEvent::Press(Button::Right), &mut ctx);
        let _ = act.on_input(InputEvent::Press(Button::Confirm), &mut ctx);
        assert!(matches!(act.modal, ModalState::None));
    }
}

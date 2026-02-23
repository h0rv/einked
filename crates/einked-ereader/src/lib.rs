#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use einked::activity_stack::{Activity, ActivityStack, Context, Transition, Ui};
use einked::core::{Color, DefaultTheme, Point, Rect};
use einked::dsl::UiDsl;
use einked::input::{Button, InputEvent};
use einked::pipeline::FramePipeline;
use einked::refresh::RefreshHint;
use einked::render_ir::DrawCmd;
use einked::storage::{FileStore, FileStoreError, SettingsStore};
use einked::ui::runtime::UiRuntime;
#[cfg(feature = "std")]
use std::io::Cursor;

#[cfg(feature = "std")]
use epub_stream::EpubBook;
#[cfg(feature = "std")]
use epub_stream_render::{
    DrawCommand as EpubDrawCommand, HyphenationMode, JustificationStrategy, RenderConfig,
    RenderEngine, RenderEngineOptions,
};

pub mod embedded_fonts;
pub mod feed;
pub mod feed_browser;

pub use embedded_fonts::{
    BOOKERLY_BOLD, BOOKERLY_BOLD_ITALIC, BOOKERLY_ITALIC, BOOKERLY_REGULAR, BOOKERLY_SET,
    EmbeddedFont,
};
pub use feed::{
    FeedSource, FeedType, JINA_READER_BASE, OpdsCatalog, OpdsEntry, OpdsLink,
    PRELOADED_OPDS_SOURCES, PRELOADED_RSS_SOURCES, all_preloaded_sources, get_reader_url,
};
pub use feed_browser::{BrowserState, FeedBrowserActivity};

pub trait FrameSink {
    fn render_and_flush(&mut self, cmds: &[DrawCmd<'static>], hint: RefreshHint) -> bool;
}

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
        }
    }
}

pub struct EreaderRuntime {
    stack: ActivityStack<DefaultTheme, 8>,
    pipeline: FramePipeline<512, 512>,
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
        mut settings: Box<dyn SettingsStore>,
        mut files: Box<dyn FileStore>,
    ) -> Self {
        let mut stack = ActivityStack::new();
        let theme = DefaultTheme;
        let mut ctx = Context {
            theme: &theme,
            screen: config.screen,
            settings: settings.as_mut(),
            files: files.as_mut(),
        };
        let _ = stack.push_root(Box::new(HomeActivity::new()), &mut ctx);

        let mut pipeline = FramePipeline::new(config.partial_refresh_limit);
        pipeline.set_viewport_width(config.screen.width);

        Self {
            stack,
            pipeline,
            theme,
            settings,
            files,
            config,
        }
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
    runtime: UiRuntime<'a, 512>,
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
}

struct NoopSettings {
    slots: [u8; 32],
}

impl Default for NoopSettings {
    fn default() -> Self {
        Self {
            slots: [u8::MAX; 32],
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
    EpubReader {
        path: String,
        chapter_idx: usize,
        chapter_count: usize,
        pages: Vec<Vec<String>>,
        page_idx: usize,
    },
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
    min_pages: usize,
    min_total_lines: usize,
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
    modal: ModalState,
}

impl HomeActivity {
    fn new() -> Self {
        let mut feed_sources = Vec::new();
        for (name, url, ty) in all_preloaded_sources() {
            feed_sources.push((name.to_string(), url.to_string(), ty));
        }
        Self {
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
            modal: ModalState::None,
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

        let mut collect_from = |base: &str, prefix: &str| {
            ctx.files.list(base, &mut |name| {
                let lower = name.to_ascii_lowercase();
                if lower.ends_with(".epub")
                    || lower.ends_with(".txt")
                    || lower.ends_with(".md")
                    || lower.ends_with(".epu")
                {
                    entries.push(format!("{}{}", prefix, name));
                }
            });
        };

        collect_from("/", "");
        collect_from("/books", "books/");

        entries.sort();
        entries.dedup();
        self.files = entries;
        self.library_idx = self
            .library_idx
            .min(self.library_item_count().saturating_sub(1));
        self.files_idx = self.files_idx.min(self.files.len().saturating_sub(1));
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
        let buf_len = if cfg!(target_os = "espidf") {
            512 * 1024
        } else {
            4 * 1024 * 1024
        };
        let mut buf = vec![0u8; buf_len];
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

    fn is_epub_boilerplate_line(line: &str) -> bool {
        let lower = line.to_ascii_lowercase();
        lower.contains("project gutenberg")
            || lower.contains("gutenberg.org")
            || lower.contains("*** start")
            || lower.contains("*** end")
            || lower.starts_with("title:")
            || lower.starts_with("author:")
            || lower.starts_with("release date:")
            || lower.starts_with("most recently updated:")
            || lower.starts_with("language:")
            || lower.starts_with("credits:")
    }

    fn epub_line_height(&self) -> i16 {
        match self.font_size_idx {
            0 => 14,
            1 => 16,
            2 => 18,
            _ => 16,
        }
    }

    fn epub_max_chars(&self) -> usize {
        match self.font_size_idx {
            0 => 72,
            1 => 64,
            2 => 56,
            _ => 64,
        }
    }

    fn epub_rows_per_page(font_size_idx: usize) -> usize {
        match font_size_idx {
            0 => 52,
            1 => 46,
            2 => 40,
            _ => 46,
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
    fn create_epub_engine(font_size_idx: usize, auto_sleep_idx: usize) -> RenderEngine {
        let mut opts = RenderEngineOptions::for_display(480, 800);
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
        opts.prep.layout_hints.base_font_size_px = Self::epub_base_font_px(font_size_idx);
        opts.prep.layout_hints.text_scale = 1.0;
        opts.prep.layout_hints.min_line_height = 1.05;
        opts.prep.layout_hints.max_line_height = 1.25;
        opts.prep.style.hints = opts.prep.layout_hints;
        RenderEngine::new(opts)
    }

    #[cfg(feature = "std")]
    fn parse_epub_chapter_pages(
        bytes: &[u8],
        chapter_idx: usize,
        cfg: EpubLoadConfig,
    ) -> Result<(usize, Vec<Vec<String>>), String> {
        let cursor = Cursor::new(bytes.to_vec());
        let mut book = match EpubBook::builder().from_reader(cursor) {
            Ok(book) => book,
            Err(err) => {
                return Err(format!("Failed to parse EPUB: {}", err));
            }
        };

        let chapter_count = book.chapter_count();
        if chapter_count == 0 {
            return Err("EPUB has no chapters.".to_string());
        }
        if chapter_idx >= chapter_count {
            return Err("Chapter out of range.".to_string());
        }

        let engine = Self::create_epub_engine(cfg.font_size_idx, cfg.auto_sleep_idx);
        let config = RenderConfig::default()
            .with_forced_font_family(Self::epub_forced_font_family(cfg.font_family_idx));
        let pages = match engine.prepare_chapter_with_config_collect(&mut book, chapter_idx, config)
        {
            Ok(pages) => pages,
            Err(err) => {
                return Err(format!("EPUB render failed: {}", err));
            }
        };
        let mut lines_all: Vec<String> = Vec::new();
        for page in pages.iter() {
            for cmd in &page.content_commands {
                if let EpubDrawCommand::Text(text) = cmd {
                    let trimmed = text.text.trim();
                    if trimmed.is_empty() || Self::is_epub_boilerplate_line(trimmed) {
                        continue;
                    }
                    let max_chars = match cfg.font_size_idx {
                        0 => 72,
                        1 => 64,
                        2 => 56,
                        _ => 64,
                    };
                    Self::wrap_single_line(trimmed, max_chars, &mut lines_all);
                }
            }
        }
        let mut out_pages: Vec<Vec<String>> = Vec::new();
        let rows_per_page = Self::epub_rows_per_page(cfg.font_size_idx).max(1);
        let mut cursor = 0usize;
        while cursor < lines_all.len() {
            let end = (cursor + rows_per_page).min(lines_all.len());
            out_pages.push(lines_all[cursor..end].to_vec());
            cursor = end;
        }
        Ok((chapter_count, out_pages))
    }

    #[cfg(not(feature = "std"))]
    fn parse_epub_chapter_pages(
        _bytes: &[u8],
        _chapter_idx: usize,
        _cfg: EpubLoadConfig,
    ) -> Result<(usize, Vec<Vec<String>>), String> {
        Err("EPUB parsing requires std feature.".to_string())
    }

    fn read_file_bytes<'a>(
        path: &str,
        ctx: &mut Context<'_, DefaultTheme>,
        buf: &'a mut [u8],
    ) -> Result<&'a [u8], FileStoreError> {
        ctx.files.read(path, buf)
    }

    fn load_epub_chapter(
        path: &str,
        chapter_idx: usize,
        cfg: EpubLoadConfig,
        ctx: &mut Context<'_, DefaultTheme>,
    ) -> Result<(usize, Vec<Vec<String>>), String> {
        let buf_len = if cfg!(target_os = "espidf") {
            512 * 1024
        } else {
            4 * 1024 * 1024
        };
        let mut buf = vec![0u8; buf_len];
        let bytes = Self::read_file_bytes(path, ctx, &mut buf)
            .map_err(|_| "Failed to read EPUB file.".to_string())?;
        Self::parse_epub_chapter_pages(bytes, chapter_idx, cfg)
    }

    fn load_epub_chapter_in_direction(
        path: &str,
        chapter_idx: usize,
        step: isize,
        cfg: EpubLoadConfig,
        ctx: &mut Context<'_, DefaultTheme>,
    ) -> Option<(usize, usize, Vec<Vec<String>>)> {
        let (chapter_count, initial_pages) =
            Self::load_epub_chapter(path, chapter_idx, cfg, ctx).ok()?;
        let initial_total_lines = initial_pages.iter().map(|page| page.len()).sum::<usize>();
        if initial_pages.len() >= cfg.min_pages.max(1) && initial_total_lines >= cfg.min_total_lines
        {
            return Some((chapter_count, chapter_idx, initial_pages));
        }

        let mut idx = chapter_idx as isize + step;
        while idx >= 0 && (idx as usize) < chapter_count {
            if let Ok((count, pages)) = Self::load_epub_chapter(path, idx as usize, cfg, ctx) {
                let total_lines = pages.iter().map(|page| page.len()).sum::<usize>();
                if pages.len() >= cfg.min_pages.max(1) && total_lines >= cfg.min_total_lines {
                    return Some((count, idx as usize, pages));
                }
            }
            idx += step;
        }
        None
    }

    fn open_epub_in_reader(&mut self, path: &str, ctx: &mut Context<'_, DefaultTheme>) {
        let cfg = EpubLoadConfig {
            font_size_idx: self.font_size_idx,
            auto_sleep_idx: self.auto_sleep_idx,
            font_family_idx: self.font_family_idx,
            min_pages: 2,
            min_total_lines: Self::epub_rows_per_page(self.font_size_idx),
        };
        match Self::load_epub_chapter_in_direction(path, 0, 1, cfg, ctx) {
            Some((chapter_count, chapter_idx, pages)) => {
                self.modal = ModalState::EpubReader {
                    path: path.to_string(),
                    chapter_idx,
                    chapter_count,
                    pages,
                    page_idx: 0,
                };
            }
            None => {
                self.modal = ModalState::Reader {
                    title: path.to_string(),
                    lines: vec!["No readable text produced by renderer.".to_string()],
                    scroll: 0,
                };
            }
        }
    }

    fn open_file_in_reader(&mut self, path: &str, ctx: &mut Context<'_, DefaultTheme>) {
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".epub") || lower.ends_with(".epu") {
            self.open_epub_in_reader(path, ctx);
            return;
        }
        match self.read_file_lines(path, ctx) {
            Ok(lines) => {
                self.modal = ModalState::Reader {
                    title: path.to_string(),
                    lines,
                    scroll: 0,
                };
            }
            Err(_) => {
                self.modal = ModalState::Reader {
                    title: path.to_string(),
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
        self.fetch_live_feed_entries(source_idx).unwrap_or_else(|| {
            vec![FeedEntry {
                title: "No entries available".to_string(),
                url: None,
                summary: Some("Feed load failed or returned no entries.".to_string()),
            }]
        })
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

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    fn fetch_live_feed_entries(&self, source_idx: usize) -> Option<Vec<FeedEntry>> {
        let (_, url, _) = self.feed_sources.get(source_idx)?;
        let base_url = url::Url::parse(url).ok();
        let response = ureq::get(url).call().ok()?;
        let mut body = response.into_body();
        let bytes = body.read_to_vec().ok()?;

        let parsed = feed_rs::parser::parse(&bytes[..]).ok()?;
        let mut entries = Vec::new();
        for entry in parsed.entries.iter().take(32) {
            entries.push(FeedEntry {
                title: entry
                    .title
                    .as_ref()
                    .map(|t| t.content.clone())
                    .unwrap_or_else(|| "Untitled".to_string()),
                url: entry.links.first().map(|l| {
                    if let Some(base) = &base_url
                        && let Ok(joined) = base.join(&l.href)
                    {
                        return joined.to_string();
                    }
                    l.href.clone()
                }),
                summary: entry.summary.as_ref().map(|s| s.content.clone()),
            });
        }
        if entries.is_empty() {
            None
        } else {
            Some(entries)
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    fn fetch_live_feed_entries(&self, _source_idx: usize) -> Option<Vec<FeedEntry>> {
        None
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    fn fetch_article_lines(url: &str) -> Result<Vec<String>, String> {
        let reader_url = get_reader_url(url);
        let response = ureq::get(&reader_url)
            .call()
            .map_err(|_| "Failed to fetch article.".to_string())?;
        let mut body = response.into_body();
        let bytes = body
            .read_to_vec()
            .map_err(|_| "Failed reading article body.".to_string())?;
        let text = String::from_utf8_lossy(&bytes);
        let mut lines = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                lines.push(String::new());
                continue;
            }
            Self::wrap_single_line(trimmed, 56, &mut lines);
        }
        if lines.is_empty() {
            return Err("Article had no readable text.".to_string());
        }
        Ok(lines)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    fn fetch_article_lines(_url: &str) -> Result<Vec<String>, String> {
        Err("Article rendering requires host networking.".to_string())
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

    fn draw_epub_lines(
        &self,
        ui_ctx: &mut dyn Ui<DefaultTheme>,
        lines: &[String],
        page_idx: usize,
        total_pages: usize,
        chapter_idx: usize,
        chapter_count: usize,
    ) {
        let line_height = self.epub_line_height();
        let start_y = 12i16;
        let footer_y = 794i16;
        let max_rows = ((footer_y - start_y) / line_height).max(1) as usize;
        let max_chars = self.epub_max_chars();
        let mut cursor = 0usize;
        let mut row = 0usize;

        while row < max_rows && cursor < lines.len() {
            let line = Self::truncate_single_line(&lines[cursor], max_chars);
            ui_ctx.draw_text_at(
                Point {
                    x: 8,
                    y: start_y + (row as i16 * line_height),
                },
                &line,
            );
            row += 1;
            cursor += 1;
        }

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
        pages: &[Vec<String>],
        page_idx: usize,
    ) {
        if let Some(lines) = pages.get(page_idx) {
            self.draw_epub_lines(
                ui_ctx,
                lines,
                page_idx,
                pages.len(),
                chapter_idx,
                chapter_count,
            );
        }
    }

    fn render_bottom_bar(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        if matches!(self.modal, ModalState::EpubReader { .. }) {
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
            ModalState::None => match self.tab {
                MainTab::Library => "Back: Refresh library",
                MainTab::Files => "Back: Up",
                MainTab::Feed => "Back: Sources",
                MainTab::Settings => "Back: No-op",
            },
            ModalState::EpubReader { .. } => "",
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
        let epub_nav_cfg = EpubLoadConfig {
            font_size_idx: self.font_size_idx,
            auto_sleep_idx: self.auto_sleep_idx,
            font_family_idx: self.font_family_idx,
            min_pages: 1,
            min_total_lines: 1,
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
            ModalState::EpubReader {
                path,
                chapter_idx,
                chapter_count,
                pages,
                page_idx,
                ..
            } => {
                return match event {
                    InputEvent::Press(Button::Back) => {
                        self.modal = ModalState::None;
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Left) => {
                        if *page_idx > 0 {
                            *page_idx -= 1;
                            return Transition::Stay;
                        }
                        if *chapter_idx > 0 {
                            let next_chapter = *chapter_idx - 1;
                            if let Some((next_count, resolved_chapter, next_pages)) =
                                Self::load_epub_chapter_in_direction(
                                    path,
                                    next_chapter,
                                    -1,
                                    epub_nav_cfg,
                                    ctx,
                                )
                            {
                                *chapter_count = next_count;
                                *chapter_idx = resolved_chapter;
                                *page_idx = next_pages.len().saturating_sub(1);
                                *pages = next_pages;
                            }
                        }
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Right) => {
                        if *page_idx + 1 < pages.len() {
                            *page_idx += 1;
                            return Transition::Stay;
                        }
                        if *chapter_idx + 1 < *chapter_count {
                            let next_chapter = *chapter_idx + 1;
                            if let Some((next_count, resolved_chapter, next_pages)) =
                                Self::load_epub_chapter_in_direction(
                                    path,
                                    next_chapter,
                                    1,
                                    epub_nav_cfg,
                                    ctx,
                                )
                            {
                                *chapter_count = next_count;
                                *chapter_idx = resolved_chapter;
                                *page_idx = 0;
                                *pages = next_pages;
                            }
                        }
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Aux1) => {
                        if *chapter_idx > 0 {
                            let next_chapter = *chapter_idx - 1;
                            if let Some((next_count, resolved_chapter, next_pages)) =
                                Self::load_epub_chapter_in_direction(
                                    path,
                                    next_chapter,
                                    -1,
                                    epub_nav_cfg,
                                    ctx,
                                )
                            {
                                *chapter_count = next_count;
                                *chapter_idx = resolved_chapter;
                                *page_idx = 0;
                                *pages = next_pages;
                            }
                        }
                        Transition::Stay
                    }
                    InputEvent::Press(Button::Aux2) => {
                        if *chapter_idx + 1 < *chapter_count {
                            let next_chapter = *chapter_idx + 1;
                            if let Some((next_count, resolved_chapter, next_pages)) =
                                Self::load_epub_chapter_in_direction(
                                    path,
                                    next_chapter,
                                    1,
                                    epub_nav_cfg,
                                    ctx,
                                )
                            {
                                *chapter_count = next_count;
                                *chapter_idx = resolved_chapter;
                                *page_idx = 0;
                                *pages = next_pages;
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
                            let article_lines = match Self::fetch_article_lines(&url) {
                                Ok(lines) => lines,
                                Err(message) => vec![message, format!("URL: {}", url)],
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
                    let entries = self.feed_entries_for_source(source_idx);
                    let title = self.feed_sources[source_idx].0.clone();
                    self.modal = ModalState::FeedEntries {
                        source_idx,
                        title,
                        entries,
                        selected_idx: 0,
                    };
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
            ModalState::EpubReader {
                chapter_idx,
                chapter_count,
                pages,
                page_idx,
                ..
            } => self.render_epub_reader(ui_ctx, *chapter_idx, *chapter_count, pages, *page_idx),
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
        let mut act = HomeActivity::new();
        let mut settings = NoopSettings::default();
        let mut files = NoopFiles;
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
        let mut act = HomeActivity::new();
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

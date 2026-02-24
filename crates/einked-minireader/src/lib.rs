#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use einked::activity_stack::{Activity, ActivityStack, Context, Transition, Ui};
use einked::core::{Color, DefaultTheme, Point, Rect};
use einked::input::{Button, InputEvent};
use einked::pipeline::FramePipeline;
use einked::refresh::RefreshHint;
use einked::render_ir::DrawCmd;
use einked::storage::{FileStore, SettingsStore};
use einked::ui::runtime::UiRuntime;

#[cfg(feature = "std")]
use epub_stream::book::{ChapterEventsOptions, OpenConfig};
#[cfg(feature = "std")]
use epub_stream::{EpubBook, EpubBookOptions, ScratchBuffers, ValidationMode, ZipLimits};
#[cfg(feature = "std")]
use epub_stream_render::{DrawCommand, RenderConfig, RenderEngine, RenderEngineOptions, RenderPage};

pub trait FrameSink {
    fn render_and_flush(&mut self, cmds: &[DrawCmd<'static>], hint: RefreshHint) -> bool;
}

#[cfg(target_os = "espidf")]
const FRAME_CMD_CAPACITY: usize = 32;
#[cfg(not(target_os = "espidf"))]
const FRAME_CMD_CAPACITY: usize = 256;

#[cfg(target_os = "espidf")]
const FRAME_PREV_CAPACITY: usize = 8;
#[cfg(not(target_os = "espidf"))]
const FRAME_PREV_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiniReaderConfig {
    pub screen: Rect,
    pub partial_refresh_limit: u8,
    pub library_root: &'static str,
    pub battery_setting_key: u8,
}

impl MiniReaderConfig {
    pub const fn xteink_x4() -> Self {
        Self {
            screen: Rect {
                x: 0,
                y: 0,
                width: 480,
                height: 800,
            },
            partial_refresh_limit: 8,
            library_root: "/books",
            battery_setting_key: 242,
        }
    }
}

pub struct MiniReaderRuntime {
    stack: ActivityStack<DefaultTheme, 4>,
    pipeline: FramePipeline<FRAME_CMD_CAPACITY, FRAME_PREV_CAPACITY>,
    theme: DefaultTheme,
    settings: Box<dyn SettingsStore>,
    files: Box<dyn FileStore>,
    config: MiniReaderConfig,
}

impl MiniReaderRuntime {
    pub fn new(config: MiniReaderConfig) -> Self {
        Self::with_backends(
            config,
            Box::new(NoopSettings::default()),
            Box::new(NoopFiles),
        )
    }

    pub fn with_backends(
        config: MiniReaderConfig,
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
        let _ = stack.push_root(
            Box::new(HomeActivity::new(
                config.library_root,
                config.battery_setting_key,
                config.screen,
            )),
            &mut ctx,
        );

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
}

#[derive(Clone)]
struct BookEntry {
    path: String,
    name: String,
}

enum Mode {
    Library,
    Reader {
        title: String,
        lines: Vec<String>,
        chapter_idx: usize,
        chapter_count: usize,
        page_idx: usize,
        total_pages: usize,
    },
    Message(String),
}

#[cfg(feature = "std")]
struct ReaderSession {
    book: EpubBook<Box<dyn einked::storage::ReadSeek>>,
    engine: RenderEngine,
    chapter_buf: Vec<u8>,
    scratch: ScratchBuffers,
    chapter_idx: usize,
    page_idx: usize,
    chapter_count: usize,
}

struct HomeActivity {
    screen: Rect,
    battery_key: u8,
    library_root: String,
    books: Vec<BookEntry>,
    selected: usize,
    battery_pct: u8,
    mode: Mode,
    #[cfg(feature = "std")]
    session: Option<ReaderSession>,
}

impl HomeActivity {
    const MAX_SCAN_DEPTH: usize = 12;
    const KEY_BACK: &'static str = "Back: Refresh";
    const KEY_SELECT: &'static str = "Confirm: Open";

    #[cfg(feature = "std")]
    const MAX_ZIP_ENTRY_BYTES: usize = 512 * 1024;
    #[cfg(feature = "std")]
    const MAX_NAV_BYTES: usize = 32 * 1024;
    #[cfg(feature = "std")]
    const MAX_CHAPTER_EVENTS: usize = 8_192;
    #[cfg(feature = "std")]
    const CHAPTER_BUF_INIT: usize = 8 * 1024;
    #[cfg(feature = "std")]
    const CHAPTER_BUF_MAX: usize = 48 * 1024;
    #[cfg(feature = "std")]
    const CHAPTER_GROW_RETRIES: usize = 4;

    fn new(library_root: &str, battery_key: u8, screen: Rect) -> Self {
        Self {
            screen,
            battery_key,
            library_root: library_root.to_string(),
            books: Vec::new(),
            selected: 0,
            battery_pct: 100,
            mode: Mode::Library,
            #[cfg(feature = "std")]
            session: None,
        }
    }

    fn refresh_library(&mut self, ctx: &mut Context<'_, DefaultTheme>) {
        self.books.clear();
        self.selected = 0;
        self.battery_pct = Self::read_battery_percent(ctx, self.battery_key);
        Self::scan_dir(
            ctx.files,
            self.library_root.as_str(),
            Self::MAX_SCAN_DEPTH,
            &mut self.books,
        );
        self.books.sort_by(|a, b| a.name.cmp(&b.name));
    }

    fn read_battery_percent(ctx: &mut Context<'_, DefaultTheme>, key: u8) -> u8 {
        let mut slot = [0u8; 1];
        if ctx.settings.load_raw(key, &mut slot) == 1 {
            slot[0].min(100)
        } else {
            100
        }
    }

    fn is_epub(path: &str) -> bool {
        path.to_ascii_lowercase().ends_with(".epub")
    }

    fn scan_dir(files: &dyn FileStore, root: &str, depth: usize, out: &mut Vec<BookEntry>) {
        if depth == 0 {
            return;
        }
        let mut names = Vec::new();
        files.list(root, &mut |name| names.push(name.to_string()));
        names.sort();
        for name in names {
            let path = if root == "/" {
                format!("/{}", name)
            } else {
                format!("{}/{}", root.trim_end_matches('/'), name)
            };
            if files.is_dir(path.as_str()) == Some(true) {
                Self::scan_dir(files, path.as_str(), depth - 1, out);
                continue;
            }
            if !Self::is_epub(path.as_str()) {
                continue;
            }
            out.push(BookEntry { path, name });
        }
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn move_down(&mut self) {
        if self.selected + 1 < self.books.len() {
            self.selected += 1;
        }
    }

    #[cfg(feature = "std")]
    fn create_engine(&self) -> RenderEngine {
        let mut opts = RenderEngineOptions::for_display(
            self.screen.width as i32,
            self.screen.height as i32,
        );
        opts.layout.margin_left = 10;
        opts.layout.margin_right = 10;
        opts.layout.margin_top = 6;
        opts.layout.margin_bottom = 16;
        opts.layout.first_line_indent_px = 0;
        opts.layout.line_gap_px = 1;
        opts.layout.paragraph_gap_px = 4;
        opts.layout.typography.justification.enabled = false;
        RenderEngine::new(opts)
    }

    #[cfg(feature = "std")]
    fn open_selected_epub(&mut self, ctx: &mut Context<'_, DefaultTheme>) -> Result<(), String> {
        let Some(book) = self.books.get(self.selected) else {
            return Err("No EPUB selected.".to_string());
        };
        let reader = ctx
            .files
            .open_read_seek(book.path.as_str())
            .map_err(|_| "Failed to open file.".to_string())?;

        let open_cfg = OpenConfig {
            options: EpubBookOptions {
                zip_limits: Some(
                    ZipLimits::new(Self::MAX_ZIP_ENTRY_BYTES, 1024).with_max_eocd_scan(8 * 1024),
                ),
                validation_mode: ValidationMode::Lenient,
                max_nav_bytes: Some(Self::MAX_NAV_BYTES),
            },
            lazy_navigation: true,
        };
        let book_obj = EpubBook::from_reader_with_config(reader, open_cfg)
            .map_err(|e| format!("Failed to parse EPUB: {}", e))?;
        let chapter_count = book_obj.chapter_count();
        if chapter_count == 0 {
            return Err("EPUB has no chapters.".to_string());
        }
        let mut session = ReaderSession {
            book: book_obj,
            engine: self.create_engine(),
            chapter_buf: Vec::with_capacity(Self::CHAPTER_BUF_INIT),
            scratch: ScratchBuffers {
                read_buf: Vec::new(),
                xml_buf: Vec::new(),
                text_buf: String::new(),
            },
            chapter_idx: 0,
            page_idx: 0,
            chapter_count,
        };
        let (lines, total_pages) = Self::load_page_lines(&mut session, 0, 0)?;
        self.mode = Mode::Reader {
            title: book.name.clone(),
            lines,
            chapter_idx: 0,
            chapter_count,
            page_idx: 0,
            total_pages,
        };
        self.session = Some(session);
        Ok(())
    }

    #[cfg(feature = "std")]
    fn grow_chapter_buf(session: &mut ReaderSession) -> Result<bool, String> {
        let current = session.chapter_buf.capacity();
        if current >= Self::CHAPTER_BUF_MAX {
            return Ok(false);
        }
        let next = current
            .max(Self::CHAPTER_BUF_INIT)
            .saturating_mul(2)
            .min(Self::CHAPTER_BUF_MAX);
        let additional = next.saturating_sub(session.chapter_buf.len());
        session
            .chapter_buf
            .try_reserve(additional)
            .map_err(|_| "Unable to allocate chapter buffer".to_string())?;
        Ok(session.chapter_buf.capacity() > current)
    }

    #[cfg(feature = "std")]
    fn wrap_line(raw: &str, max_chars: usize, out: &mut Vec<String>) {
        if raw.is_empty() {
            out.push(String::new());
            return;
        }
        let mut line = String::new();
        for word in raw.split_whitespace() {
            if line.is_empty() {
                line.push_str(word);
                continue;
            }
            if line.len() + 1 + word.len() <= max_chars {
                line.push(' ');
                line.push_str(word);
                continue;
            }
            out.push(line);
            line = String::new();
            line.push_str(word);
        }
        if !line.is_empty() {
            out.push(line);
        }
    }

    #[cfg(feature = "std")]
    fn load_page_lines(
        session: &mut ReaderSession,
        chapter_idx: usize,
        page_idx: usize,
    ) -> Result<(Vec<String>, usize), String> {
        let chapter_opts = ChapterEventsOptions {
            max_items: Self::MAX_CHAPTER_EVENTS,
            ..ChapterEventsOptions::default()
        };
        let mut grow_retries = 0usize;
        loop {
            let config = RenderConfig::default().with_page_range(page_idx..page_idx + 1);
            let mut render_session = session.engine.begin(chapter_idx, config);
            let mut picked: Option<RenderPage> = None;
            let mut layout_error: Option<String> = None;
            let stream_result = session.book.chapter_events_with_scratch(
                chapter_idx,
                chapter_opts,
                &mut session.chapter_buf,
                &mut session.scratch,
                |item| {
                    if layout_error.is_some() || picked.is_some() {
                        return Ok::<(), epub_stream::EpubError>(());
                    }
                    if let Err(err) = render_session.push(item) {
                        layout_error = Some(err.to_string());
                        return Ok::<(), epub_stream::EpubError>(());
                    }
                    render_session.drain_pages(|p| {
                        if picked.is_none() {
                            picked = Some(p);
                        }
                    });
                    Ok::<(), epub_stream::EpubError>(())
                },
            );
            if let Err(err) = stream_result {
                let err_s = err.to_string();
                let buffer_small = err_s.to_ascii_lowercase().contains("buffer too small");
                if buffer_small
                    && grow_retries < Self::CHAPTER_GROW_RETRIES
                    && Self::grow_chapter_buf(session)?
                {
                    grow_retries += 1;
                    continue;
                }
                return Err(format!("Render stream failed: {}", err_s));
            }
            if let Some(err) = layout_error {
                return Err(format!("Render layout failed: {}", err));
            }
            render_session
                .finish()
                .map_err(|e| format!("Render finalize failed: {}", e))?;
            render_session.drain_pages(|p| {
                if picked.is_none() {
                    picked = Some(p);
                }
            });
            let Some(page) = picked else {
                return Err("No readable page".to_string());
            };
            let total_pages = page.metrics.chapter_page_count.unwrap_or(page_idx + 1).max(1);
            let mut lines = Vec::new();
            let max_chars = 56usize;
            for cmd in page.content_commands {
                if let DrawCommand::Text(text) = cmd {
                    let t = text.text.trim();
                    if t.is_empty() {
                        continue;
                    }
                    Self::wrap_line(t, max_chars, &mut lines);
                }
            }
            if lines.is_empty() {
                return Err("No readable text".to_string());
            }
            return Ok((lines, total_pages));
        }
    }
}

impl Activity<DefaultTheme> for HomeActivity {
    fn on_enter(&mut self, ctx: &mut Context<'_, DefaultTheme>) {
        self.refresh_library(ctx);
    }

    fn on_input(
        &mut self,
        event: InputEvent,
        ctx: &mut Context<'_, DefaultTheme>,
    ) -> Transition<DefaultTheme> {
        match &mut self.mode {
            Mode::Library => match event {
                InputEvent::Press(Button::Up) | InputEvent::Press(Button::Aux1) => self.move_up(),
                InputEvent::Press(Button::Down) | InputEvent::Press(Button::Aux2) => {
                    self.move_down()
                }
                InputEvent::Press(Button::Back) => self.refresh_library(ctx),
                InputEvent::Press(Button::Confirm) => {
                    #[cfg(feature = "std")]
                    {
                        if let Err(msg) = self.open_selected_epub(ctx) {
                            self.mode = Mode::Message(msg);
                        }
                    }
                    #[cfg(not(feature = "std"))]
                    {
                        self.mode = Mode::Message("Built without std support.".to_string());
                    }
                }
                _ => {}
            },
            Mode::Message(_) => {
                if matches!(event, InputEvent::Press(Button::Back | Button::Confirm)) {
                    self.mode = Mode::Library;
                }
            }
            Mode::Reader {
                chapter_idx,
                chapter_count,
                page_idx,
                total_pages,
                lines,
                ..
            } => match event {
                InputEvent::Press(Button::Back) => {
                    self.mode = Mode::Library;
                    #[cfg(feature = "std")]
                    {
                        self.session = None;
                    }
                }
                #[cfg(feature = "std")]
                InputEvent::Press(Button::Right) => {
                    if let Some(session) = self.session.as_mut() {
                        let target_page = *page_idx + 1;
                        match Self::load_page_lines(session, *chapter_idx, target_page) {
                            Ok((new_lines, pages)) => {
                                *lines = new_lines;
                                *page_idx = target_page;
                                *total_pages = pages;
                                session.page_idx = *page_idx;
                            }
                            Err(_) if *chapter_idx + 1 < *chapter_count => {
                                let next_ch = *chapter_idx + 1;
                                if let Ok((new_lines, pages)) =
                                    Self::load_page_lines(session, next_ch, 0)
                                {
                                    *lines = new_lines;
                                    *chapter_idx = next_ch;
                                    *page_idx = 0;
                                    *total_pages = pages;
                                    session.chapter_idx = *chapter_idx;
                                    session.page_idx = *page_idx;
                                }
                            }
                            Err(_) => {}
                        }
                    }
                }
                #[cfg(feature = "std")]
                InputEvent::Press(Button::Left) => {
                    if let Some(session) = self.session.as_mut() {
                        if *page_idx > 0 {
                            let target_page = *page_idx - 1;
                            if let Ok((new_lines, pages)) =
                                Self::load_page_lines(session, *chapter_idx, target_page)
                            {
                                *lines = new_lines;
                                *page_idx = target_page;
                                *total_pages = pages;
                                session.page_idx = *page_idx;
                            }
                        } else if *chapter_idx > 0 {
                            let prev_ch = *chapter_idx - 1;
                            if let Ok((new_lines, pages)) =
                                Self::load_page_lines(session, prev_ch, 0)
                            {
                                *lines = new_lines;
                                *chapter_idx = prev_ch;
                                *page_idx = 0;
                                *total_pages = pages;
                                session.chapter_idx = *chapter_idx;
                                session.page_idx = *page_idx;
                            }
                        }
                    }
                }
                _ => {}
            },
        }
        Transition::Stay
    }

    fn refresh_hint(&self) -> RefreshHint {
        RefreshHint::Partial
    }

    fn render(&self, ui: &mut dyn Ui<DefaultTheme>) {
        match &self.mode {
            Mode::Library => {
                ui.draw_text_at(Point { x: 12, y: 24 }, "Minimal Reader");
                ui.draw_line(
                    Point { x: 12, y: 32 },
                    Point { x: 468, y: 32 },
                    Color::Black,
                    1,
                );
                ui.draw_text_at(
                    Point { x: 410, y: 24 },
                    format!("{}%", self.battery_pct).as_str(),
                );
                if self.books.is_empty() {
                    ui.draw_text_at(Point { x: 14, y: 64 }, "No EPUB files found.");
                    ui.draw_text_at(
                        Point { x: 14, y: 84 },
                        format!("Expected under {}", self.library_root).as_str(),
                    );
                } else {
                    let start = self.selected.saturating_sub(10);
                    let end = (start + 30).min(self.books.len());
                    for (row, idx) in (start..end).enumerate() {
                        let prefix = if idx == self.selected { ">" } else { " " };
                        ui.draw_text_at(
                            Point {
                                x: 14,
                                y: 56 + (row as i16 * 20),
                            },
                            format!("{} {}", prefix, self.books[idx].name).as_str(),
                        );
                    }
                }
                ui.draw_line(
                    Point { x: 0, y: 772 },
                    Point { x: 479, y: 772 },
                    Color::Black,
                    1,
                );
                ui.draw_text_at(Point { x: 12, y: 792 }, Self::KEY_BACK);
                ui.draw_text_at(Point { x: 320, y: 792 }, Self::KEY_SELECT);
            }
            Mode::Message(msg) => {
                ui.draw_text_at(Point { x: 12, y: 24 }, "Error");
                ui.draw_line(
                    Point { x: 12, y: 32 },
                    Point { x: 468, y: 32 },
                    Color::Black,
                    1,
                );
                ui.draw_text_at(Point { x: 14, y: 64 }, msg.as_str());
                ui.draw_line(
                    Point { x: 0, y: 772 },
                    Point { x: 479, y: 772 },
                    Color::Black,
                    1,
                );
                ui.draw_text_at(Point { x: 12, y: 792 }, "Back: Close");
            }
            Mode::Reader {
                title,
                lines,
                chapter_idx,
                chapter_count,
                page_idx,
                total_pages,
            } => {
                let mut row = 0usize;
                while row < lines.len() && row < 37 {
                    ui.draw_text_at(
                        Point {
                            x: 8,
                            y: 18 + (row as i16 * 20),
                        },
                        lines[row].as_str(),
                    );
                    row += 1;
                }
                ui.draw_line(
                    Point { x: 0, y: 772 },
                    Point { x: 479, y: 772 },
                    Color::Black,
                    1,
                );
                ui.draw_text_at(Point { x: 10, y: 792 }, title.as_str());
                ui.draw_text_at(
                    Point { x: 250, y: 792 },
                    format!(
                        "ch {}/{} p {}/{}",
                        chapter_idx + 1,
                        chapter_count,
                        page_idx + 1,
                        total_pages
                    )
                    .as_str(),
                );
            }
        }
    }
}

#[derive(Default)]
struct NoopSettings {
    slots: [u8; 64],
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
    fn read<'a>(
        &self,
        _path: &str,
        _buf: &'a mut [u8],
    ) -> Result<&'a [u8], einked::storage::FileStoreError> {
        Err(einked::storage::FileStoreError::Io)
    }
    fn exists(&self, _path: &str) -> bool {
        false
    }
}

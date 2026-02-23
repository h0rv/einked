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

#[derive(Default)]
struct NoopSettings {
    slots: [u8; 32],
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
    FeedEntries {
        source_idx: usize,
        title: String,
        entries: Vec<String>,
        selected_idx: usize,
    },
    FeedItem {
        source_idx: usize,
        title: String,
        entries: Vec<String>,
        item_idx: usize,
        lines: Vec<String>,
        scroll: usize,
    },
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
            modal: ModalState::None,
        }
    }

    const SETTINGS_ITEMS: [&'static str; 5] = [
        "Font Size: Medium",
        "Font Family: Serif",
        "Auto Sleep: 10m",
        "Refresh: Never",
        "Invert Colors: Off",
    ];
    const TRANSFER_ITEMS: [&'static str; 3] = ["Edit AP SSID", "Edit AP Password", "Start/Restart"];

    fn move_up(idx: &mut usize) {
        *idx = idx.saturating_sub(1);
    }

    fn move_down(idx: &mut usize, len: usize) {
        if *idx + 1 < len {
            *idx += 1;
        }
    }

    fn library_item_count(&self) -> usize {
        self.files.len().min(4) + 1
    }

    fn refresh_files(&mut self, ctx: &mut Context<'_, DefaultTheme>) {
        let mut entries = Vec::new();
        ctx.files.list("/", &mut |name| {
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".epub")
                || lower.ends_with(".txt")
                || lower.ends_with(".md")
                || lower.ends_with(".epu")
            {
                entries.push(name.to_string());
            }
        });
        entries.sort();
        if entries.is_empty() {
            entries.push("sample_books/notes.txt".to_string());
            entries.push("sample_books/Frankenstein.epub".to_string());
        }
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
        let mut buf = vec![0u8; 64 * 1024];
        let bytes = ctx.files.read(path, &mut buf)?;
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".epub") || lower.ends_with(".epu") {
            let mut lines = Vec::new();
            lines.push(format!("EPUB: {}", path));
            lines.push(format!("Size: {} bytes", bytes.len()));
            lines.push("EPUB parser hookup in progress.".to_string());
            lines.push("This file opened successfully.".to_string());
            Ok(lines)
        } else {
            let text = String::from_utf8_lossy(bytes);
            let mut lines = Vec::new();
            for line in text.lines() {
                lines.push(line.to_string());
            }
            if lines.is_empty() {
                lines.push("(empty file)".to_string());
            }
            Ok(lines)
        }
    }

    fn open_file_in_reader(&mut self, path: &str, ctx: &mut Context<'_, DefaultTheme>) {
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

    fn feed_entries_for_source(&self, source_idx: usize) -> Vec<String> {
        let mut entries = Vec::new();
        if let Some((name, _, ty)) = self.feed_sources.get(source_idx) {
            match ty {
                FeedType::Opds => {
                    entries.push(format!("{}: Top", name));
                    entries.push(format!("{}: Popular", name));
                    entries.push(format!("{}: New", name));
                }
                FeedType::Rss => {
                    entries.push(format!("{}: Headline 1", name));
                    entries.push(format!("{}: Headline 2", name));
                    entries.push(format!("{}: Headline 3", name));
                }
            }
        }
        if entries.is_empty() {
            entries.push("No entries".to_string());
        }
        entries
    }

    fn draw_list_str(
        ui_ctx: &mut dyn Ui<DefaultTheme>,
        y_start: i16,
        selected: usize,
        items: &[String],
    ) {
        for (idx, item) in items.iter().enumerate() {
            let prefix = if idx == selected { "> " } else { "  " };
            ui_ctx.draw_text_at(
                Point {
                    x: 18,
                    y: y_start + (idx as i16 * 22),
                },
                &format!("{}{}", prefix, item),
            );
        }
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
        let items: Vec<String> = Self::SETTINGS_ITEMS
            .iter()
            .map(|s| (*s).to_string())
            .collect();
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

    fn render_bottom_bar(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
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
            ModalState::None => match self.tab {
                MainTab::Library => "Back: Refresh library",
                MainTab::Files => "Back: Up",
                MainTab::Feed => "Back: Sources",
                MainTab::Settings => "Back: No-op",
            },
        };
        ui_ctx.draw_text_at(Point { x: 14, y: 792 }, left_hint);
        ui_ctx.draw_text_at(Point { x: 210, y: 792 }, self.tab.dot_label());
        ui_ctx.draw_text_at(Point { x: 432, y: 792 }, "100%");
    }
}

impl Activity<DefaultTheme> for HomeActivity {
    fn on_enter(&mut self, ctx: &mut Context<'_, DefaultTheme>) {
        self.refresh_files(ctx);
    }

    fn on_input(
        &mut self,
        event: InputEvent,
        ctx: &mut Context<'_, DefaultTheme>,
    ) -> Transition<DefaultTheme> {
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
                        let title = entries[item_idx].clone();
                        let lines = vec![
                            format!("Entry: {}", title),
                            "Feed item opened.".to_string(),
                            "Back returns to entries.".to_string(),
                        ];
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
                        Self::move_down(&mut self.settings_idx, Self::SETTINGS_ITEMS.len())
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
                MainTab::Settings => Transition::Stay,
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
                Self::draw_list_str(ui_ctx, 88, *selected_idx, entries);
            }
            ModalState::FeedItem {
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
    fn confirm_on_library_and_files_opens_detail_modal() {
        let mut act = HomeActivity::new();
        let mut settings = NoopSettings::default();
        let mut files = NoopFiles;
        let mut ctx = test_ctx(&mut settings, &mut files);
        act.on_enter(&mut ctx);

        let _ = act.on_input(InputEvent::Press(Button::Confirm), &mut ctx);
        assert!(matches!(act.modal, ModalState::Reader { .. }));
        let _ = act.on_input(InputEvent::Press(Button::Back), &mut ctx);
        assert!(matches!(act.modal, ModalState::None));

        let _ = act.on_input(InputEvent::Press(Button::Right), &mut ctx);
        let _ = act.on_input(InputEvent::Press(Button::Down), &mut ctx);
        let _ = act.on_input(InputEvent::Press(Button::Confirm), &mut ctx);
        assert!(matches!(act.modal, ModalState::Reader { .. }));
    }
}

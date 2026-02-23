#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;

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
    settings: NoopSettings,
    files: NoopFiles,
    config: DeviceConfig,
}

impl EreaderRuntime {
    pub fn new(config: DeviceConfig) -> Self {
        let mut stack = ActivityStack::new();
        let theme = DefaultTheme;
        let mut settings = NoopSettings::default();
        let mut files = NoopFiles;
        let mut ctx = Context {
            theme: &theme,
            screen: config.screen,
            settings: &mut settings,
            files: &mut files,
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
            settings: &mut self.settings,
            files: &mut self.files,
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
    Settings,
}

impl MainTab {
    fn next(self) -> Self {
        match self {
            Self::Library => Self::Files,
            Self::Files => Self::Settings,
            Self::Settings => Self::Library,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Library => Self::Settings,
            Self::Files => Self::Library,
            Self::Settings => Self::Files,
        }
    }

    fn dot_label(self) -> &'static str {
        match self {
            Self::Library => "O o o",
            Self::Files => "o O o",
            Self::Settings => "o o O",
        }
    }
}

struct HomeActivity {
    tab: MainTab,
    library_idx: usize,
    files_idx: usize,
    settings_idx: usize,
    transfer_open: bool,
    transfer_menu_idx: usize,
}

impl HomeActivity {
    fn new() -> Self {
        Self {
            tab: MainTab::Library,
            library_idx: 0,
            files_idx: 0,
            settings_idx: 0,
            transfer_open: false,
            transfer_menu_idx: 0,
        }
    }

    const LIBRARY_ITEMS: [&'static str; 5] = [
        "Continue: Moby Dick (43%)",
        "Pride and Prejudice (12%)",
        "Frankenstein (new)",
        "The Great Gatsby (7%)",
        "File Transfer",
    ];
    const FILES_ITEMS: [&'static str; 4] = ["books/", "downloads/", "notes/", "samples/"];
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

    fn draw_list(ui_ctx: &mut dyn Ui<DefaultTheme>, y_start: i16, selected: usize, items: &[&str]) {
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

    fn render_library(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        ui_ctx.draw_text_at(Point { x: 16, y: 26 }, "Library");
        ui_ctx.draw_line(
            Point { x: 16, y: 34 },
            Point { x: 464, y: 34 },
            Color::Black,
            1,
        );
        Self::draw_list(ui_ctx, 66, self.library_idx, &Self::LIBRARY_ITEMS);
    }

    fn render_files(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        ui_ctx.draw_text_at(Point { x: 16, y: 26 }, "Files");
        ui_ctx.draw_line(
            Point { x: 16, y: 34 },
            Point { x: 464, y: 34 },
            Color::Black,
            1,
        );
        Self::draw_list(ui_ctx, 66, self.files_idx, &Self::FILES_ITEMS);
    }

    fn render_settings(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        ui_ctx.draw_text_at(Point { x: 16, y: 26 }, "Settings");
        ui_ctx.draw_line(
            Point { x: 16, y: 34 },
            Point { x: 464, y: 34 },
            Color::Black,
            1,
        );
        Self::draw_list(ui_ctx, 66, self.settings_idx, &Self::SETTINGS_ITEMS);
    }

    fn render_transfer_screen(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
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
        Self::draw_list(ui_ctx, 210, self.transfer_menu_idx, &Self::TRANSFER_ITEMS);
    }

    fn render_bottom_bar(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        ui_ctx.draw_line(
            Point { x: 0, y: 772 },
            Point { x: 479, y: 772 },
            Color::Black,
            1,
        );
        let left_hint = if self.transfer_open {
            "Back: Exit transfer"
        } else {
            match self.tab {
                MainTab::Library => "Back: Refresh library",
                MainTab::Files => "Back: Up",
                MainTab::Settings => "Back: No-op",
            }
        };
        ui_ctx.draw_text_at(Point { x: 14, y: 792 }, left_hint);
        ui_ctx.draw_text_at(Point { x: 215, y: 792 }, self.tab.dot_label());
        ui_ctx.draw_text_at(Point { x: 432, y: 792 }, "100%");
    }
}

impl Activity<DefaultTheme> for HomeActivity {
    fn on_input(
        &mut self,
        event: InputEvent,
        _ctx: &mut Context<'_, DefaultTheme>,
    ) -> Transition<DefaultTheme> {
        if self.transfer_open {
            return match event {
                InputEvent::Press(Button::Back) => {
                    self.transfer_open = false;
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
                    MainTab::Settings => Self::move_up(&mut self.settings_idx),
                }
                Transition::Stay
            }
            InputEvent::Press(Button::Down) | InputEvent::Press(Button::Aux2) => {
                match self.tab {
                    MainTab::Library => {
                        Self::move_down(&mut self.library_idx, Self::LIBRARY_ITEMS.len())
                    }
                    MainTab::Files => Self::move_down(&mut self.files_idx, Self::FILES_ITEMS.len()),
                    MainTab::Settings => {
                        Self::move_down(&mut self.settings_idx, Self::SETTINGS_ITEMS.len())
                    }
                }
                Transition::Stay
            }
            InputEvent::Press(Button::Confirm) => match self.tab {
                MainTab::Library if self.library_idx == Self::LIBRARY_ITEMS.len() - 1 => {
                    self.transfer_open = true;
                    self.transfer_menu_idx = 0;
                    Transition::Stay
                }
                _ => Transition::Stay,
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

        if self.transfer_open {
            self.render_transfer_screen(ui_ctx);
        } else {
            match self.tab {
                MainTab::Library => self.render_library(ui_ctx),
                MainTab::Files => self.render_files(ui_ctx),
                MainTab::Settings => self.render_settings(ui_ctx),
            }
        }
        self.render_bottom_bar(ui_ctx);
    }

    fn refresh_hint(&self) -> RefreshHint {
        RefreshHint::Fast
    }
}

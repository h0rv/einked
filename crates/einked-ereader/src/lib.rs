#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;

use einked::activity_stack::{Activity, ActivityStack, Context, Transition, Ui};
use einked::core::{DefaultTheme, Rect};
use einked::dsl::UiDsl;
use einked::input::{Button, InputEvent};
use einked::pipeline::FramePipeline;
use einked::refresh::RefreshHint;
use einked::render_ir::DrawCmd;
use einked::storage::{FileStore, FileStoreError, SettingsStore};
use einked::ui::components::Header;
use einked::ui::runtime::UiRuntime;

pub trait FrameSink {
    fn render_and_flush(&mut self, cmds: &[DrawCmd<'static>], hint: RefreshHint) -> bool;
}

pub struct EreaderRuntime {
    stack: ActivityStack<DefaultTheme, 8>,
    pipeline: FramePipeline<512, 512>,
    theme: DefaultTheme,
    settings: NoopSettings,
    files: NoopFiles,
    screen: Rect,
}

impl EreaderRuntime {
    pub fn new(screen: Rect) -> Self {
        let mut stack = ActivityStack::new();
        let theme = DefaultTheme;
        let mut settings = NoopSettings::default();
        let mut files = NoopFiles;
        let mut ctx = Context {
            theme: &theme,
            screen,
            settings: &mut settings,
            files: &mut files,
        };
        let _ = stack.push_root(Box::new(HomeActivity::new()), &mut ctx);

        let mut pipeline = FramePipeline::new(8);
        pipeline.set_viewport_width(screen.width);

        Self {
            stack,
            pipeline,
            theme,
            settings,
            files,
            screen,
        }
    }

    pub fn tick(&mut self, input: Option<InputEvent>, sink: &mut impl FrameSink) -> bool {
        let mut ctx = Context {
            theme: &self.theme,
            screen: self.screen,
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
            Header::new("einked")
                .with_right_text("ereader")
                .render_to_runtime(&mut ui.runtime);
            hint = ui.runtime.take_refresh_hint();
        }

        sink.render_and_flush(self.pipeline.current_commands(), hint)
    }
}

impl Default for EreaderRuntime {
    fn default() -> Self {
        Self::new(Rect {
            x: 0,
            y: 0,
            width: 480,
            height: 800,
        })
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

struct HomeActivity {
    selected: usize,
}

impl HomeActivity {
    fn new() -> Self {
        Self { selected: 0 }
    }

    const ITEMS: [&'static str; 4] = ["Read", "Library", "Settings", "About"];
}

impl Activity<DefaultTheme> for HomeActivity {
    fn on_input(
        &mut self,
        event: InputEvent,
        _ctx: &mut Context<'_, DefaultTheme>,
    ) -> Transition<DefaultTheme> {
        match event {
            InputEvent::Press(Button::Up) | InputEvent::Press(Button::Aux1) => {
                if self.selected == 0 {
                    self.selected = Self::ITEMS.len() - 1;
                } else {
                    self.selected -= 1;
                }
                Transition::Stay
            }
            InputEvent::Press(Button::Down) | InputEvent::Press(Button::Aux2) => {
                self.selected = (self.selected + 1) % Self::ITEMS.len();
                Transition::Stay
            }
            InputEvent::Press(Button::Confirm) => Transition::Push(Box::new(DetailActivity {
                ticks: 0,
                selected: self.selected,
            })),
            _ => Transition::Stay,
        }
    }

    fn render(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        ui_ctx.status_bar("Home", "Confirm");
        ui_ctx.divider();
        for (idx, item) in Self::ITEMS.iter().enumerate() {
            if idx == self.selected {
                ui_ctx.label(&format!("> {}", item));
            } else {
                ui_ctx.label(item);
            }
        }
    }

    fn refresh_hint(&self) -> RefreshHint {
        RefreshHint::Fast
    }
}

struct DetailActivity {
    ticks: u32,
    selected: usize,
}

impl Activity<DefaultTheme> for DetailActivity {
    fn on_input(
        &mut self,
        event: InputEvent,
        _ctx: &mut Context<'_, DefaultTheme>,
    ) -> Transition<DefaultTheme> {
        match event {
            InputEvent::Press(Button::Back) => Transition::Pop,
            _ => {
                self.ticks = self.ticks.saturating_add(1);
                Transition::Stay
            }
        }
    }

    fn render(&self, ui_ctx: &mut dyn Ui<DefaultTheme>) {
        ui_ctx.status_bar("Detail", "Back");
        ui_ctx.divider();
        ui_ctx.paragraph("einked-ereader app runtime");
        ui_ctx.label(&format!("selected item index: {}", self.selected));
        ui_ctx.label(&format!("ticks: {}", self.ticks));
    }

    fn refresh_hint(&self) -> RefreshHint {
        RefreshHint::Adaptive
    }
}

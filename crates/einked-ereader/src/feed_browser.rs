extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use einked::activity_stack::{Activity, Context, Transition, Ui};
use einked::core::DefaultTheme;
use einked::input::{Button, InputEvent};
use einked::refresh::RefreshHint;

use crate::feed::{FeedType, OpdsCatalog, all_preloaded_sources};

#[derive(Debug, Clone, PartialEq)]
pub enum BrowserState {
    SourceList,
    Loading,
    BrowsingCatalog,
    BookDetail,
    Downloading(f32),
    Error(String),
}

pub struct FeedBrowserActivity {
    state: BrowserState,
    selected_index: usize,
    scroll_offset: usize,
    current_catalog: Option<OpdsCatalog>,
    pending_fetch_url: Option<String>,
    pending_download_url: Option<String>,
    status_message: Option<String>,
    sources: Vec<(&'static str, &'static str, FeedType)>,
}

impl FeedBrowserActivity {
    pub fn new() -> Self {
        Self {
            state: BrowserState::SourceList,
            selected_index: 0,
            scroll_offset: 0,
            current_catalog: None,
            pending_fetch_url: None,
            pending_download_url: None,
            status_message: None,
            sources: all_preloaded_sources(),
        }
    }

    pub fn state(&self) -> &BrowserState {
        &self.state
    }

    pub fn set_catalog(&mut self, catalog: OpdsCatalog) {
        self.current_catalog = Some(catalog);
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.state = BrowserState::BrowsingCatalog;
    }

    pub fn set_loading(&mut self) {
        self.state = BrowserState::Loading;
    }

    pub fn set_error(&mut self, message: String) {
        self.state = BrowserState::Error(message);
    }

    pub fn set_download_progress(&mut self, progress: f32) {
        self.state = BrowserState::Downloading(progress.clamp(0.0, 1.0));
    }

    pub fn complete_download(&mut self) {
        self.state = BrowserState::BrowsingCatalog;
        self.status_message = Some("Download complete".to_string());
    }

    pub fn take_fetch_request(&mut self) -> Option<String> {
        self.pending_fetch_url.take()
    }

    pub fn take_download_request(&mut self) -> Option<String> {
        self.pending_download_url.take()
    }

    fn item_count(&self) -> usize {
        match self.state {
            BrowserState::SourceList => self.sources.len(),
            BrowserState::BrowsingCatalog | BrowserState::BookDetail => self
                .current_catalog
                .as_ref()
                .map(|catalog| catalog.entries.len())
                .unwrap_or(0),
            BrowserState::Loading | BrowserState::Downloading(_) | BrowserState::Error(_) => 0,
        }
    }

    fn select_next(&mut self) {
        let count = self.item_count();
        if count == 0 {
            return;
        }
        self.selected_index = (self.selected_index + 1) % count;
    }

    fn select_prev(&mut self) {
        let count = self.item_count();
        if count == 0 {
            return;
        }
        self.selected_index = if self.selected_index == 0 {
            count - 1
        } else {
            self.selected_index - 1
        };
    }
}

impl Default for FeedBrowserActivity {
    fn default() -> Self {
        Self::new()
    }
}

impl Activity<DefaultTheme> for FeedBrowserActivity {
    fn on_input(
        &mut self,
        event: InputEvent,
        _ctx: &mut Context<'_, DefaultTheme>,
    ) -> Transition<DefaultTheme> {
        match (&self.state, event) {
            (BrowserState::SourceList, InputEvent::Press(Button::Down))
            | (BrowserState::SourceList, InputEvent::Press(Button::Aux2)) => {
                self.select_next();
                Transition::Stay
            }
            (BrowserState::SourceList, InputEvent::Press(Button::Up))
            | (BrowserState::SourceList, InputEvent::Press(Button::Aux1)) => {
                self.select_prev();
                Transition::Stay
            }
            (BrowserState::SourceList, InputEvent::Press(Button::Confirm))
            | (BrowserState::SourceList, InputEvent::Press(Button::Right)) => {
                if let Some((_, url, _)) = self.sources.get(self.selected_index) {
                    self.pending_fetch_url = Some((*url).to_string());
                    self.state = BrowserState::Loading;
                }
                Transition::Stay
            }
            (BrowserState::SourceList, InputEvent::Press(Button::Back)) => Transition::Pop,

            (BrowserState::BrowsingCatalog, InputEvent::Press(Button::Down))
            | (BrowserState::BrowsingCatalog, InputEvent::Press(Button::Aux2)) => {
                self.select_next();
                Transition::Stay
            }
            (BrowserState::BrowsingCatalog, InputEvent::Press(Button::Up))
            | (BrowserState::BrowsingCatalog, InputEvent::Press(Button::Aux1)) => {
                self.select_prev();
                Transition::Stay
            }
            (BrowserState::BrowsingCatalog, InputEvent::Press(Button::Confirm))
            | (BrowserState::BrowsingCatalog, InputEvent::Press(Button::Right)) => {
                self.state = BrowserState::BookDetail;
                Transition::Stay
            }
            (BrowserState::BrowsingCatalog, InputEvent::Press(Button::Back))
            | (BrowserState::BrowsingCatalog, InputEvent::Press(Button::Left)) => {
                self.state = BrowserState::SourceList;
                self.current_catalog = None;
                self.selected_index = 0;
                self.scroll_offset = 0;
                Transition::Stay
            }

            (BrowserState::BookDetail, InputEvent::Press(Button::Back))
            | (BrowserState::BookDetail, InputEvent::Press(Button::Left)) => {
                self.state = BrowserState::BrowsingCatalog;
                Transition::Stay
            }

            (_, InputEvent::Press(Button::Back)) => {
                self.state = BrowserState::SourceList;
                Transition::Stay
            }
            _ => Transition::Stay,
        }
    }

    fn render(&self, ui: &mut dyn Ui<DefaultTheme>) {
        ui.status_bar("Feeds", "Back");
        ui.divider();
        match &self.state {
            BrowserState::SourceList => {
                for (idx, (name, _, ty)) in self.sources.iter().enumerate() {
                    let prefix = if idx == self.selected_index {
                        "> "
                    } else {
                        "  "
                    };
                    let kind = match ty {
                        FeedType::Opds => "OPDS",
                        FeedType::Rss => "RSS",
                    };
                    ui.label(&format!("{}{} ({})", prefix, name, kind));
                }
            }
            BrowserState::Loading => {
                ui.paragraph("Loading feed...");
            }
            BrowserState::BrowsingCatalog => {
                if let Some(catalog) = &self.current_catalog {
                    ui.label(&catalog.title);
                    for (idx, entry) in catalog.entries.iter().enumerate() {
                        let prefix = if idx == self.selected_index {
                            "> "
                        } else {
                            "  "
                        };
                        ui.label(&format!("{}{}", prefix, entry.title));
                    }
                } else {
                    ui.paragraph("No catalog loaded.");
                }
            }
            BrowserState::BookDetail => {
                if let Some(catalog) = &self.current_catalog
                    && let Some(entry) = catalog.entries.get(self.selected_index)
                {
                    ui.label(&entry.title);
                    if let Some(author) = &entry.author {
                        ui.label(&format!("by {}", author));
                    }
                    if let Some(summary) = &entry.summary {
                        ui.paragraph(summary);
                    }
                }
            }
            BrowserState::Downloading(progress) => {
                ui.label("Downloading...");
                ui.label(&format!("{:.0}%", progress * 100.0));
            }
            BrowserState::Error(msg) => {
                ui.label("Feed Error");
                ui.paragraph(msg);
            }
        }

        if let Some(msg) = &self.status_message {
            ui.divider();
            ui.label(msg);
        }
    }

    fn refresh_hint(&self) -> RefreshHint {
        RefreshHint::Fast
    }
}

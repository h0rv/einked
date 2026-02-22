use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics_web_simulator::{
    display::WebSimulatorDisplay, output_settings::OutputSettingsBuilder,
};

use einked::input::Button;
use einked::ui::{Header, List, Theme, Toast};

const DISPLAY_WIDTH: u32 = 480;
const DISPLAY_HEIGHT: u32 = 800;

struct State {
    theme: Theme,
    list: List,
    toast_ticks: u8,
    display: WebSimulatorDisplay<BinaryColor>,
}

impl State {
    fn new(display: WebSimulatorDisplay<BinaryColor>) -> Self {
        let mut state = Self {
            theme: Theme::default(),
            list: List::new(
                vec![
                    "Open Book".to_string(),
                    "Library".to_string(),
                    "Settings".to_string(),
                    "About".to_string(),
                ],
                20,
                92,
                DISPLAY_WIDTH - 40,
                4,
            ),
            toast_ticks: 0,
            display,
        };
        state.render();
        state
    }

    fn on_button(&mut self, button: Button) {
        match button {
            Button::Up | Button::Aux1 => self.list.select_prev(),
            Button::Down | Button::Aux2 => self.list.select_next(),
            Button::Confirm => self.toast_ticks = 20,
            _ => {}
        }
        self.render();
    }

    fn render(&mut self) {
        Rectangle::new(Point::new(0, 0), Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
            .draw(&mut self.display)
            .expect("clear");

        Header::new("einked Web")
            .with_right_text("demo")
            .render(&mut self.display, &self.theme)
            .expect("header");
        self.list
            .render(&mut self.display, &self.theme)
            .expect("list");

        if self.toast_ticks > 0 {
            let selected = self.list.selected().unwrap_or("-");
            Toast::bottom_center(
                &format!("Selected: {}", selected),
                DISPLAY_WIDTH,
                DISPLAY_HEIGHT,
            )
            .render(&mut self.display)
            .expect("toast");
            self.toast_ticks -= 1;
        }

        self.display.flush().expect("flush");
    }
}

#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let window = web_sys::window().expect("window");
    let document = window.document().expect("document");
    let container = document
        .get_element_by_id("display-container")
        .expect("display-container element");

    let output_settings = OutputSettingsBuilder::new().scale(1).build();
    let display = WebSimulatorDisplay::new(
        (DISPLAY_WIDTH, DISPLAY_HEIGHT),
        &output_settings,
        Some(&container),
    );

    let state = Rc::new(RefCell::new(State::new(display)));

    let state_clone = state.clone();
    let closure = Closure::wrap(Box::new(move |e: web_sys::KeyboardEvent| {
        if let Some(btn) = key_to_button(&e.key()) {
            e.prevent_default();
            state_clone.borrow_mut().on_button(btn);
        }
    }) as Box<dyn FnMut(_)>);

    window.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref())?;
    closure.forget();

    web_sys::console::log_1(&"einked web simulator ready".into());
    web_sys::console::log_1(&"controls: up/down or w/s, enter".into());

    Ok(())
}

fn key_to_button(key: &str) -> Option<Button> {
    match key {
        "ArrowUp" | "w" => Some(Button::Up),
        "ArrowDown" | "s" => Some(Button::Down),
        "ArrowLeft" | "a" => Some(Button::Left),
        "ArrowRight" | "d" => Some(Button::Right),
        "Enter" | " " => Some(Button::Confirm),
        "Backspace" => Some(Button::Back),
        _ => None,
    }
}

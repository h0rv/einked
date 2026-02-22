use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics_simulator::{
    sdl2::Keycode, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};

use einked::input::Button;
use einked::ui::{Header, List, Theme, Toast};

const DISPLAY_WIDTH: u32 = 480;
const DISPLAY_HEIGHT: u32 = 800;

struct DemoState {
    theme: Theme,
    list: List,
    toast_ticks: u8,
}

impl DemoState {
    fn new() -> Self {
        let items = vec![
            "Open Book".to_string(),
            "Library".to_string(),
            "Settings".to_string(),
            "About".to_string(),
        ];

        Self {
            theme: Theme::default(),
            list: List::new(items, 20, 92, DISPLAY_WIDTH - 40, 4),
            toast_ticks: 0,
        }
    }

    fn on_button(&mut self, button: Button) {
        match button {
            Button::Up | Button::Aux1 => self.list.select_prev(),
            Button::Down | Button::Aux2 => self.list.select_next(),
            Button::Confirm => self.toast_ticks = 30,
            _ => {}
        }
    }

    fn render(&mut self, display: &mut SimulatorDisplay<BinaryColor>) {
        Rectangle::new(Point::new(0, 0), Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
            .draw(display)
            .expect("clear");

        Header::new("einked Desktop")
            .with_right_text("demo")
            .render(display, &self.theme)
            .expect("header");
        self.list.render(display, &self.theme).expect("list");

        if self.toast_ticks > 0 {
            let selected = self.list.selected().unwrap_or("-");
            Toast::bottom_center(
                &format!("Selected: {}", selected),
                DISPLAY_WIDTH,
                DISPLAY_HEIGHT,
            )
            .render(display)
            .expect("toast");
            self.toast_ticks -= 1;
        }
    }
}

fn main() {
    let output_settings = OutputSettingsBuilder::new().scale(1).build();
    let mut display: SimulatorDisplay<BinaryColor> =
        SimulatorDisplay::new(Size::new(DISPLAY_WIDTH, DISPLAY_HEIGHT));
    let mut window = Window::new("einked-sim-desktop", &output_settings);

    let mut state = DemoState::new();
    state.render(&mut display);
    window.update(&display);

    println!("einked desktop simulator");
    println!("controls: up/down or w/s, enter to validate input/render pipeline");

    'event_loop: loop {
        let events = window.events().collect::<Vec<_>>();
        for event in events {
            match event {
                SimulatorEvent::Quit => break 'event_loop,
                SimulatorEvent::KeyDown { keycode, .. } => {
                    if keycode == Keycode::Escape {
                        break 'event_loop;
                    }

                    if let Some(button) = map_key(keycode) {
                        state.on_button(button);
                        state.render(&mut display);
                        window.update(&display);
                    }
                }
                _ => {}
            }
        }
    }
}

fn map_key(keycode: Keycode) -> Option<Button> {
    match keycode {
        Keycode::Up | Keycode::W => Some(Button::Up),
        Keycode::Down | Keycode::S => Some(Button::Down),
        Keycode::Left | Keycode::A => Some(Button::Left),
        Keycode::Right | Keycode::D => Some(Button::Right),
        Keycode::Return | Keycode::Space => Some(Button::Confirm),
        Keycode::Backspace => Some(Button::Back),
        _ => None,
    }
}

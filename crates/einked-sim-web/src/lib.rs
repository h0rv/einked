use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use embedded_graphics::mono_font::{ascii, MonoTextStyleBuilder};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use embedded_graphics_web_simulator::{
    display::WebSimulatorDisplay, output_settings::OutputSettingsBuilder,
};

use einked::core::Color;
use einked::input::{Button, InputEvent};
use einked::refresh::RefreshHint;
use einked::render_ir::{DrawCmd, ImageFormat};
use einked_ereader::{DeviceConfig, EreaderRuntime, FrameSink};

struct State {
    runtime: EreaderRuntime,
    display: WebSimulatorDisplay<BinaryColor>,
}

#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let window = web_sys::window().expect("window");
    let document = window.document().expect("document");
    let container = document
        .get_element_by_id("display-container")
        .expect("display-container element");

    let config = DeviceConfig::xteink_x4();
    let output_settings = OutputSettingsBuilder::new().scale(1).build();
    let display = WebSimulatorDisplay::new(
        (config.screen.width as u32, config.screen.height as u32),
        &output_settings,
        Some(&container),
    );

    let mut init_state = State {
        runtime: EreaderRuntime::new(config),
        display,
    };
    {
        let mut sink = StateSink {
            display: &mut init_state.display,
        };
        let _ = init_state.runtime.tick(None, &mut sink);
    }

    let state = Rc::new(RefCell::new(init_state));

    let state_clone = state.clone();
    let closure = Closure::wrap(Box::new(move |e: web_sys::KeyboardEvent| {
        if let Some(btn) = key_to_button(&e.key()) {
            e.prevent_default();
            let mut state = state_clone.borrow_mut();
            let input = Some(InputEvent::Press(btn));
            let mut sink = StateSink {
                display: &mut state.display,
            };
            let _ = state.runtime.tick(input, &mut sink);
        }
    }) as Box<dyn FnMut(_)>);

    window.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref())?;
    closure.forget();

    web_sys::console::log_1(&"einked-ereader web simulator ready".into());
    Ok(())
}

struct StateSink<'a> {
    display: &'a mut WebSimulatorDisplay<BinaryColor>,
}

impl FrameSink for StateSink<'_> {
    fn render_and_flush(&mut self, cmds: &[DrawCmd<'static>], _hint: RefreshHint) -> bool {
        self.display.clear(BinaryColor::Off).ok();
        rasterize_commands(cmds, self.display);
        self.display.flush().ok();
        true
    }
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

fn rasterize_commands(cmds: &[DrawCmd<'static>], display: &mut WebSimulatorDisplay<BinaryColor>) {
    for cmd in cmds {
        match cmd {
            DrawCmd::FillRect { rect, color } => {
                let draw_color = to_binary(*color);
                let _ = Rectangle::new(
                    Point::new(rect.x as i32, rect.y as i32),
                    Size::new(rect.width as u32, rect.height as u32),
                )
                .into_styled(PrimitiveStyle::with_fill(draw_color))
                .draw(display);
            }
            DrawCmd::DrawText { pos, text, .. } => {
                let style = MonoTextStyleBuilder::new()
                    .font(&ascii::FONT_8X13_BOLD)
                    .text_color(BinaryColor::On)
                    .build();
                let _ = Text::new(text.as_str(), Point::new(pos.x as i32, pos.y as i32), style)
                    .draw(display);
            }
            DrawCmd::DrawLine {
                start, end, color, ..
            } => {
                let min_x = start.x.min(end.x);
                let max_x = start.x.max(end.x);
                let min_y = start.y.min(end.y);
                let max_y = start.y.max(end.y);
                let _ = Rectangle::new(
                    Point::new(min_x as i32, min_y as i32),
                    Size::new((max_x - min_x + 1) as u32, (max_y - min_y + 1) as u32),
                )
                .into_styled(PrimitiveStyle::with_fill(to_binary(*color)))
                .draw(display);
            }
            DrawCmd::DrawImage {
                rect, data, format, ..
            } => draw_image(display, *rect, data, *format),
            DrawCmd::Clip { .. } | DrawCmd::Unclip => {}
        }
    }
}

fn draw_image(
    display: &mut WebSimulatorDisplay<BinaryColor>,
    rect: einked::core::Rect,
    data: &[u8],
    format: ImageFormat,
) {
    match format {
        ImageFormat::Mono1bpp => {
            let stride = (rect.width as usize).div_ceil(8);
            let _ = display.draw_iter((0..rect.height as usize).flat_map(|y| {
                let row = data
                    .get(y.saturating_mul(stride)..((y + 1).saturating_mul(stride)).min(data.len()))
                    .unwrap_or(&[]);
                (0..rect.width as usize).map(move |x| {
                    let byte = row.get(x / 8).copied().unwrap_or(0);
                    let bit = 7 - (x % 8);
                    let color = if (byte >> bit) & 1 == 1 {
                        BinaryColor::On
                    } else {
                        BinaryColor::Off
                    };
                    Pixel(Point::new(rect.x as i32 + x as i32, rect.y as i32 + y as i32), color)
                })
            }));
        }
        ImageFormat::Gray8 => {
            let stride = rect.width as usize;
            let _ = display.draw_iter((0..rect.height as usize).flat_map(|y| {
                let row = data
                    .get(y.saturating_mul(stride)..((y + 1).saturating_mul(stride)).min(data.len()))
                    .unwrap_or(&[]);
                (0..rect.width as usize).map(move |x| {
                    let color = if row.get(x).copied().unwrap_or(255) < 128 {
                        BinaryColor::On
                    } else {
                        BinaryColor::Off
                    };
                    Pixel(Point::new(rect.x as i32 + x as i32, rect.y as i32 + y as i32), color)
                })
            }));
        }
    }
}

fn to_binary(color: Color) -> BinaryColor {
    match color {
        Color::Black => BinaryColor::On,
        Color::White => BinaryColor::Off,
        Color::Gray(v) => {
            if v < 128 {
                BinaryColor::On
            } else {
                BinaryColor::Off
            }
        }
        Color::Red | Color::Custom(_) => BinaryColor::On,
    }
}

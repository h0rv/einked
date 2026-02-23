use embedded_graphics::mono_font::{ascii, MonoTextStyleBuilder};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;
use embedded_graphics_simulator::{
    sdl2::Keycode, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};

use einked::core::Color;
use einked::input::{Button, InputEvent};
use einked::refresh::RefreshHint;
use einked::render_ir::DrawCmd;
use einked_ereader::{DeviceConfig, EreaderRuntime, FrameSink};

struct DesktopSink<'a> {
    display: &'a mut SimulatorDisplay<BinaryColor>,
}

impl FrameSink for DesktopSink<'_> {
    fn render_and_flush(&mut self, cmds: &[DrawCmd<'static>], _hint: RefreshHint) -> bool {
        self.display.clear(BinaryColor::Off).ok();
        rasterize_commands(cmds, self.display);
        true
    }
}

fn main() {
    let config = DeviceConfig::xteink_x4();
    let output_settings = OutputSettingsBuilder::new().scale(1).build();
    let mut display: SimulatorDisplay<BinaryColor> =
        SimulatorDisplay::new(Size::new(config.screen.width as u32, config.screen.height as u32));
    let mut window = Window::new("einked-ereader (desktop)", &output_settings);
    let mut runtime = EreaderRuntime::new(config);

    {
        let mut sink = DesktopSink {
            display: &mut display,
        };
        let _ = runtime.tick(None, &mut sink);
    }
    window.update(&display);

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
                        let mut sink = DesktopSink {
                            display: &mut display,
                        };
                        let _ = runtime.tick(Some(InputEvent::Press(button)), &mut sink);
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

fn rasterize_commands(cmds: &[DrawCmd<'static>], display: &mut SimulatorDisplay<BinaryColor>) {
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
            DrawCmd::DrawImage { .. } | DrawCmd::Clip { .. } | DrawCmd::Unclip => {}
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

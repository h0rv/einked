use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use einked::core::Color;
use einked::input::{Button, InputEvent};
use einked::refresh::RefreshHint;
use einked::render_ir::DrawCmd;
use einked::storage::{FileStore, FileStoreError, SettingsStore};
use einked_ereader::{DeviceConfig, EreaderRuntime, FrameSink};
use font8x8::{BASIC_FONTS, UnicodeFonts};
use image::{GrayImage, Luma};

struct CaptureSink {
    cmds: Vec<DrawCmd<'static>>,
}

impl CaptureSink {
    fn new() -> Self {
        Self { cmds: Vec::new() }
    }
}

impl FrameSink for CaptureSink {
    fn render_and_flush(&mut self, cmds: &[DrawCmd<'static>], _hint: RefreshHint) -> bool {
        self.cmds = cmds.to_vec();
        true
    }
}

struct TestSettings {
    slots: [u8; 64],
}

impl Default for TestSettings {
    fn default() -> Self {
        Self {
            slots: [u8::MAX; 64],
        }
    }
}

impl SettingsStore for TestSettings {
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

struct TestFiles {
    files: BTreeMap<String, Vec<u8>>,
}

impl TestFiles {
    fn from_map(files: BTreeMap<String, Vec<u8>>) -> Self {
        Self { files }
    }
}

impl FileStore for TestFiles {
    fn list(&self, path: &str, out: &mut dyn FnMut(&str)) {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            for key in self.files.keys() {
                if !key.contains('/') {
                    out(key);
                }
            }
            return;
        }

        let prefix = format!("{path}/");
        for key in self.files.keys() {
            if let Some(name) = key.strip_prefix(&prefix)
                && !name.contains('/')
            {
                out(name);
            }
        }
    }

    fn read<'a>(&self, path: &str, buf: &'a mut [u8]) -> Result<&'a [u8], FileStoreError> {
        let key = path.trim_start_matches('/');
        let bytes = self.files.get(key).ok_or(FileStoreError::Io)?;
        let n = bytes.len().min(buf.len());
        buf[..n].copy_from_slice(&bytes[..n]);
        Ok(&buf[..n])
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path.trim_start_matches('/'))
    }

    fn open_read_seek(
        &self,
        path: &str,
    ) -> Result<Box<dyn einked::storage::ReadSeek>, FileStoreError> {
        let key = path.trim_start_matches('/');
        let bytes = self.files.get(key).ok_or(FileStoreError::Io)?;
        Ok(Box::new(Cursor::new(bytes.clone())))
    }
}

#[derive(Debug, Clone)]
struct ScreenText {
    x: i16,
    y: i16,
    text: String,
}

fn capture(
    runtime: &mut EreaderRuntime,
    sink: &mut CaptureSink,
    input: Option<InputEvent>,
    scenario: &str,
    step_idx: usize,
) -> Vec<ScreenText> {
    let _ = runtime.tick(input, sink);
    let texts = extract_texts(&sink.cmds);
    write_artifact(
        scenario,
        step_idx,
        &sink.cmds,
        runtime.config().screen.width as u32,
        runtime.config().screen.height as u32,
        &texts,
    );
    texts
}

fn extract_texts(cmds: &[DrawCmd<'static>]) -> Vec<ScreenText> {
    let mut out = Vec::new();
    for cmd in cmds {
        if let DrawCmd::DrawText { pos, text, .. } = cmd {
            out.push(ScreenText {
                x: pos.x,
                y: pos.y,
                text: text.as_str().to_string(),
            });
        }
    }
    out
}

fn contains_text(texts: &[ScreenText], needle: &str) -> bool {
    texts.iter().any(|t| t.text.contains(needle))
}

fn contains_any(texts: &[ScreenText], needles: &[&str]) -> bool {
    needles.iter().any(|needle| contains_text(texts, needle))
}

fn footer_metrics(texts: &[ScreenText]) -> Option<(usize, usize, usize, usize)> {
    let footer = texts.iter().find(|t| t.text.starts_with("ch "))?;
    let mut nums = Vec::new();
    let mut cur = String::new();
    for ch in footer.text.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            nums.push(cur.parse::<usize>().ok()?);
            cur.clear();
        }
    }
    if !cur.is_empty() {
        nums.push(cur.parse::<usize>().ok()?);
    }
    if nums.len() < 4 {
        return None;
    }
    Some((nums[0], nums[1], nums[2], nums[3]))
}

fn write_artifact(
    test_name: &str,
    step_idx: usize,
    cmds: &[DrawCmd<'static>],
    width: u32,
    height: u32,
    texts: &[ScreenText],
) {
    let root = PathBuf::from("target/ui-audit").join(test_name);
    let _ = fs::create_dir_all(&root);

    let png = root.join(format!("step-{step_idx:02}.png"));
    let txt = root.join(format!("step-{step_idx:02}.txt"));

    let mut img = GrayImage::from_pixel(width, height, Luma([255u8]));
    rasterize_cmds(cmds, &mut img);
    let _ = img.save(png);

    let mut lines = String::new();
    for t in texts {
        lines.push_str(&format!("{}:{} {}\n", t.x, t.y, t.text));
    }
    let _ = fs::write(txt, lines);
}

fn rasterize_cmds(cmds: &[DrawCmd<'static>], img: &mut GrayImage) {
    for cmd in cmds {
        match cmd {
            DrawCmd::FillRect { rect, color } => {
                let c = color_to_luma(*color);
                for y in rect.y.max(0) as u32
                    ..(rect.y.max(0) as u32 + rect.height as u32).min(img.height())
                {
                    for x in rect.x.max(0) as u32
                        ..(rect.x.max(0) as u32 + rect.width as u32).min(img.width())
                    {
                        img.put_pixel(x, y, Luma([c]));
                    }
                }
            }
            DrawCmd::DrawLine {
                start, end, color, ..
            } => {
                draw_line(
                    img,
                    start.x as i32,
                    start.y as i32,
                    end.x as i32,
                    end.y as i32,
                    color_to_luma(*color),
                );
            }
            DrawCmd::DrawText { pos, text, .. } => {
                draw_text(img, pos.x as i32, pos.y as i32 - 8, text.as_str(), 0);
            }
            DrawCmd::DrawImage { .. } | DrawCmd::Clip { .. } | DrawCmd::Unclip => {}
        }
    }
}

fn color_to_luma(color: Color) -> u8 {
    match color {
        Color::Black => 0,
        Color::White => 255,
        Color::Gray(v) => v,
        Color::Red | Color::Custom(_) => 0,
    }
}

fn draw_line(img: &mut GrayImage, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: u8) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if x0 >= 0 && y0 >= 0 && (x0 as u32) < img.width() && (y0 as u32) < img.height() {
            img.put_pixel(x0 as u32, y0 as u32, Luma([color]));
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn draw_text(img: &mut GrayImage, x: i32, y: i32, text: &str, color: u8) {
    let mut cursor_x = x;
    for ch in text.chars() {
        if let Some(glyph) = BASIC_FONTS.get(ch) {
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..8 {
                    if (bits >> col) & 1 == 1 {
                        let px = cursor_x + col;
                        let py = y + row as i32;
                        if px >= 0
                            && py >= 0
                            && (px as u32) < img.width()
                            && (py as u32) < img.height()
                        {
                            img.put_pixel(px as u32, py as u32, Luma([color]));
                        }
                    }
                }
            }
        }
        cursor_x += 8;
    }
}

fn load_fixture(name: &str) -> Vec<u8> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../sample_books")
        .join(name);
    fs::read(base).expect("fixture book should exist")
}

#[test]
fn library_and_epub_navigation_regression() {
    let mut files = BTreeMap::new();
    files.insert(
        "books/pg84-frankenstein.epub".to_string(),
        load_fixture("pg84-frankenstein.epub"),
    );
    files.insert("books/sample.txt".to_string(), b"hello\nworld\n".to_vec());

    let mut runtime = EreaderRuntime::with_backends(
        DeviceConfig::xteink_x4(),
        Box::new(TestSettings::default()),
        Box::new(TestFiles::from_map(files)),
    );
    let mut sink = CaptureSink::new();

    let s0 = capture(&mut runtime, &mut sink, None, "library_epub", 0);
    assert!(contains_text(&s0, "Library"));
    let library_rows = s0
        .iter()
        .filter(|t| t.x == 18 && t.y >= 66 && t.y <= 150)
        .count();
    assert_eq!(
        library_rows, 3,
        "library rows should not overlap/wrap into extra lines"
    );

    let s1 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Confirm)),
        "library_epub",
        1,
    );
    assert!(
        !contains_text(&s1, "No readable text produced by renderer.")
            && !contains_text(&s1, "Failed to open EPUB file."),
        "epub render path regressed"
    );
    assert!(contains_text(&s1, "ch "));
    let s1_has_top_content = s1.iter().any(|t| t.y < 40 && !t.text.starts_with("ch "));

    let before = footer_metrics(&s1).expect("epub footer should expose chapter/page metrics");
    let s2 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Right)),
        "library_epub",
        2,
    );
    let after_page = footer_metrics(&s2).expect("epub footer should remain visible after paging");
    let s2_has_top_content = s2.iter().any(|t| t.y < 40 && !t.text.starts_with("ch "));
    assert!(
        s1_has_top_content || s2_has_top_content,
        "epub content should appear near top area on open or next page"
    );
    if before.3 > 1 {
        assert!(
            after_page.2 >= before.2,
            "right press should keep or increase page index"
        );
    }

    let s3 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Aux2)),
        "library_epub",
        3,
    );
    let after_chapter =
        footer_metrics(&s3).expect("epub footer should remain visible after chapter jump");
    assert!(
        after_chapter.0 >= before.0,
        "aux2 should keep or increase chapter index"
    );
}

#[test]
fn settings_and_txt_reader_regression() {
    let mut files = BTreeMap::new();
    files.insert(
        "books/pg84-frankenstein.epub".to_string(),
        load_fixture("pg84-frankenstein.epub"),
    );
    files.insert("books/sample.txt".to_string(), load_fixture("sample.txt"));

    let mut runtime = EreaderRuntime::with_backends(
        DeviceConfig::xteink_x4(),
        Box::new(TestSettings::default()),
        Box::new(TestFiles::from_map(files)),
    );
    let mut sink = CaptureSink::new();

    let s0 = capture(&mut runtime, &mut sink, None, "settings_txt", 0);
    assert!(contains_text(&s0, "Library"));

    let s1 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Right)),
        "settings_txt",
        1,
    );
    let s2 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Right)),
        "settings_txt",
        2,
    );
    let s3 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Right)),
        "settings_txt",
        3,
    );
    assert!(contains_text(&s3, "Settings"));
    assert!(contains_text(&s3, "Font Size: Medium"));
    assert!(contains_text(&s3, "Invert Colors: Off"));

    let s4 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Confirm)),
        "settings_txt",
        4,
    );
    assert!(contains_text(&s4, "Font Size: Large"));

    let s5 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Down)),
        "settings_txt",
        5,
    );
    assert!(contains_text(&s5, "Font Family: Serif"));

    let s6 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Confirm)),
        "settings_txt",
        6,
    );
    assert!(contains_text(&s6, "Font Family: Sans"));
    let _ = (s1, s2);
}

#[test]
fn feeds_modal_flow_regression() {
    let mut files = BTreeMap::new();
    files.insert(
        "books/pg84-frankenstein.epub".to_string(),
        load_fixture("pg84-frankenstein.epub"),
    );

    let mut runtime = EreaderRuntime::with_backends(
        DeviceConfig::xteink_x4(),
        Box::new(TestSettings::default()),
        Box::new(TestFiles::from_map(files)),
    );
    let mut sink = CaptureSink::new();

    let s0 = capture(&mut runtime, &mut sink, None, "feeds_flow", 0);
    assert!(contains_text(&s0, "Library"));
    let s1 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Right)),
        "feeds_flow",
        1,
    );
    let s2 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Right)),
        "feeds_flow",
        2,
    );
    assert!(contains_text(&s2, "Feed"));

    let s3 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Confirm)),
        "feeds_flow",
        3,
    );
    assert!(contains_any(&s3, &["Feed Entries", "Feed Network Required"]));

    let s4 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Confirm)),
        "feeds_flow",
        4,
    );
    assert!(contains_any(&s4, &["Entry:", "Feed Network Required"]));

    let s4b = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Confirm)),
        "feeds_flow",
        5,
    );
    assert!(contains_any(
        &s4b,
        &[
            "Failed to fetch article.",
            "No article URL available for this entry.",
            "URL:",
            "Feed Network Required"
        ]
    ));

    let s5 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Back)),
        "feeds_flow",
        6,
    );
    assert!(contains_any(&s5, &["Feed Entries", "Entry:", "Feed"]));

    let s6 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Back)),
        "feeds_flow",
        7,
    );
    assert!(contains_any(&s6, &["Feed Entries", "Feed"]));
    let _ = s1;
}

#[test]
fn files_tab_txt_open_regression() {
    let mut files = BTreeMap::new();
    files.insert(
        "books/pg84-frankenstein.epub".to_string(),
        load_fixture("pg84-frankenstein.epub"),
    );
    files.insert("books/sample.txt".to_string(), load_fixture("sample.txt"));

    let mut runtime = EreaderRuntime::with_backends(
        DeviceConfig::xteink_x4(),
        Box::new(TestSettings::default()),
        Box::new(TestFiles::from_map(files)),
    );
    let mut sink = CaptureSink::new();

    let s0 = capture(&mut runtime, &mut sink, None, "files_txt", 0);
    assert!(contains_text(&s0, "Library"));

    let s1 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Right)),
        "files_txt",
        1,
    );
    assert!(contains_text(&s1, "Files"));

    let s2 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Down)),
        "files_txt",
        2,
    );
    assert!(contains_text(&s2, "sample.txt"));

    let s3 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Confirm)),
        "files_txt",
        3,
    );
    assert!(contains_text(&s3, "Reader"));
    assert!(contains_text(&s3, "sample.txt"));
    assert!(contains_text(&s3, "Sample Text File for Xteink X4 Testing"));

    let s4 = capture(
        &mut runtime,
        &mut sink,
        Some(InputEvent::Press(Button::Back)),
        "files_txt",
        4,
    );
    assert!(contains_text(&s4, "Files"));
}

//! Texto UI — DejaVu + fontdue::layout (kerning/baseline corretos).

use std::sync::OnceLock;

use fontdue::{
    layout::{CoordinateSystem, HorizontalAlign, Layout, LayoutSettings, TextStyle},
    Font, FontSettings,
};

use super::{raster::Canvas, theme::Rgba};

struct Fonts {
    regular: Font,
    bold: Font,
}

static FONTS: OnceLock<Fonts> = OnceLock::new();
static LAYOUT: OnceLock<std::sync::Mutex<Layout>> = OnceLock::new();

fn fonts() -> &'static Fonts {
    FONTS.get_or_init(|| {
        let regular = Font::from_bytes(
            &include_bytes!("../../assets/DejaVuSans.ttf")[..],
            FontSettings {
                scale: 48.0,
                ..FontSettings::default()
            },
        )
        .expect("DejaVuSans.ttf");
        let bold = Font::from_bytes(
            &include_bytes!("../../assets/DejaVuSans-Bold.ttf")[..],
            FontSettings {
                scale: 48.0,
                ..FontSettings::default()
            },
        )
        .expect("DejaVuSans-Bold.ttf");
        Fonts { regular, bold }
    })
}

fn layout_ctx() -> &'static std::sync::Mutex<Layout> {
    LAYOUT.get_or_init(|| std::sync::Mutex::new(Layout::new(CoordinateSystem::PositiveYDown)))
}

pub fn width(text: &str, size: f32, bold: bool) -> i32 {
    measure(text, size, bold, None, HorizontalAlign::Left).0
}

pub fn draw(c: &mut Canvas, x: i32, y: i32, size: f32, text: &str, color: Rgba) {
    paint(c, x, y, size, text, color, false, None, HorizontalAlign::Left);
}

pub fn draw_bold(c: &mut Canvas, x: i32, y: i32, size: f32, text: &str, color: Rgba) {
    paint(c, x, y, size, text, color, true, None, HorizontalAlign::Left);
}

pub fn draw_centered_in(
    c: &mut Canvas,
    rect_x: i32,
    rect_y: i32,
    rect_w: i32,
    rect_h: i32,
    size: f32,
    text: &str,
    color: Rgba,
) {
    let (tw, th) = measure(text, size, false, Some(rect_w as f32), HorizontalAlign::Center);
    let x = rect_x + (rect_w - tw) / 2;
    let y = rect_y + (rect_h - th) / 2;
    paint(
        c,
        x,
        y,
        size,
        text,
        color,
        false,
        Some(rect_w as f32),
        HorizontalAlign::Center,
    );
}

fn measure(
    text: &str,
    size: f32,
    bold: bool,
    max_width: Option<f32>,
    align: HorizontalAlign,
) -> (i32, i32) {
    let fonts = fonts();
    let font_list = if bold {
        vec![&fonts.bold]
    } else {
        vec![&fonts.regular]
    };
    let mut layout = layout_ctx().lock().unwrap();
    layout.reset(&LayoutSettings {
        x: 0.0,
        y: 0.0,
        max_width,
        horizontal_align: align,
        ..LayoutSettings::default()
    });
    layout.append(&font_list, &TextStyle::new(text, size, 0));

    let mut max_x = 0.0f32;
    let mut max_y = 0.0f32;
    for g in layout.glyphs() {
        max_x = max_x.max(g.x + g.width as f32);
        max_y = max_y.max(g.y + g.height as f32);
    }
    (max_x.ceil() as i32, max_y.ceil() as i32)
}

fn paint(
    c: &mut Canvas,
    x: i32,
    y: i32,
    size: f32,
    text: &str,
    color: Rgba,
    bold: bool,
    max_width: Option<f32>,
    align: HorizontalAlign,
) {
    let fonts = fonts();
    let font_list = if bold {
        vec![&fonts.bold]
    } else {
        vec![&fonts.regular]
    };
    let mut layout = layout_ctx().lock().unwrap();
    layout.reset(&LayoutSettings {
        x: x as f32,
        y: y as f32,
        max_width,
        horizontal_align: align,
        ..LayoutSettings::default()
    });
    layout.append(&font_list, &TextStyle::new(text, size, 0));

    for glyph in layout.glyphs() {
        let font = font_list[glyph.font_index];
        let (metrics, bitmap) = font.rasterize_indexed(glyph.key.glyph_index, glyph.key.px);
        let gw = metrics.width.min(glyph.width);
        let gh = metrics.height.min(glyph.height);
        for row in 0..gh {
            for col in 0..gw {
                let alpha = bitmap[row * metrics.width + col];
                if alpha > 0 {
                    blend(
                        c,
                        glyph.x as i32 + col as i32,
                        glyph.y as i32 + row as i32,
                        color,
                        alpha,
                    );
                }
            }
        }
    }
}

pub fn blend(c: &mut Canvas, x: i32, y: i32, fg: Rgba, alpha: u8) {
    if x < 0 || y < 0 || x >= c.w || y >= c.h {
        return;
    }
    if alpha == 255 {
        c.put(x, y, fg);
        return;
    }
    let i = ((y * c.w + x) * 4) as usize;
    if i + 3 >= c.pixels.len() {
        return;
    }
    let bg = Rgba::new(c.pixels[i + 2], c.pixels[i + 1], c.pixels[i], 255);
    let a = alpha as u16;
    let inv = 255 - a;
    let r = ((fg.r as u16 * a + bg.r as u16 * inv) / 255) as u8;
    let g = ((fg.g as u16 * a + bg.g as u16 * inv) / 255) as u8;
    let b = ((fg.b as u16 * a + bg.b as u16 * inv) / 255) as u8;
    c.put(x, y, Rgba::new(r, g, b, 255));
}

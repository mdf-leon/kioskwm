//! Icones Breeze SVG copiados de /usr/share/icons/breeze.

use std::sync::OnceLock;

use usvg::Tree;

use super::{raster::Canvas, text, theme::{self, Rgba}};

static MOUSE_SVG: &[u8] = include_bytes!("../../assets/icons/input-mouse.svg");
static BACK_SVG: &[u8] = include_bytes!("../../assets/icons/go-previous.svg");

static MOUSE_TREE: OnceLock<Tree> = OnceLock::new();
static BACK_TREE: OnceLock<Tree> = OnceLock::new();

fn parse_svg(bytes: &'static [u8]) -> Tree {
    let opts = usvg::Options::default();
    usvg::Tree::from_data(bytes, &opts).expect("svg breeze")
}

fn mouse_tree() -> &'static Tree {
    MOUSE_TREE.get_or_init(|| parse_svg(MOUSE_SVG))
}

fn back_tree() -> &'static Tree {
    BACK_TREE.get_or_init(|| parse_svg(BACK_SVG))
}

pub fn draw_mouse(c: &mut Canvas, x: i32, y: i32, size: i32) {
    blit_svg(c, mouse_tree(), x, y, size, theme::TEXT);
}

pub fn draw_back(c: &mut Canvas, x: i32, y: i32, size: i32) {
    blit_svg(c, back_tree(), x, y, size, theme::ACCENT);
}

fn blit_svg(c: &mut Canvas, tree: &Tree, x: i32, y: i32, size: i32, tint: Rgba) {
    let ts = tree.size();
    if ts.width() <= 0.0 || ts.height() <= 0.0 {
        return;
    }
    let mut pixmap = match tiny_skia::Pixmap::new(size as u32, size as u32) {
        Some(p) => p,
        None => return,
    };
    let sx = size as f32 / ts.width();
    let sy = size as f32 / ts.height();
    let transform = tiny_skia::Transform::from_scale(sx, sy);
    resvg::render(tree, transform, &mut pixmap.as_mut());

    for row in 0..size {
        for col in 0..size {
            let Some(p) = pixmap.pixel(col as u32, row as u32) else {
                continue;
            };
            if p.alpha() == 0 {
                continue;
            }
            text::blend(c, x + col, y + row, tint, p.alpha());
        }
    }
}

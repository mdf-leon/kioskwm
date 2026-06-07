use std::io::Read;

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::memory::MemoryRenderBuffer,
            gles::GlesRenderer,
        },
    },
    utils::Transform,
};
use tracing::warn;
use xcursor::{parser::parse_xcursor, CursorTheme};

pub struct PointerCursor {
    pub buffer: MemoryRenderBuffer,
    pub hotspot: (i32, i32),
}

impl PointerCursor {
    pub fn load() -> Self {
        match try_load_system_cursor() {
            Ok(cursor) => {
                tracing::info!("Cursor carregado: left_ptr");
                cursor
            }
            Err(err) => {
                warn!("Falha ao carregar cursor do sistema ({err}) — usando fallback");
                fallback_cursor()
            }
        }
    }
}

fn try_load_system_cursor() -> Result<PointerCursor, Box<dyn std::error::Error>> {
    for theme_name in cursor_theme_candidates() {
        if let Ok(cursor) = try_load_from_theme(&theme_name) {
            tracing::debug!("Tema de cursor: {theme_name}");
            return Ok(cursor);
        }
    }
    Err("nenhum tema de cursor encontrado".into())
}

fn cursor_theme_candidates() -> Vec<String> {
    let mut themes = Vec::new();
    for var in ["KIOSKWM_CURSOR_THEME", "XCURSOR_THEME"] {
        if let Ok(name) = std::env::var(var) {
            let name = name.trim().to_string();
            if !name.is_empty() {
                themes.push(name);
            }
        }
    }
    for default in ["Breeze_Light", "breeze_cursors", "Adwaita", "default"] {
        themes.push(default.into());
    }
    themes
}

fn try_load_from_theme(theme_name: &str) -> Result<PointerCursor, Box<dyn std::error::Error>> {
    let theme = CursorTheme::load(theme_name);
    let icon_path = theme
        .load_icon("left_ptr")
        .or_else(|| theme.load_icon("default"))
        .ok_or("ícone left_ptr não encontrado")?;

    let mut file = std::fs::File::open(&icon_path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    let images = parse_xcursor(&data).ok_or("falha ao parsear xcursor")?;
    let size = std::env::var("XCURSOR_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(24);
    let image = nearest_image(size, &images).ok_or("xcursor vazio")?;

    let buffer = MemoryRenderBuffer::from_slice(
        &image.pixels_rgba,
        Fourcc::Argb8888,
        (image.width as i32, image.height as i32),
        1,
        Transform::Normal,
        None,
    );

    Ok(PointerCursor {
        buffer,
        hotspot: (image.xhot as i32, image.yhot as i32),
    })
}

fn nearest_image(size: u32, images: &[xcursor::parser::Image]) -> Option<&xcursor::parser::Image> {
    if images.is_empty() {
        return None;
    }
    let nearest = images
        .iter()
        .min_by_key(|img| (size as i32 - img.size as i32).abs())?;
    images.iter().find(|img| {
        img.width == nearest.width && img.height == nearest.height && img.size == nearest.size
    })
}

fn fallback_cursor() -> PointerCursor {
    // Seta simples 16x16 em RGBA
    let w = 16i32;
    let h = 16i32;
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..=y.min(10) {
            let i = ((y * w + x) * 4) as usize;
            pixels[i] = 255;
            pixels[i + 1] = 255;
            pixels[i + 2] = 255;
            pixels[i + 3] = 255;
        }
    }
    PointerCursor {
        buffer: MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Argb8888,
            (w, h),
            1,
            Transform::Normal,
            None,
        ),
        hotspot: (1, 1),
    }
}

pub fn cursor_location(
    pos: smithay::utils::Point<f64, smithay::utils::Logical>,
    hotspot: (i32, i32),
) -> (f64, f64) {
    ((pos.x - hotspot.0 as f64), (pos.y - hotspot.1 as f64))
}

pub type CursorElement = smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement<GlesRenderer>;

pub fn cursor_element(
    renderer: &mut GlesRenderer,
    cursor: &PointerCursor,
    pos: smithay::utils::Point<f64, smithay::utils::Logical>,
) -> Result<CursorElement, Box<dyn std::error::Error>> {
    let (x, y) = cursor_location(pos, cursor.hotspot);
    Ok(smithay::backend::renderer::element::memory::MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        (x, y),
        &cursor.buffer,
        None,
        None,
        None,
        smithay::backend::renderer::element::Kind::Cursor,
    )?)
}

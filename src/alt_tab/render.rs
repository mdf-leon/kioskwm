use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                Kind,
            },
            gles::GlesRenderer,
        },
    },
    utils::{Point, Rectangle, Size, Transform},
};

use crate::{
    settings::{raster::Canvas, text, theme},
    state::State,
};

use super::layout::{self, icon_rect, tile_rect};

pub struct AltTabElement {
    pub elem: MemoryRenderBufferRenderElement<GlesRenderer>,
}

pub fn prepare_overlay(
    renderer: &mut GlesRenderer,
    state: &mut State,
    output: Size<i32, smithay::utils::Physical>,
    scale: f64,
    time_ms: u32,
) -> Result<Option<AltTabElement>, Box<dyn std::error::Error>> {
    if !state.alt_tab.open {
        return Ok(None);
    }

    rebuild_cache_if_needed(state, time_ms)?;

    let cache = state.alt_tab_cache.lock().unwrap();
    let Some(buf) = cache.as_ref() else {
        return Ok(None);
    };

    let loc = Point::<f64, smithay::utils::Physical>::from((0.0, 0.0));
    let elem = MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        loc,
        &buf.buffer,
        None,
        None,
        None,
        Kind::Unspecified,
    )?;

    let _ = (output, scale);
    Ok(Some(AltTabElement { elem }))
}

fn rebuild_cache_if_needed(state: &mut State, time_ms: u32) -> Result<(), Box<dyn std::error::Error>> {
    let key = cache_key(state, time_ms);
    {
        let guard = state.alt_tab_cache.lock().unwrap();
        if guard.as_ref().is_some_and(|c| c.key == key) {
            return Ok(());
        }
    }

    let w = state.output_size.w;
    let h = state.output_size.h;
    let mut canvas = Canvas::new(w, h);
    paint_overlay(&mut canvas, state, time_ms);
    upload_pixels(state, &canvas, key);
    Ok(())
}

fn paint_overlay(c: &mut Canvas, state: &State, time_ms: u32) {
    let w = state.output_size.w;
    let h = state.output_size.h;
    c.fill(theme::MODAL_SCRIM);

    let order = layout::ordered_indices(state);
    let count = order.len();
    if count == 0 {
        return;
    }

    let (row_x, row_y) = layout::row_origin(w, h, count);
    let (rw, rh) = layout::row_size(count);
    c.bordered_rounded_rect(
        row_x,
        row_y,
        rw,
        rh,
        layout::ROW_RADIUS,
        theme::WINDOW_BG,
        theme::BORDER,
    );

    let selected_slot = state.alt_tab.slot.min(count.saturating_sub(1));
    let blink = layout::blink_phase(time_ms);

    for (slot, &app_idx) in order.iter().enumerate() {
        let tile = tile_rect(row_x, row_y, slot);
        let selected = slot == selected_slot;
        let name = state.unified_app_name(app_idx);
        let letter = layout::app_icon_letter(name);

        let bg = if selected && blink {
            theme::TILE_HOVER
        } else {
            theme::TILE_BG
        };
        c.fill_rounded_rect(tile.x, tile.y, tile.w, tile.h, 8, bg);
        if selected {
            let border = if blink { theme::ACCENT } else { theme::TEXT };
            c.bordered_rounded_rect(tile.x, tile.y, tile.w, tile.h, 8, bg, border);
        } else {
            c.bordered_rounded_rect(tile.x, tile.y, tile.w, tile.h, 8, bg, theme::BORDER);
        }

        let icon = icon_rect(tile);
        c.fill_rounded_rect(icon.x, icon.y, icon.w, icon.h, 10, theme::TILE_ICON_BG);
        let ch = letter.to_string();
        text::draw_bold(
            c,
            icon.x + icon.w / 2 - 10,
            icon.y + icon.h / 2 - 14,
            28.0,
            &ch,
            theme::ACCENT,
        );

        let label = truncate_label(name, 16);
        let tw = text::width(&label, 12.0, false);
        let lx = tile.x + (tile.w - tw) / 2;
        let color = if selected && blink {
            theme::TEXT
        } else if selected {
            theme::ACCENT
        } else {
            theme::TEXT_INACTIVE
        };
        text::draw(c, lx, tile.y + tile.h - 28, 12.0, &label, color);
    }
}

fn truncate_label(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        return name.to_string();
    }
    let mut s: String = name.chars().take(max.saturating_sub(1)).collect();
    s.push('…');
    s
}

fn upload_pixels(state: &mut State, canvas: &Canvas, key: u64) {
    let w = state.output_size.w;
    let h = state.output_size.h;
    let mut guard = state.alt_tab_cache.lock().unwrap();
    if let Some(cache) = guard.as_mut() {
        let mut ctx = cache.buffer.render();
        let _ = ctx.draw(|buf| {
            buf.copy_from_slice(&canvas.pixels);
            Ok::<_, std::convert::Infallible>(vec![Rectangle::from_size(Size::from((w, h)))])
        });
        cache.key = key;
        return;
    }

    let mut buffer = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (w, h),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = buffer.render();
        let _ = ctx.draw(|buf| {
            buf.copy_from_slice(&canvas.pixels);
            Ok::<_, std::convert::Infallible>(vec![Rectangle::from_size(Size::from((w, h)))])
        });
    }
    *guard = Some(crate::state::AltTabCache { buffer, key });
}

fn cache_key(state: &State, time_ms: u32) -> u64 {
    let slot = state.alt_tab.slot as u64;
    let apps = state.app_count() as u64;
    let blink = layout::blink_phase(time_ms) as u64;
    let focus = state.unified_focus_index() as u64;
    let mru_tag = state.app_mru.len() as u64;
    slot | (apps << 8) | (blink << 16) | (focus << 24) | (mru_tag << 32)
}

pub fn invalidate_cache(state: &mut State) {
    if let Some(cache) = state.alt_tab_cache.lock().unwrap().as_mut() {
        cache.key = u64::MAX;
    }
}

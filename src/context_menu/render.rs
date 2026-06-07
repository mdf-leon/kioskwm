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
    i18n::Msg,
    settings::{raster::Canvas, text, theme},
    state::State,
};

use super::layout::{self, Hit};

pub struct MenuElement {
    pub elem: MemoryRenderBufferRenderElement<GlesRenderer>,
}

pub fn prepare_menu(
    renderer: &mut GlesRenderer,
    state: &mut State,
    output: Size<i32, smithay::utils::Physical>,
    scale: f64,
) -> Result<Option<MenuElement>, Box<dyn std::error::Error>> {
    if !state.context_menu.open {
        return Ok(None);
    }

    rebuild_cache_if_needed(state)?;

    let cache = state.context_menu_cache.lock().unwrap();
    let Some(buf) = cache.as_ref() else {
        return Ok(None);
    };

    let (mw, mh) = layout::menu_size(state.app_count());
    let px = (state.context_menu.origin_x as f64 * scale).round() as i32;
    let py = (state.context_menu.origin_y as f64 * scale).round() as i32;
    let loc = Point::<f64, smithay::utils::Physical>::from((px as f64, py as f64));

    let elem = MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        loc,
        &buf.buffer,
        None,
        None,
        None,
        Kind::Unspecified,
    )?;

    let _ = (mw, mh, output);
    Ok(Some(MenuElement { elem }))
}

fn rebuild_cache_if_needed(state: &mut State) -> Result<(), Box<dyn std::error::Error>> {
    let key = cache_key(state);
    {
        let guard = state.context_menu_cache.lock().unwrap();
        if guard.as_ref().is_some_and(|c| c.key == key) {
            return Ok(());
        }
    }

    let (mw, mh) = layout::menu_size(state.app_count());
    let mut canvas = Canvas::new(mw, mh);
    paint_menu(&mut canvas, state);
    upload_pixels(state, &canvas, key);
    Ok(())
}

fn paint_menu(c: &mut Canvas, state: &State) {
    let i18n = state.i18n;
    let hover = state.context_menu.hover;
    let app_count = state.app_count();
    let (mw, mh) = layout::menu_size(app_count);
    let focus_idx = state.unified_focus_index();

    c.bordered_rounded_rect(0, 0, mw, mh, layout::RADIUS, theme::WINDOW_BG, theme::BORDER);

    let header = layout::header_rect(0, 0);
    text::draw_bold(
        c,
        header.x + 8,
        header.y + 6,
        12.0,
        i18n.t(Msg::Main),
        theme::TEXT_INACTIVE,
    );

    for i in 0..app_count {
        let rect = layout::app_rect(0, 0, i);
        let hit = Hit::App(i);
        let focused = i == focus_idx;
        let bg = if hover == Some(hit) {
            theme::BUTTON_HOVER
        } else {
            theme::WINDOW_BG
        };
        c.fill_rounded_rect(rect.x, rect.y, rect.w, rect.h, 4, bg);
        let name = state.unified_app_name(i);
        let label = layout::unified_label(i18n, name, focused);
        let color = if focused { theme::ACCENT } else { theme::TEXT_INACTIVE };
        text::draw(c, rect.x + 8, rect.y + 8, 12.5, &label, color);
    }

    let div_y = layout::divider_y(0, app_count);
    c.hline(layout::PADDING, div_y, mw - layout::PADDING * 2, theme::BORDER);

    if app_count > 0 {
        let close = layout::close_rect(0, 0, app_count);
        let cbg = layout::item_bg(hover, Hit::CloseApp);
        c.fill_rounded_rect(close.x, close.y, close.w, close.h, 4, cbg);
        text::draw(
            c,
            close.x + 12,
            close.y + 8,
            13.0,
            i18n.t(Msg::CloseApp),
            theme::TEXT_INACTIVE,
        );
        let close_div = close.y + close.h;
        c.hline(layout::PADDING, close_div, mw - layout::PADDING * 2, theme::BORDER);
    }

    let settings = layout::settings_rect(0, 0, app_count);
    let sbg = layout::item_bg(hover, Hit::OpenSettings);
    c.fill_rounded_rect(settings.x, settings.y, settings.w, settings.h, 4, sbg);
    text::draw(
        c,
        settings.x + 12,
        settings.y + 8,
        13.0,
        i18n.t(Msg::OpenSettings),
        theme::TEXT,
    );
}

fn upload_pixels(state: &mut State, canvas: &Canvas, key: u64) {
    let (mw, mh) = layout::menu_size(state.app_count());
    let mut guard = state.context_menu_cache.lock().unwrap();
    if let Some(cache) = guard.as_mut() {
        let mut ctx = cache.buffer.render();
        let _ = ctx.draw(|buf| {
            buf.copy_from_slice(&canvas.pixels);
            Ok::<_, std::convert::Infallible>(vec![Rectangle::from_size(Size::from((mw, mh)))])
        });
        cache.key = key;
        return;
    }

    let mut buffer = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (mw, mh),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = buffer.render();
        let _ = ctx.draw(|buf| {
            buf.copy_from_slice(&canvas.pixels);
            Ok::<_, std::convert::Infallible>(vec![Rectangle::from_size(Size::from((mw, mh)))])
        });
    }
    *guard = Some(crate::state::ContextMenuCache { buffer, key });
}

fn cache_key(state: &State) -> u64 {
    let hover = match state.context_menu.hover {
        None | Some(Hit::None) => 0u64,
        Some(Hit::OpenSettings) => 1,
        Some(Hit::CloseApp) => 2,
        Some(Hit::App(i)) => 3 + i as u64,
    };
    let apps = state.app_count() as u64;
    let focus = state.unified_focus_index() as u64;
    let lang = state.i18n.cache_tag() as u64;
    hover | (apps << 8) | (focus << 16) | (lang << 24)
}

pub fn invalidate_cache(state: &mut State) {
    if let Some(cache) = state.context_menu_cache.lock().unwrap().as_mut() {
        cache.key = u64::MAX;
    }
}

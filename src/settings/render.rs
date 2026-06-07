use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                Kind,
            },
            gles::{GlesFrame, GlesRenderer},
        },
    },
    utils::{Point, Rectangle, Size, Transform},
};

use crate::{i18n::{I18n, Msg}, state::State};

use super::{
    icon,
    layout::{self, ConfirmAction, Hit, Screen},
    raster::Canvas,
    slider::{format_speed, t_from_speed},
    text,
    theme::{self, Rgba},
};

pub struct PanelElement {
    pub elem: MemoryRenderBufferRenderElement<GlesRenderer>,
}

pub fn prepare_scrim(
    renderer: &mut GlesRenderer,
    state: &mut State,
    output: Size<i32, smithay::utils::Physical>,
) -> Result<
    Option<MemoryRenderBufferRenderElement<GlesRenderer>>,
    Box<dyn std::error::Error>,
> {
    if !state.overlay_open {
        return Ok(None);
    }

    let key = (output.w, output.h);
    {
        let guard = state.overlay_scrim_cache.lock().unwrap();
        if guard.as_ref().is_some_and(|c| c.size == key) {
            let buf = &guard.as_ref().unwrap().buffer;
            let elem = MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                (0.0, 0.0),
                buf,
                None,
                None,
                None,
                Kind::Unspecified,
            )?;
            return Ok(Some(elem));
        }
    }

    let mut canvas = Canvas::new(output.w, output.h);
    canvas.fill(theme::MODAL_SCRIM);

    let mut buffer = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (output.w, output.h),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = buffer.render();
        let _ = ctx.draw(|buf| {
            buf.copy_from_slice(&canvas.pixels);
            Ok::<_, std::convert::Infallible>(vec![Rectangle::from_size(Size::from((
                output.w,
                output.h,
            )))])
        });
    }

    let elem = MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        (0.0, 0.0),
        &buffer,
        None,
        None,
        None,
        Kind::Unspecified,
    )?;

    *state.overlay_scrim_cache.lock().unwrap() = Some(crate::state::OverlayScrimCache {
        buffer,
        size: key,
    });

    Ok(Some(elem))
}

pub fn prepare_panel(
    renderer: &mut GlesRenderer,
    state: &mut State,
    output: Size<i32, smithay::utils::Physical>,
    scale: f64,
) -> Result<Option<PanelElement>, Box<dyn std::error::Error>> {
    if !state.overlay_open {
        return Ok(None);
    }

    rebuild_cache_if_needed(state)?;

    let cache = state.settings_cache.lock().unwrap();
    let Some(buf) = cache.as_ref() else {
        return Ok(None);
    };

    let pw = (theme::PANEL_W as f64 * scale).round() as i32;
    let ph = (theme::PANEL_H as f64 * scale).round() as i32;
    let px = (output.w - pw) / 2;
    let py = (output.h - ph) / 2;
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

    Ok(Some(PanelElement { elem }))
}

pub fn draw_overlay_extras(
    _frame: &mut GlesFrame<'_, '_>,
    _state: &State,
    _output: Size<i32, smithay::utils::Physical>,
    _scale: f64,
) {
    // Knob do slider vai no canvas (winit usa Transform::Flipped180 e quebrava o draw_solid).
}

fn rebuild_cache_if_needed(state: &mut State) -> Result<(), Box<dyn std::error::Error>> {
    let key = cache_key(state);
    {
        let guard = state.settings_cache.lock().unwrap();
        if guard.as_ref().is_some_and(|c| c.key == key) {
            return Ok(());
        }
    }

    let mut canvas = Canvas::new(theme::PANEL_W, theme::PANEL_H);
    paint_chrome(&mut canvas);

    let hover = state.settings.hover;
    let i18n = state.i18n;

    match state.settings.screen {
        Screen::Main => paint_main(&mut canvas, i18n, hover),
        Screen::Mouse => paint_mouse_static(&mut canvas, i18n, state.pointer_speed, hover),
    }

    if let Some(confirm) = state.settings.confirm {
        paint_confirm(&mut canvas, i18n, confirm, hover);
    }

    upload_pixels(state, &canvas, key);
    Ok(())
}

fn paint_chrome(c: &mut Canvas) {
    c.fill(theme::WINDOW_BG);
    c.bordered_rounded_rect(
        0,
        0,
        theme::PANEL_W,
        theme::PANEL_H,
        theme::PANEL_RADIUS,
        theme::WINDOW_BG,
        theme::BORDER,
    );
}

fn upload_pixels(state: &mut State, canvas: &Canvas, key: u64) {
    let mut guard = state.settings_cache.lock().unwrap();
    if let Some(cache) = guard.as_mut() {
        let mut ctx = cache.buffer.render();
        let _ = ctx.draw(|buf| {
            buf.copy_from_slice(&canvas.pixels);
            Ok::<_, std::convert::Infallible>(vec![Rectangle::from_size(Size::from((
                theme::PANEL_W,
                theme::PANEL_H,
            )))])
        });
        cache.key = key;
        return;
    }

    let mut buffer = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (theme::PANEL_W, theme::PANEL_H),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = buffer.render();
        let _ = ctx.draw(|buf| {
            buf.copy_from_slice(&canvas.pixels);
            Ok::<_, std::convert::Infallible>(vec![Rectangle::from_size(Size::from((
                theme::PANEL_W,
                theme::PANEL_H,
            )))])
        });
    }
    *guard = Some(crate::state::SettingsPanelCache { buffer, key });
}

fn cache_key(state: &State) -> u64 {
    let screen = state.settings.screen as u8 as u64;
    let confirm = match state.settings.confirm {
        None => 0u64,
        Some(ConfirmAction::QuitWm) => 1,
        Some(ConfirmAction::Shutdown) => 2,
        Some(ConfirmAction::Reboot) => 3,
    };
    let speed_q = if state.settings.screen == Screen::Mouse {
        (state.pointer_speed * 5.0).round() as u64
    } else {
        0
    };
    let hover_id = state.settings.hover.map(hit_cache_id).unwrap_or(0);
    let lang = state.i18n.cache_tag() as u64;
    screen | (confirm << 4) | (speed_q << 8) | (hover_id << 12) | (lang << 16)
}

fn hit_cache_id(hit: Hit) -> u64 {
    match hit {
        Hit::None => 0,
        Hit::Close => 1,
        Hit::AppletMouse => 2,
        Hit::FooterQuit => 3,
        Hit::FooterShutdown => 4,
        Hit::FooterReboot => 5,
        Hit::MouseBack => 6,
        Hit::Slider => 7,
        Hit::ConfirmCancel => 8,
        Hit::ConfirmOk => 9,
    }
}

fn paint_close(c: &mut Canvas, hover: Option<Hit>) {
    let r = layout::HEADER_CLOSE;
    if hover == Some(Hit::Close) {
        c.fill_rounded_rect(r.x, r.y, r.w, r.h, 6, theme::CLOSE_HOVER);
    }
    c.draw_close(r.x + 9, r.y + 10, theme::TEXT_INACTIVE);
}

fn paint_header(c: &mut Canvas, title: &str, hover: Option<Hit>) {
    c.draw_hamburger(16, 16, theme::TEXT);
    text::draw_bold(c, 44, 10, 17.0, title, theme::TEXT);
    paint_close(c, hover);
    c.hline(0, theme::HEADER_H - 1, theme::PANEL_W, theme::BORDER);
}

fn paint_sub_header(c: &mut Canvas, i18n: I18n, title: &str, hover: Option<Hit>) {
    let back = layout::MOUSE_BACK;
    if hover == Some(Hit::MouseBack) {
        c.fill_rounded_rect(back.x, back.y, back.w, back.h, 6, theme::BUTTON_HOVER);
    }
    icon::draw_back(c, 16, 15, 18);
    text::draw(c, 38, 14, 12.0, i18n.t(Msg::Back), theme::ACCENT);
    let tw = text::width(title, 17.0, true);
    text::draw_bold(c, (theme::PANEL_W - tw) / 2, 10, 17.0, title, theme::TEXT);
    paint_close(c, hover);
    c.hline(0, theme::HEADER_H - 1, theme::PANEL_W, theme::BORDER);
}

fn paint_main(c: &mut Canvas, i18n: I18n, hover: Option<Hit>) {
    paint_header(c, i18n.t(Msg::Settings), hover);

    text::draw(c, 20, theme::HEADER_H + 10, 11.0, i18n.t(Msg::MostUsedPages), theme::TEXT_INACTIVE);

    let tile = layout::APPLET_MOUSE;
    let tile_bg = if hover == Some(Hit::AppletMouse) {
        theme::TILE_HOVER
    } else {
        theme::TILE_BG
    };
    c.bordered_rounded_rect(tile.x, tile.y, tile.w, tile.h, 6, tile_bg, theme::BORDER);
    icon::draw_mouse(c, tile.x + 10, tile.y + 7, 22);
    text::draw(c, tile.x + 40, tile.y + 9, 13.0, i18n.t(Msg::Mouse), theme::TEXT);

    let footer_y = theme::PANEL_H - theme::FOOTER_H;
    c.hline(0, footer_y, theme::PANEL_W, theme::BORDER);

    let footers = layout::footer_buttons();
    let labels = [
        i18n.t(Msg::FooterQuitWm),
        i18n.t(Msg::FooterShutDown),
        i18n.t(Msg::FooterRestart),
    ];
    let hits = [Hit::FooterQuit, Hit::FooterShutdown, Hit::FooterReboot];
    for (i, rect) in footers.iter().enumerate() {
        let destructive = i > 0;
        let (fill, border) = footer_button_style(hover, hits[i], destructive);
        c.bordered_rounded_rect(rect.x, rect.y, rect.w, rect.h, 4, fill, border);
        let tw = text::width(labels[i], 11.5, false);
        let tc = if destructive { theme::NEGATIVE } else { theme::TEXT };
        text::draw(
            c,
            rect.x + (rect.w - tw) / 2,
            rect.y + 7,
            11.5,
            labels[i],
            tc,
        );
    }
}

fn footer_button_style(hover: Option<Hit>, hit: Hit, destructive: bool) -> (Rgba, Rgba) {
    let hovered = hover == Some(hit);
    if destructive {
        (
            if hovered { theme::NEGATIVE_HOVER } else { theme::BUTTON_BG },
            theme::NEGATIVE,
        )
    } else {
        (
            if hovered { theme::BUTTON_HOVER } else { theme::BUTTON_BG },
            theme::BORDER,
        )
    }
}

fn paint_mouse_static(c: &mut Canvas, i18n: I18n, speed: f64, hover: Option<Hit>) {
    paint_sub_header(c, i18n, i18n.t(Msg::Mouse), hover);

    text::draw(c, 32, 72, 13.0, i18n.t(Msg::PointerSpeed), theme::TEXT);

    let track = layout::mouse_slider();
    c.fill_rounded_rect(track.x, track.y, track.w, track.h, 2, theme::SLIDER_TRACK);
    let mid = track.x + track.w / 2;
    c.fill_rect(mid, track.y - 5, 1, track.h + 10, theme::SLIDER_TICK);

    let t = t_from_speed(speed);
    let kx = track.x + (track.w as f64 * t).round() as i32;
    let ky = track.y + track.h / 2;
    c.fill_circle(kx, ky, 9, theme::KNOB_FILL, theme::ACCENT);

    text::draw(
        c,
        track.x,
        track.y + 16,
        10.0,
        layout::SLIDER_LABEL_LEFT,
        theme::TEXT_INACTIVE,
    );
    let cw = text::width(layout::SLIDER_LABEL_CENTER, 10.0, false);
    text::draw(
        c,
        mid - cw / 2,
        track.y + 16,
        10.0,
        layout::SLIDER_LABEL_CENTER,
        theme::TEXT_INACTIVE,
    );
    let rw = text::width(layout::SLIDER_LABEL_RIGHT, 10.0, false);
    text::draw(
        c,
        track.x + track.w - rw,
        track.y + 16,
        10.0,
        layout::SLIDER_LABEL_RIGHT,
        theme::TEXT_INACTIVE,
    );

    let val = format_speed(speed);
    let vw = text::width(&val, 20.0, true);
    text::draw_bold(c, (theme::PANEL_W - vw) / 2, 228, 20.0, &val, theme::ACCENT);

    let footer_y = theme::PANEL_H - theme::MOUSE_FOOTER_H;
    c.hline(0, footer_y, theme::PANEL_W, theme::BORDER);
    text::draw(
        c,
        20,
        footer_y + 12,
        10.0,
        i18n.t(Msg::MouseFooterHint),
        theme::TEXT_INACTIVE,
    );
}

fn paint_confirm(c: &mut Canvas, i18n: I18n, action: ConfirmAction, hover: Option<Hit>) {
    c.fill_rect(0, 0, theme::PANEL_W, theme::PANEL_H, theme::MODAL_SCRIM);

    let modal = layout::confirm_modal();
    c.bordered_rounded_rect(modal.x, modal.y, modal.w, modal.h, 6, theme::MODAL_BG, theme::BORDER);

    let (title, body) = i18n.confirm_dialog(action);

    text::draw_bold(c, modal.x + 20, modal.y + 24, 15.0, title, theme::TEXT);
    text::draw(c, modal.x + 20, modal.y + 52, 11.5, body, theme::TEXT_INACTIVE);

    let (cancel, ok) = layout::confirm_buttons(modal);
    let (cfill, cborder) = footer_button_style(hover, Hit::ConfirmCancel, false);
    c.bordered_rounded_rect(cancel.x, cancel.y, cancel.w, cancel.h, 4, cfill, cborder);
    text::draw(c, cancel.x + 22, cancel.y + 7, 11.5, i18n.t(Msg::Cancel), theme::TEXT);
    let (ofill, oborder) = footer_button_style(hover, Hit::ConfirmOk, true);
    c.bordered_rounded_rect(ok.x, ok.y, ok.w, ok.h, 4, ofill, oborder);
    text::draw(c, ok.x + 16, ok.y + 7, 11.5, i18n.t(Msg::Confirm), theme::TEXT);
}

pub fn invalidate_cache(state: &mut State) {
    if let Some(cache) = state.settings_cache.lock().unwrap().as_mut() {
        cache.key = u64::MAX;
    }
}

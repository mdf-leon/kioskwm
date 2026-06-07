use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                Kind,
            },
            gles::{GlesFrame, GlesRenderer},
            Color32F,
        },
    },
    utils::{Point, Rectangle, Size, Transform},
};

use crate::state::State;

use super::{
    layout::{self, ConfirmAction, Screen},
    raster::Canvas,
    slider::{format_speed, t_from_speed},
    theme,
};

pub struct PanelElement {
    pub elem: MemoryRenderBufferRenderElement<GlesRenderer>,
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

/// Knob do slider — GPU barato, atualiza a cada frame sem rerasterizar o painel.
pub fn draw_overlay_extras(
    frame: &mut GlesFrame<'_, '_>,
    state: &State,
    output: Size<i32, smithay::utils::Physical>,
    scale: f64,
) {
    if !state.overlay_open
        || state.settings.screen != Screen::Mouse
        || state.settings.confirm.is_some()
    {
        return;
    }

    let pw = (theme::PANEL_W as f64 * scale).round() as i32;
    let ph = (theme::PANEL_H as f64 * scale).round() as i32;
    let px = (output.w - pw) / 2;
    let py = (output.h - ph) / 2;

    let track = layout::mouse_slider();
    let t = t_from_speed(state.pointer_speed);
    let kx = px + ((track.x as f64 + track.w as f64 * t) * scale).round() as i32;
    let ky = py + ((track.y as f64 - 5.0) * scale).round() as i32;
    let ks = (16.0 * scale).round() as i32;

    let border = Color32F::new(0.24, 0.68, 0.91, 1.0);
    let fill = Color32F::new(0.16, 0.17, 0.19, 1.0);
    draw_phys_rect(frame, kx - ks / 2, ky, ks, ks, border);
    draw_phys_rect(frame, kx - ks / 2 + 2, ky + 2, ks - 4, ks - 4, fill);
}

fn draw_phys_rect(frame: &mut GlesFrame<'_, '_>, x: i32, y: i32, w: i32, h: i32, color: Color32F) {
    if w <= 0 || h <= 0 {
        return;
    }
    let dest = Rectangle::new(Point::from((x, y)), Size::from((w, h)));
    let damage = Rectangle::from_size(dest.size);
    let _ = frame.draw_solid(dest, &[damage], color);
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
    canvas.fill(theme::WINDOW_BG);
    canvas.fill_rect(0, 0, theme::PANEL_W, 3, theme::ACCENT);

    match state.settings.screen {
        Screen::Main => paint_main(&mut canvas),
        Screen::Mouse => paint_mouse_static(&mut canvas, state.pointer_speed),
    }

    if let Some(confirm) = state.settings.confirm {
        paint_confirm(&mut canvas, confirm);
    }

    upload_pixels(state, &canvas, key);
    Ok(())
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

/// Velocidade fora da chave — knob/texto extras; cache so muda tela/confirm.
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
    screen | (confirm << 4) | (speed_q << 8)
}

fn paint_main(c: &mut Canvas) {
    c.draw_hamburger(20, 18, theme::TEXT);
    c.text(52, 16, "Ajustes rapidos", theme::TEXT, 2);
    c.hline(0, theme::HEADER_H - 1, theme::PANEL_W, theme::BORDER);

    c.text(24, 88, "Paginas mais usadas", theme::TEXT_INACTIVE, 1);

    let row = layout::APPLET_MOUSE;
    c.bordered_rounded_rect(row.x, row.y, row.w, row.h, 6, theme::BUTTON_BG, theme::BORDER);
    c.draw_breeze_mouse_icon(row.x + 14, row.y + 12);
    c.text(row.x + 48, row.y + 16, "Mouse", theme::TEXT, 2);

    let footer_y = theme::PANEL_H - theme::FOOTER_H;
    c.fill_rect(0, footer_y, theme::PANEL_W, 1, theme::BORDER);
    c.fill_rect(0, footer_y + 1, theme::PANEL_W, theme::FOOTER_H - 1, theme::HEADER_BG);
    c.text(24, footer_y + 10, "Sistema", theme::TEXT_INACTIVE, 1);

    let footers = layout::footer_buttons();
    let labels = ["Fechar WM", "Desligar", "Reiniciar"];
    for (i, rect) in footers.iter().enumerate() {
        let border = if i == 0 {
            theme::BORDER
        } else {
            theme::NEGATIVE
        };
        c.bordered_rounded_rect(rect.x, rect.y, rect.w, rect.h, 6, theme::BUTTON_BG, border);
        let tw = Canvas::text_width(labels[i], 1);
        let tc = if i == 0 { theme::TEXT } else { theme::NEGATIVE };
        c.text(rect.x + (rect.w - tw) / 2, rect.y + 12, labels[i], tc, 1);
    }
}

fn paint_mouse_static(c: &mut Canvas, speed: f64) {
    c.text(24, 16, "Mouse", theme::TEXT, 2);
    c.hline(0, theme::HEADER_H - 1, theme::PANEL_W, theme::BORDER);

    let back = layout::MOUSE_BACK;
    c.bordered_rounded_rect(back.x, back.y, back.w, back.h, 6, theme::BUTTON_BG, theme::BORDER);
    c.text(back.x + 12, back.y + 10, "< Voltar", theme::ACCENT, 1);

    c.text(48, 108, "Velocidade do ponteiro", theme::TEXT, 2);
    c.text(
        48,
        134,
        "Centro = 1x   Esquerda = 0.01x   Direita = 4x",
        theme::TEXT_INACTIVE,
        1,
    );

    let track = layout::mouse_slider();
    c.fill_rounded_rect(track.x, track.y, track.w, track.h, 3, theme::SLIDER_TRACK);
    let mid = track.x + track.w / 2;
    c.fill_rect(mid, track.y - 6, 1, track.h + 12, theme::SLIDER_TICK);

    c.text(track.x, track.y + 16, layout::SLIDER_LABEL_LEFT, theme::TEXT_INACTIVE, 1);
    let cw = Canvas::text_width(layout::SLIDER_LABEL_CENTER, 1);
    c.text(mid - cw / 2, track.y + 16, layout::SLIDER_LABEL_CENTER, theme::TEXT_INACTIVE, 1);
    let rw = Canvas::text_width(layout::SLIDER_LABEL_RIGHT, 1);
    c.text(track.x + track.w - rw, track.y + 16, layout::SLIDER_LABEL_RIGHT, theme::TEXT_INACTIVE, 1);

    let val = format_speed(speed);
    let vw = Canvas::text_width(&val, 2);
    c.text((theme::PANEL_W - vw) / 2, 188, &val, theme::ACCENT, 2);

    let footer_y = theme::PANEL_H - theme::MOUSE_FOOTER_H;
    c.fill_rect(0, footer_y, theme::PANEL_W, 1, theme::BORDER);
    c.fill_rect(0, footer_y + 1, theme::PANEL_W, theme::MOUSE_FOOTER_H - 1, theme::VIEW_BG);
    c.text(24, footer_y + 14, "Arraste o controle ou use +/- e setas.", theme::TEXT_INACTIVE, 1);
    c.text(24, footer_y + 32, "Esc ou Voltar: menu principal.", theme::TEXT_INACTIVE, 1);
    c.text(
        24,
        footer_y + 50,
        "Ctrl+Alt+Del / Ctrl+Shift+Esc / Super+Esc: fechar ajustes.",
        theme::TEXT_INACTIVE,
        1,
    );
}

fn paint_confirm(c: &mut Canvas, action: ConfirmAction) {
    c.fill_rect(0, 0, theme::PANEL_W, theme::PANEL_H, theme::MODAL_SCRIM);

    let modal = layout::confirm_modal();
    c.bordered_rounded_rect(modal.x, modal.y, modal.w, modal.h, 8, theme::MODAL_BG, theme::BORDER);
    c.fill_rect(modal.x, modal.y, modal.w, 3, theme::ACCENT);

    let (title, body) = match action {
        ConfirmAction::QuitWm => ("Fechar kioskwm?", "O compositor Wayland sera encerrado."),
        ConfirmAction::Shutdown => ("Desligar o computador?", "Todos os programas serao fechados."),
        ConfirmAction::Reboot => ("Reiniciar o computador?", "Todos os programas serao fechados."),
    };

    c.text(modal.x + 24, modal.y + 24, title, theme::TEXT, 2);
    c.text(modal.x + 24, modal.y + 56, body, theme::TEXT_INACTIVE, 1);

    let (cancel, ok) = layout::confirm_buttons(modal);
    c.bordered_rounded_rect(cancel.x, cancel.y, cancel.w, cancel.h, 6, theme::BUTTON_BG, theme::BORDER);
    c.text(cancel.x + 28, cancel.y + 12, "Cancelar", theme::TEXT, 1);
    c.bordered_rounded_rect(ok.x, ok.y, ok.w, ok.h, 6, theme::NEGATIVE, theme::NEGATIVE);
    c.text(ok.x + 22, ok.y + 12, "Confirmar", theme::TEXT, 1);
}

pub fn invalidate_cache(state: &mut State) {
    if let Some(cache) = state.settings_cache.lock().unwrap().as_mut() {
        cache.key = u64::MAX;
    }
}

//! Painel Ctrl+Alt+Del — modo DEBUG: quadrado 500x500 com "OI" no centro.

use std::sync::atomic::{AtomicBool, Ordering};

use smithay::{
    backend::renderer::{gles::GlesFrame, Color32F, Frame},
    utils::{Point, Rectangle, Size},
};

use crate::state::State;

pub const PANEL_W: i32 = 500;
pub const PANEL_H: i32 = 500;

pub struct OverlayControl {
    toggle_requested: AtomicBool,
}

impl OverlayControl {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            toggle_requested: AtomicBool::new(false),
        })
    }

    /// Thread evdev / SIGUSR2 — pedido assincrono.
    pub fn request_toggle(&self) {
        self.toggle_requested.store(true, Ordering::SeqCst);
        self.notify_changed();
    }

    /// Filtro do compositor — aplica no mesmo tick do teclado.
    pub fn toggle_now(&self, state: &mut State) {
        state.overlay_open = !state.overlay_open;
        tracing::info!(
            "Painel P1 {} (overlay_open={})",
            if state.overlay_open { "aberto" } else { "fechado" },
            state.overlay_open
        );
        self.notify_changed();
    }

    pub fn notify_changed(&self) {
        unsafe {
            libc::raise(libc::SIGUSR2);
        }
    }

    pub fn poll(&self, state: &mut State) {
        if self.toggle_requested.swap(false, Ordering::SeqCst) {
            state.overlay_open = !state.overlay_open;
            tracing::info!(
                "Painel P1 {} (overlay_open={})",
                if state.overlay_open { "aberto" } else { "fechado" },
                state.overlay_open
            );
        }
    }
}

/// Painel 500x500 opaco no centro + "OI". P1 no render (cursor P0 vem depois).
pub fn draw_debug_overlay(
    frame: &mut GlesFrame<'_, '_>,
    output: Size<i32, smithay::utils::Physical>,
    scale: f64,
) {
    let pw = (PANEL_W as f64 * scale).round() as i32;
    let ph = (PANEL_H as f64 * scale).round() as i32;
    let px = (output.w - pw) / 2;
    let py = (output.h - ph) / 2;

    tracing::trace!(
        "draw_debug_overlay phys={}x{} panel=({px}, {py}) {pw}x{ph}",
        output.w,
        output.h,
    );

    // Laranja forte — impossivel confundir com o Konsole.
    draw_physical_rect(
        frame,
        px,
        py,
        pw,
        ph,
        Color32F::new(1.0, 0.45, 0.05, 1.0),
    );

    let border = (6.0 * scale).round() as i32;
    let border_color = Color32F::new(0.1, 0.1, 0.1, 1.0);
    draw_physical_rect(frame, px, py, pw, border, border_color);
    draw_physical_rect(frame, px, py + ph - border, pw, border, border_color);
    draw_physical_rect(frame, px, py, border, ph, border_color);
    draw_physical_rect(frame, px + pw - border, py, border, ph, border_color);

    draw_block_oi(frame, px, py, pw, ph, scale);
}

fn draw_physical_rect(
    frame: &mut GlesFrame<'_, '_>,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: Color32F,
) {
    if w <= 0 || h <= 0 {
        return;
    }
    let dest = Rectangle::new(
        Point::<i32, smithay::utils::Physical>::from((x, y)),
        Size::from((w, h)),
    );
    // damage e relativo a dest — loc absoluto duplicava o offset e mandava o painel para fora da tela.
    let damage = Rectangle::from_size(dest.size);
    let _ = frame.draw_solid(dest, &[damage], color);
}

fn draw_block_oi(
    frame: &mut GlesFrame<'_, '_>,
    px: i32,
    py: i32,
    pw: i32,
    ph: i32,
    scale: f64,
) {
    let bar = (28.0 * scale).round() as i32;
    let letter_h = (200.0 * scale).round() as i32;
    let letter_w = (160.0 * scale).round() as i32;
    let gap = (80.0 * scale).round() as i32;
    let margin = (70.0 * scale).round() as i32;

    let oy = py + (ph - letter_h) / 2;
    let ox = px + margin;
    let ink = Color32F::new(0.05, 0.05, 0.55, 1.0);

    // O
    draw_physical_rect(frame, ox, oy, letter_w, bar, ink);
    draw_physical_rect(frame, ox, oy + letter_h - bar, letter_w, bar, ink);
    draw_physical_rect(frame, ox, oy, bar, letter_h, ink);
    draw_physical_rect(frame, ox + letter_w - bar, oy, bar, letter_h, ink);

    // I
    let ix = ox + letter_w + gap;
    let serif = (50.0 * scale).round() as i32;
    draw_physical_rect(frame, ix, oy, bar, letter_h, ink);
    draw_physical_rect(frame, ix - serif / 2, oy, bar + serif, bar, ink);
    draw_physical_rect(frame, ix - serif / 2, oy + letter_h - bar, bar + serif, bar, ink);
}

#[derive(Clone)]
pub struct DiscoveredTool {
    pub label: String,
    pub command: String,
    pub args: Vec<String>,
    pub note: String,
}

pub fn adjust_speed(_state: &mut State, _delta: f64) {}
pub fn first_tool() -> Option<DiscoveredTool> {
    None
}
pub fn launch_tool(_tool: &DiscoveredTool) -> bool {
    false
}

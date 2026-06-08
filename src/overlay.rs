//! Painel P1 (Ctrl+Alt+Del / Ctrl+Shift+Esc / Super+Esc / Super+Del) — menu estilo Ajustes do macOS.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::{settings::input, state::State};

pub const PANEL_W: i32 = crate::settings::theme::PANEL_W;
pub const PANEL_H: i32 = crate::settings::theme::PANEL_H;

pub struct OverlayControl {
    toggle_requested: AtomicBool,
    /// TTY: acorda o calloop via SIGUSR2. Winit: o main loop já faz poll a cada frame.
    wake_loop: bool,
}

impl OverlayControl {
    pub fn new() -> std::sync::Arc<Self> {
        Self::with_loop_wake(false)
    }

    pub fn with_loop_wake(wake: bool) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            toggle_requested: AtomicBool::new(false),
            wake_loop: wake,
        })
    }

    pub fn request_toggle(&self) {
        self.toggle_requested.store(true, Ordering::SeqCst);
        self.notify_changed();
    }

    pub fn toggle_now(&self, state: &mut State) {
        state.overlay_open = !state.overlay_open;
        if state.overlay_open {
            state.suspend_client_keyboard_for_wm_ui();
            input::reset_on_open(state);
            state.invalidate_wm_backdrop();
        } else {
            state.clear_wm_backdrop();
            state.resync_input_after_overlay();
        }
        tracing::info!(
            "Painel P1 {} (overlay_open={})",
            if state.overlay_open { "aberto" } else { "fechado" },
            state.overlay_open
        );
        state.note_full_damage();
        state.request_render();
        self.notify_changed();
    }

    pub fn notify_changed(&self) {
        if self.wake_loop {
            unsafe {
                libc::raise(libc::SIGUSR2);
            }
        }
    }

    pub fn poll(&self, state: &mut State) {
        if self.toggle_requested.swap(false, Ordering::SeqCst) {
            state.overlay_open = !state.overlay_open;
            if state.overlay_open {
                state.suspend_client_keyboard_for_wm_ui();
                input::reset_on_open(state);
                state.invalidate_wm_backdrop();
            } else {
                state.clear_wm_backdrop();
                state.resync_input_after_overlay();
            }
            tracing::info!(
                "Painel P1 {} (overlay_open={})",
                if state.overlay_open { "aberto" } else { "fechado" },
                state.overlay_open
            );
            state.note_full_damage();
            state.request_render();
        }
    }
}

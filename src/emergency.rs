//! Atalhos de emergencia — P0 encerra, P1 abre painel. Usado pelo compositor e pelo thread evdev.

use std::sync::{atomic::AtomicBool, Arc};

use smithay::input::keyboard::{FilterResult, KeysymHandle, ModifiersState};
use xkbcommon::xkb::keysyms;

use crate::{
    overlay::OverlayControl,
    state::{request_exit, State},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmergencyAction {
    /// P0: encerrar compositor (Ctrl+Alt+Shift+Del)
    ForceQuit,
    /// P1: painel (Ctrl+Alt+Del)
    ToggleOverlay,
    /// Troca de VT (Ctrl+Alt+F1–F12)
    SwitchVt(u8),
}

pub struct EmergencyContext {
    pub exit_flag: Arc<AtomicBool>,
    pub overlay: Arc<OverlayControl>,
}

impl EmergencyContext {
    pub fn new(exit_flag: Arc<AtomicBool>, overlay: Arc<OverlayControl>) -> Self {
        Self { exit_flag, overlay }
    }
}

pub fn match_combo(modifiers: &ModifiersState, keysym: &KeysymHandle<'_>) -> Option<EmergencyAction> {
    if !(modifiers.ctrl || modifiers.logo) || !modifiers.alt {
        return None;
    }
    let sym = keysym.modified_sym().raw();
    if modifiers.shift {
        if is_delete(sym) {
            return Some(EmergencyAction::ForceQuit);
        }
        return None;
    }
    if is_delete(sym) {
        return Some(EmergencyAction::ToggleOverlay);
    }
    vt_from_keysym(sym).map(EmergencyAction::SwitchVt)
}

fn is_delete(sym: u32) -> bool {
    sym == keysyms::KEY_Delete as u32 || sym == keysyms::KEY_KP_Delete as u32
}

fn vt_from_keysym(sym: u32) -> Option<u8> {
    match sym {
        k if k == keysyms::KEY_F1 as u32 => Some(1),
        k if k == keysyms::KEY_F2 as u32 => Some(2),
        k if k == keysyms::KEY_F3 as u32 => Some(3),
        k if k == keysyms::KEY_F4 as u32 => Some(4),
        k if k == keysyms::KEY_F5 as u32 => Some(5),
        k if k == keysyms::KEY_F6 as u32 => Some(6),
        k if k == keysyms::KEY_F7 as u32 => Some(7),
        k if k == keysyms::KEY_F8 as u32 => Some(8),
        k if k == keysyms::KEY_F9 as u32 => Some(9),
        k if k == keysyms::KEY_F10 as u32 => Some(10),
        k if k == keysyms::KEY_F11 as u32 => Some(11),
        k if k == keysyms::KEY_F12 as u32 => Some(12),
        _ => None,
    }
}

/// Libera grabs/foco dos clientes para o painel ficar por cima e receber teclas.
pub fn seize_for_overlay(state: &mut State) {
    if state.pointer.is_grabbed() {
        let pointer = state.pointer.clone();
        let serial = state.next_serial();
        pointer.unset_grab(state, serial, 0);
    }
    let keyboard = state.keyboard.clone();
    let serial = state.next_serial();
    keyboard.set_focus(state, None, serial);
    tracing::info!("Painel P1: input dos clientes bloqueado");
}

pub fn execute(action: EmergencyAction, ctx: &EmergencyContext, state: &mut State) {
    match action {
        EmergencyAction::ForceQuit => force_quit(&ctx.exit_flag),
        EmergencyAction::ToggleOverlay => {
            state.overlay_open = !state.overlay_open;
            tracing::info!(
                "Painel P1 {}",
                if state.overlay_open { "aberto" } else { "fechado" }
            );
            if state.overlay_open {
                seize_for_overlay(state);
            }
            ctx.overlay.notify_changed();
        }
        EmergencyAction::SwitchVt(vt) => switch_vt(vt),
    }
}

pub fn force_quit(exit_flag: &AtomicBool) {
    tracing::error!("P0 KILL — encerrando compositor");
    request_exit(exit_flag);
    unsafe {
        libc::raise(libc::SIGUSR1);
    }
    std::thread::sleep(std::time::Duration::from_millis(500));
    tracing::error!("P0 KILL — _exit");
    unsafe {
        libc::_exit(0);
    }
}

pub fn switch_vt(vt: u8) {
    tracing::info!("Trocando para tty{vt}");
    const VT_ACTIVATE: libc::c_ulong = 0x5606;
    if let Ok(tty0) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty0")
    {
        use std::os::fd::AsRawFd;
        let ret = unsafe {
            libc::ioctl(
                tty0.as_raw_fd(),
                VT_ACTIVATE,
                vt as libc::c_ulong,
            )
        };
        if ret == 0 {
            return;
        }
    }
    std::thread::spawn(move || {
        let _ = std::process::Command::new("chvt")
            .arg(vt.to_string())
            .status();
    });
}

/// Filtro de teclado do compositor — roda ANTES de qualquer cliente Wayland (Konsole, etc.).
pub fn compositor_keyboard_filter(
    state: &mut State,
    ctx: &EmergencyContext,
    modifiers: &ModifiersState,
    keysym: KeysymHandle<'_>,
    key_state: smithay::backend::input::KeyState,
) -> FilterResult<()> {
    use smithay::backend::input::KeyState;

    if key_state == KeyState::Pressed {
        if let Some(action) = match_combo(modifiers, &keysym) {
            execute(action, ctx, state);
            return FilterResult::Intercept(());
        }
    }

    if state.overlay_open {
        return overlay_panel_filter(state, modifiers, &keysym, key_state);
    }

    FilterResult::Forward
}

fn overlay_panel_filter(
    state: &mut State,
    _modifiers: &ModifiersState,
    keysym: &KeysymHandle<'_>,
    key_state: smithay::backend::input::KeyState,
) -> FilterResult<()> {
    use smithay::backend::input::KeyState;

    if key_state != KeyState::Pressed {
        return FilterResult::Intercept(());
    }
    let sym = keysym.modified_sym().raw();
    match sym {
        k if k == keysyms::KEY_Escape as u32 || k == keysyms::KEY_Return as u32 => {
            state.overlay_open = false;
            FilterResult::Intercept(())
        }
        k if k == keysyms::KEY_Left as u32
            || k == keysyms::KEY_Down as u32
            || k == keysyms::KEY_minus as u32
            || k == keysyms::KEY_KP_Subtract as u32 =>
        {
            crate::overlay::adjust_speed(state, -0.25);
            FilterResult::Intercept(())
        }
        k if k == keysyms::KEY_Right as u32
            || k == keysyms::KEY_Up as u32
            || k == keysyms::KEY_plus as u32
            || k == keysyms::KEY_equal as u32
            || k == keysyms::KEY_KP_Add as u32 =>
        {
            crate::overlay::adjust_speed(state, 0.25);
            FilterResult::Intercept(())
        }
        k if k == keysyms::KEY_o as u32 || k == keysyms::KEY_O as u32 => {
            if let Some(tool) = crate::overlay::first_tool() {
                crate::overlay::launch_tool(&tool);
            }
            FilterResult::Intercept(())
        }
        _ => FilterResult::Intercept(()),
    }
}

/// Evdev: mesma logica P0/P1 para quando o thread consegue ler hardware direto.
pub fn match_evdev(ctrl: bool, alt: bool, shift: bool, pressed: bool, code: u16) -> Option<EmergencyAction> {
    if !pressed || !ctrl || !alt {
        return None;
    }
    const KEY_DELETE: u16 = 111;
    const KEY_F1: u16 = 59;
    const KEY_F12: u16 = 70;
    if shift && code == KEY_DELETE {
        return Some(EmergencyAction::ForceQuit);
    }
    if !shift && code == KEY_DELETE {
        return Some(EmergencyAction::ToggleOverlay);
    }
    if !shift && (KEY_F1..=KEY_F12).contains(&code) {
        return Some(EmergencyAction::SwitchVt((code - KEY_F1 + 1) as u8));
    }
    None
}

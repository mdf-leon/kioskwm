//! Prioridades de atalho:
//! - P0: cursor sempre por cima; Ctrl+Alt+Shift+Del encerra; Ctrl+Alt+F1–F12 ou Ctrl+Alt+0–9 troca VT.
//! - P1: Ctrl+Alt+Del (painel) — interceptado no compositor antes dos clientes.

use std::{
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use calloop::channel::Sender;
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
    /// P0: troca de VT (Ctrl+Alt+F1–F12 ou Ctrl+Alt+0–9)
    SwitchVt(u8),
}

static LAST_P1_MS: AtomicU64 = AtomicU64::new(0);
static LAST_P0_VT_MS: AtomicU64 = AtomicU64::new(0);
const P1_DEBOUNCE_MS: u64 = 250;
const P0_VT_DEBOUNCE_MS: u64 = 200;

/// Evita que compositor + thread evdev executem o mesmo P1 duas vezes na mesma tecla.
pub fn try_p1_debounce() -> bool {
    debounce_ms(&LAST_P1_MS, P1_DEBOUNCE_MS)
}

/// Evita duplo envio compositor + evdev na mesma tecla (nao compartilha debounce com P1).
pub fn try_p0_vt_debounce() -> bool {
    debounce_ms(&LAST_P0_VT_MS, P0_VT_DEBOUNCE_MS)
}

fn debounce_ms(slot: &AtomicU64, window_ms: u64) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let prev = slot.swap(now, Ordering::SeqCst);
    now.saturating_sub(prev) >= window_ms
}

pub struct EmergencyContext {
    pub exit_flag: Arc<AtomicBool>,
    pub overlay: Arc<OverlayControl>,
    vt_sender: Mutex<Option<Sender<u8>>>,
}

impl EmergencyContext {
    pub fn new(exit_flag: Arc<AtomicBool>, overlay: Arc<OverlayControl>) -> Self {
        Self {
            exit_flag,
            overlay,
            vt_sender: Mutex::new(None),
        }
    }

    pub fn set_vt_sender(&self, tx: Sender<u8>) {
        *self.vt_sender.lock().unwrap() = Some(tx);
    }

    pub fn request_vt_switch(&self, vt: u8) {
        if let Some(tx) = self.vt_sender.lock().unwrap().as_ref() {
            if tx.send(vt).is_ok() {
                tracing::info!("P0 — pedido de troca para tty{vt} (main loop)");
                return;
            }
            tracing::warn!("canal VT cheio — tentando chvt direto");
        }
        do_vt_switch(vt);
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
        k if k == keysyms::KEY_1 as u32 => Some(1),
        k if k == keysyms::KEY_2 as u32 => Some(2),
        k if k == keysyms::KEY_3 as u32 => Some(3),
        k if k == keysyms::KEY_4 as u32 => Some(4),
        k if k == keysyms::KEY_5 as u32 => Some(5),
        k if k == keysyms::KEY_6 as u32 => Some(6),
        k if k == keysyms::KEY_7 as u32 => Some(7),
        k if k == keysyms::KEY_8 as u32 => Some(8),
        k if k == keysyms::KEY_9 as u32 => Some(9),
        k if k == keysyms::KEY_0 as u32 => Some(10),
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
            if try_p1_debounce() {
                ctx.overlay.toggle_now(state);
            }
        }
        EmergencyAction::SwitchVt(vt) => {
            if try_p0_vt_debounce() {
                ctx.request_vt_switch(vt);
            }
        }
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

/// Troca de VT no kernel (chamar apos pausar DRM no thread principal).
pub fn do_vt_switch(vt: u8) {
    tracing::info!("P0 — ativando tty{vt}");
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
            tracing::info!("ioctl VT_ACTIVATE tty{vt} ok");
            return;
        }
        tracing::warn!(
            "ioctl VT_ACTIVATE falhou: {}",
            std::io::Error::last_os_error()
        );
    }
    match Command::new("chvt").arg(vt.to_string()).status() {
        Ok(status) if status.success() => tracing::info!("chvt {vt} ok"),
        Ok(status) => tracing::warn!("chvt {vt} retornou {status}"),
        Err(err) => tracing::warn!("chvt indisponivel: {err}"),
    }
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

    // P1 (e P0) sempre antes do filtro do painel e antes dos clientes Wayland.
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
    // DEBUG: so Esc fecha o painel.
    if sym == keysyms::KEY_Escape as u32 {
        state.overlay_open = false;
    }
    FilterResult::Intercept(())
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
    const KEY_1: u16 = 2;
    const KEY_9: u16 = 10;
    const KEY_0: u16 = 11;
    if !shift && (KEY_1..=KEY_9).contains(&code) {
        return Some(EmergencyAction::SwitchVt((code - KEY_1 + 1) as u8));
    }
    if !shift && code == KEY_0 {
        return Some(EmergencyAction::SwitchVt(10));
    }
    None
}

//! Workaround para compositor aninhado (KDE/Plasma, etc.): o compositor pai
//! consome atalhos globais (Ctrl+Alt+Del, Super+Esc…) antes de repassá-los.
//!
//! Na Wayland pedimos `zwp_keyboard_shortcuts_inhibit` na janela winit.
//! O thread evdev (`kill_switch`) continua como fallback quando o inhibit falha.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_registry, wl_seat, wl_surface},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::{
    zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1,
    zwp_keyboard_shortcuts_inhibitor_v1::ZwpKeyboardShortcutsInhibitorV1,
};

use crate::env_detect;

struct ShortcutState;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for ShortcutState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for ShortcutState {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_surface::WlSurface, ()> for ShortcutState {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitManagerV1, ()> for ShortcutState {
    fn event(
        _: &mut Self,
        _: &ZwpKeyboardShortcutsInhibitManagerV1,
        _: wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibit_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, ()> for ShortcutState {
    fn event(
        _: &mut Self,
        _: &ZwpKeyboardShortcutsInhibitorV1,
        _: wayland_protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibitor_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Mantém o pedido de inhibit ativo na conexão Wayland do compositor pai.
pub struct ParentShortcutGuard {
    conn: Connection,
    state: ShortcutState,
    event_queue: EventQueue<ShortcutState>,
    manager: ZwpKeyboardShortcutsInhibitManagerV1,
    seat: wl_seat::WlSeat,
    surface: wl_surface::WlSurface,
    inhibitor: Option<ZwpKeyboardShortcutsInhibitorV1>,
}

impl ParentShortcutGuard {
    pub fn try_new(window: &winit::window::Window) -> Option<Self> {
        if !env_detect::parent_steals_global_shortcuts() {
            return None;
        }

        let (display_ptr, surface_ptr) = wayland_handles(window)?;
        let conn = unsafe {
            let backend = Backend::from_foreign_display(display_ptr.cast());
            Connection::from_backend(backend)
        };

        let (globals, event_queue) = registry_queue_init::<ShortcutState>(&conn).ok()?;
        let qh = event_queue.handle();

        let manager = globals
            .bind(&qh, 1..=1, ())
            .map_err(|err| {
                tracing::warn!("Compositor pai sem keyboard_shortcuts_inhibit: {err}");
            })
            .ok()?;
        let seat = globals.bind(&qh, 1..=7, ()).ok()?;

        let surface_id = unsafe {
            ObjectId::from_ptr(wl_surface::WlSurface::interface(), surface_ptr.cast())
            .map_err(|err| {
                tracing::warn!("wl_surface do winit inválido: {err}");
            })
        }
        .ok()?;
        let surface = wl_surface::WlSurface::from_id(&conn, surface_id).ok()?;

        let mut guard = Self {
            conn,
            state: ShortcutState,
            event_queue,
            manager,
            seat,
            surface,
            inhibitor: None,
        };
        guard.ensure_inhibited();
        guard.poll();
        Some(guard)
    }

    pub fn on_focus(&mut self, focused: bool) {
        if focused {
            self.ensure_inhibited();
            self.poll();
        }
    }

    /// Drena eventos Wayland do compositor pai (necessário para o inhibit funcionar no KWin).
    pub fn poll(&mut self) {
        loop {
            match self.event_queue.dispatch_pending(&mut self.state) {
                Ok(0) => break,
                Ok(_) => {}
                Err(err) => {
                    tracing::trace!("parent_shortcuts dispatch: {err}");
                    break;
                }
            }
        }
        if self.conn.flush().is_err() {
            tracing::trace!("parent_shortcuts flush falhou");
        }
    }
}

impl ParentShortcutGuard {
    fn ensure_inhibited(&mut self) {
        // KWin permite um inhibitor por surface+seat; recriar gera protocol error.
        if self.inhibitor.is_some() {
            return;
        }

        let qh = self.event_queue.handle();
        let inhibitor = self.manager.inhibit_shortcuts(
            &self.surface,
            &self.seat,
            &qh,
            (),
        );
        if self.conn.flush().is_err() {
            tracing::warn!(
                "Falha ao enviar keyboard_shortcuts_inhibit — usando fallback evdev"
            );
            return;
        }
        tracing::info!("Atalhos globais do compositor pai inibidos (keyboard_shortcuts_inhibit)");
        self.inhibitor = Some(inhibitor);
    }
}

fn wayland_handles(window: &winit::window::Window) -> Option<(*mut std::ffi::c_void, *mut std::ffi::c_void)> {
    let display = window.display_handle().ok()?;
    let surface = window.window_handle().ok()?;

    let display_ptr = match display.as_raw() {
        RawDisplayHandle::Wayland(wh) => wh.display.as_ptr() as *mut std::ffi::c_void,
        _ => {
            tracing::info!("Sessão pai em X11 — inhibit Wayland indisponível; evdev ativo");
            return None;
        }
    };

    let surface_ptr = match surface.as_raw() {
        RawWindowHandle::Wayland(wh) => wh.surface.as_ptr() as *mut std::ffi::c_void,
        _ => return None,
    };

    if display_ptr.is_null() || surface_ptr.is_null() {
        return None;
    }

    Some((display_ptr, surface_ptr))
}

pub fn log_workaround() {
    if !env_detect::parent_steals_global_shortcuts() {
        return;
    }
    if env_detect::is_kde_session() {
        tracing::info!(
            "KDE/Plasma detectado — KWin pode engolir atalhos globais e Super; \
             ativando keyboard_shortcuts_inhibit + evdev"
        );
    } else {
        tracing::info!(
            "Compositor aninhado — atalhos globais do pai podem ser interceptados; \
             ativando inhibit Wayland + evdev"
        );
    }
}

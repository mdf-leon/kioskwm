//! Apps Wayland em execução — foco, nomes e menu de contexto.

use smithay::{
    input::{
        keyboard::KeyboardTarget,
        pointer::{MotionEvent, PointerTarget},
    },
    utils::{Logical, Point},
    wayland::{
        compositor::with_states,
        shell::xdg::{ToplevelSurface, XdgToplevelSurfaceData},
    },
    xwayland::X11Surface,
};
use wayland_server::{protocol::wl_surface::WlSurface, Resource};

#[derive(Clone)]
pub struct RunningApp {
    pub surface: ToplevelSurface,
    pub display_name: String,
}

pub struct X11App {
    pub surface: X11Surface,
    pub display_name: String,
}

pub fn display_name(surface: &ToplevelSurface) -> String {
    with_states(surface.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|lock| lock.lock().ok())
            .map(|attrs| {
                attrs
                    .app_id
                    .as_deref()
                    .map(short_name)
                    .or_else(|| {
                        attrs
                            .title
                            .as_ref()
                            .map(|t| short_name(t))
                    })
                    .unwrap_or_else(|| "app".to_string())
            })
            .unwrap_or_else(|| "app".to_string())
    })
}

pub fn menu_label(main_word: &str, app_name: &str) -> String {
    format!("{main_word} ({app_name})")
}

pub fn short_name(raw: &str) -> String {
    let s = raw.rsplit('.').next().unwrap_or(raw);
    s.to_lowercase()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnifiedApp {
    Wayland(usize),
    X11(usize),
}

impl crate::state::State {
    pub fn app_count(&self) -> usize {
        self.running_apps.len() + self.x11_apps.len()
    }

    /// Menu: X11 primeiro (Krita), depois Wayland (Konsole).
    pub fn unified_app(&self, index: usize) -> Option<UnifiedApp> {
        if index >= self.app_count() {
            return None;
        }
        let x11 = self.x11_apps.len();
        if index < x11 {
            Some(UnifiedApp::X11(index))
        } else {
            Some(UnifiedApp::Wayland(index - x11))
        }
    }

    pub fn unified_app_name(&self, index: usize) -> &str {
        match self.unified_app(index) {
            Some(UnifiedApp::X11(i)) => self
                .x11_apps
                .get(i)
                .map(|a| a.display_name.as_str())
                .unwrap_or("app"),
            Some(UnifiedApp::Wayland(i)) => self
                .running_apps
                .get(i)
                .map(|a| a.display_name.as_str())
                .unwrap_or("app"),
            None => "app",
        }
    }

    /// Remove toplevels Wayland fantasma do cliente XWayland (duplicam Krita no menu).
    pub fn purge_xwayland_from_running_apps(&mut self) {
        let before = self.running_apps.len();
        self.running_apps.retain(|app| {
            !app
                .surface
                .wl_surface()
                .client()
                .is_some_and(|c| c.get_data::<smithay::xwayland::XWaylandClientData>().is_some())
        });
        if self.running_apps.len() == before {
            return;
        }
        tracing::info!(
            "Toplevel fantasma XWayland removido ({} → {})",
            before,
            self.running_apps.len()
        );
        if self.running_apps.is_empty() {
            self.focused_app = 0;
        } else if self.focused_app >= self.running_apps.len() {
            self.focused_app = self.running_apps.len() - 1;
        }
        if !self.focused_is_x11 {
            self.apply_focus();
        }
        crate::context_menu::invalidate_cache(self);
    }

    fn leave_x11_input(&mut self) {
        let x11_surface = self.x11_apps.get(self.focused_x11).map(|a| a.surface.clone());
        let Some(x11_surface) = x11_surface else {
            return;
        };
        let seat = self.seat.clone();
        let serial = self.next_serial();
        KeyboardTarget::leave(&x11_surface, &seat, self, serial);
    }

    fn leave_wayland_input(&mut self) {
        let Some(app) = self.running_apps.get(self.focused_app) else {
            return;
        };
        let wl = app.surface.wl_surface().clone();
        let seat = self.seat.clone();
        let serial = self.next_serial();
        KeyboardTarget::leave(&wl, &seat, self, serial);
        PointerTarget::leave(&wl, &seat, self, serial, 0);
    }

    /// Reenvia ponteiro/teclado à app focada (após menu WM, etc.).
    pub fn sync_input_to_focus(&mut self) {
        self.apply_focus();
    }

    pub fn unified_focus_index(&self) -> usize {
        if self.focused_is_x11 {
            let n = self.x11_apps.len();
            if n == 0 {
                return 0;
            }
            self.focused_x11.min(n - 1)
        } else {
            let wl = self.running_apps.len();
            if wl == 0 {
                return 0;
            }
            self.x11_apps.len() + self.focused_app.min(wl - 1)
        }
    }

    pub fn sync_app_mru(&mut self) {
        let count = self.app_count();
        let mut order = self.app_mru.clone();
        order.retain(|&i| i < count);
        for i in 0..count {
            if !order.contains(&i) {
                order.push(i);
            }
        }
        self.app_mru = order;
    }

    pub fn touch_mru(&mut self, unified_idx: usize) {
        if unified_idx >= self.app_count() {
            return;
        }
        self.app_mru.retain(|&i| i != unified_idx);
        self.app_mru.insert(0, unified_idx);
    }

    pub fn focus_unified(&mut self, index: usize) {
        let Some(kind) = self.unified_app(index) else {
            tracing::warn!("Menu: índice {index} inválido (apps={})", self.app_count());
            return;
        };
        let name = self.unified_app_name(index).to_string();
        tracing::info!(
            "Menu clique índice {index} → {name} ({kind:?}) | wl={} x11={} foco_atual={}",
            self.running_apps.len(),
            self.x11_apps.len(),
            self.unified_focus_index(),
        );
        match kind {
            UnifiedApp::Wayland(i) => self.focus_app(i),
            UnifiedApp::X11(i) => self.focus_x11(i),
        }
    }

    /// Fecha a app em foco (Alt+F4, X do KDE, menu WM).
    pub fn close_focused(&mut self) {
        if self.focused_is_x11 {
            let Some(app) = self.x11_apps.get(self.focused_x11) else {
                return;
            };
            let name = app.display_name.clone();
            if let Err(err) = app.surface.close() {
                tracing::warn!("X11 close {name}: {err}");
            } else {
                tracing::info!("Fechando: {name}");
            }
            return;
        }
        let Some(app) = self.running_apps.get(self.focused_app) else {
            return;
        };
        let name = app.display_name.clone();
        app.surface.send_close();
        tracing::info!("Fechando: {name}");
    }

    /// X do KDE / botão fechar da janela do compositor.
    pub fn handle_close_request(&mut self) {
        if self.app_count() > 0 {
            self.dismiss_popup_grab();
            self.close_focused();
        } else {
            crate::state::request_exit(&self.exit_requested);
        }
    }

    pub fn focused_toplevel(&self) -> Option<&ToplevelSurface> {
        if self.focused_is_x11 {
            return None;
        }
        self.running_apps.get(self.focused_app).map(|a| &a.surface)
    }

    pub fn focus_app(&mut self, index: usize) {
        if index >= self.running_apps.len() {
            return;
        }
        if !self.focused_is_x11 && self.focused_app == index {
            return;
        }
        if self.focused_is_x11 {
            self.leave_x11_input();
        }
        let surface = self.running_apps[index].surface.clone();
        self.focused_is_x11 = false;
        self.x11_input_wanted = false;
        self.x11_focus_pending = false;
        self.focused_app = index;
        self.dismiss_popup_grab();
        self.configure_kiosk(&surface);
        self.touch_mru(self.unified_focus_index());
        self.apply_focus();
        crate::context_menu::invalidate_cache(self);
    }

    pub fn focus_x11(&mut self, index: usize) {
        if index >= self.x11_apps.len() {
            return;
        }
        if self.focused_is_x11
            && self.focused_x11 == index
            && self.x11_input_wanted
            && !self.x11_focus_pending
        {
            return;
        }

        let prev_x11 = self.focused_is_x11.then_some(self.focused_x11);
        let had_input = self.x11_input_active();
        let x11_surface = self.x11_apps[index].surface.clone();
        let wl_ready = x11_surface.wl_surface().is_some();

        if let Some(prev) = prev_x11 {
            if prev != index && had_input {
                self.leave_x11_input();
            }
        }

        self.focused_is_x11 = true;
        self.focused_x11 = index;
        self.x11_input_wanted = true;
        self.dismiss_popup_grab();

        if !wl_ready {
            self.x11_focus_pending = true;
            tracing::info!(
                "X11 {}: aguardando wl_surface (input Wayland até lá)",
                self.x11_apps[index].display_name
            );
            crate::context_menu::invalidate_cache(self);
            return;
        }

        self.x11_focus_pending = false;
        if !had_input {
            self.leave_wayland_input();
        }

        self.touch_mru(self.unified_focus_index());
        self.apply_focus();
        crate::context_menu::invalidate_cache(self);
    }

    pub fn sync_app_name(&mut self, surface: &ToplevelSurface) {
        let name = display_name(surface);
        if let Some(app) = self
            .running_apps
            .iter_mut()
            .find(|a| a.surface.wl_surface() == surface.wl_surface())
        {
            if app.display_name != name {
                tracing::info!("App: {} → {name}", app.display_name);
                app.display_name = name;
                crate::context_menu::invalidate_cache(self);
            }
        }
    }

    fn clear_input_focus(&mut self) {
        let keyboard = self.keyboard.clone();
        let pointer = self.pointer.clone();
        let serial = self.next_serial();
        keyboard.set_focus(self, None, serial);
        let motion = MotionEvent {
            location: self.pointer_pos,
            serial: self.next_serial(),
            time: 0,
        };
        pointer.motion(self, None, &motion);
        pointer.frame(self);
    }

    pub fn apply_focus(&mut self) {
        if self.context_menu.open || self.overlay_open || self.alt_tab.open {
            self.deferred_focus = true;
            return;
        }
        self.deferred_focus = false;
        if self.focused_is_x11 {
            self.apply_x11_focus();
        } else {
            self.apply_wayland_focus();
        }
    }

    pub fn flush_deferred_focus(&mut self) {
        if !self.deferred_focus {
            return;
        }
        self.deferred_focus = false;
        if self.context_menu.open || self.overlay_open || self.alt_tab.open {
            return;
        }
        if self.focused_is_x11 {
            self.apply_x11_focus();
        } else {
            self.apply_wayland_focus();
        }
    }

    fn apply_x11_focus(&mut self) {
        if !self.x11_input_wanted {
            return;
        }
        let focused = self.focused_x11;
        let Some(x11_surface) = self.x11_apps.get(focused).map(|a| a.surface.clone()) else {
            self.x11_focus_pending = false;
            return;
        };
        let name = self.x11_apps[focused].display_name.clone();

        self.dismiss_popup_grab();

        let Some(wl_surface) = x11_surface.wl_surface() else {
            self.x11_focus_pending = true;
            tracing::info!("X11 {name}: aguardando wl_surface");
            return;
        };

        self.x11_focus_pending = false;
        self.leave_wayland_input();

        let keyboard = self.keyboard.clone();
        let pointer = self.pointer.clone();
        let seat = self.seat.clone();
        let focus_serial = self.next_serial();

        keyboard.set_focus(self, Some(wl_surface.clone()), focus_serial);
        let motion = MotionEvent {
            location: self.pointer_pos,
            serial: self.next_serial(),
            time: 0,
        };
        PointerTarget::enter(&wl_surface, &seat, self, &motion);
        pointer.motion(
            self,
            Some((wl_surface.clone(), Point::from((0.0, 0.0)))),
            &motion,
        );
        pointer.frame(self);
        tracing::info!("Input → X11 {name} (wl_surface={})", wl_surface.id());
    }

    fn apply_wayland_focus(&mut self) {
        self.x11_focus_pending = false;
        self.dismiss_popup_grab();
        let Some(app) = self.running_apps.get(self.focused_app) else {
            self.clear_input_focus();
            return;
        };
        let wl_surface = app.surface.wl_surface().clone();
        let name = app.display_name.clone();
        let keyboard = self.keyboard.clone();
        let pointer = self.pointer.clone();
        let focus_serial = self.next_serial();
        keyboard.set_focus(self, Some(wl_surface.clone()), focus_serial);
        let origin = self.surface_origin_for(&wl_surface);
        let motion_serial = self.next_serial();
        pointer.motion(
            self,
            Some((wl_surface, origin)),
            &MotionEvent {
                location: self.pointer_pos,
                serial: motion_serial,
                time: 0,
            },
        );
        pointer.frame(self);
        tracing::info!("Input → Wayland {name}");
    }

    pub fn dismiss_popup_grab(&mut self) {
        self.active_popup = None;
        if self.pointer.is_grabbed() {
            let pointer = self.pointer.clone();
            let serial = self.next_serial();
            pointer.unset_grab(self, serial, 0);
        }
    }

    pub fn surface_origin_for(&self, surface: &WlSurface) -> Point<f64, Logical> {
        if let Some((ox, oy)) = self.popup_render_offset_for(surface) {
            return Point::from((ox as f64, oy as f64));
        }
        Point::from((0.0, 0.0))
    }

    pub fn toplevel_for_surface(&self, wl: &WlSurface) -> Option<ToplevelSurface> {
        self.running_apps
            .iter()
            .find(|a| a.surface.wl_surface() == wl)
            .map(|a| a.surface.clone())
    }
}

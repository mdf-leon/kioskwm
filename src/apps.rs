//! Apps Wayland em execução — foco, nomes e menu de contexto.

use std::time::Instant;

use smithay::{
    backend::input::ButtonState,
    desktop::space::SpaceElement,
    input::{
        keyboard::KeyboardTarget,
        pointer::{ButtonEvent, MotionEvent, PointerTarget},
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

/// App visível e que recebe input — fonte única para render e pointer/keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveTarget {
    Wayland(usize),
    X11(usize),
}

impl crate::state::State {
    pub fn active_target(&self) -> Option<ActiveTarget> {
        if self.focused_is_x11 && self.x11_input_wanted {
            if !self.x11_apps.is_empty() {
                return Some(ActiveTarget::X11(
                    self.focused_x11.min(self.x11_apps.len() - 1),
                ));
            }
            if self.x11_foot_refocus_at.is_some() {
                // Splash fechou, janela principal ainda não mapeou — tela preta.
                return None;
            }
        }
        if !self.running_apps.is_empty() {
            return Some(ActiveTarget::Wayland(
                self.focused_app.min(self.running_apps.len() - 1),
            ));
        }
        if !self.x11_apps.is_empty() {
            return Some(ActiveTarget::X11(
                self.focused_x11.min(self.x11_apps.len() - 1),
            ));
        }
        None
    }

    pub fn app_count(&self) -> usize {
        self.running_apps.len() + self.x11_apps.len()
    }

    /// Menu: Wayland primeiro (Konsole principal), depois X11 (Kate/Krita).
    pub fn unified_app(&self, index: usize) -> Option<UnifiedApp> {
        let wl = self.running_apps.len();
        let total = wl + self.x11_apps.len();
        if index >= total {
            return None;
        }
        if index < wl {
            Some(UnifiedApp::Wayland(index))
        } else {
            Some(UnifiedApp::X11(index - wl))
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
        PointerTarget::leave(&x11_surface, &seat, self, serial, 0);
        let _ = x11_surface.set_activated(false);
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
        self.resync_input_after_overlay();
    }

    /// Bloqueia teclado dos clientes enquanto overlay WM (Alt+Tab, etc.) está aberto.
    pub fn suspend_client_keyboard_for_wm_ui(&mut self) {
        self.release_all_keys();
        self.release_keyboard_grab();
        let keyboard = self.keyboard.clone();
        let serial = self.next_serial();
        keyboard.set_focus(self, None, serial);
    }

    /// Restaura teclado/mouse/modificadores após Alt+Tab, menu WM, etc.
    pub fn resync_input_after_overlay(&mut self) {
        self.deferred_focus = false;
        self.dismiss_popup_grab();
        if let Some(idx) = self.pending_autofocus.take() {
            if self.wm_ui_blocks_focus() {
                self.pending_autofocus = Some(idx);
            } else {
                self.focus_unified(idx);
            }
        }
        if self.keyboard.is_grabbed() && self.focused_is_x11 && self.x11_input_wanted {
            if let Some(app) = self.x11_apps.get(self.focused_x11) {
                let surface = app.surface.clone();
                self.sync_x11_keyboard_enter(&surface);
            }
            self.sync_x11_pointer_after_grab();
            self.push_modifiers_to_focus();
            self.refresh_active_surface();
            return;
        }
        let keyboard = self.keyboard.clone();
        let serial = self.next_serial();
        keyboard.set_focus(self, None, serial);
        self.dispatch_input_focus();
        self.refresh_active_surface();
        self.push_modifiers_to_focus();
    }

    pub fn wm_ui_blocks_focus(&self) -> bool {
        self.context_menu.open || self.overlay_open || self.alt_tab.open
    }

    /// Após unmap do splash (ou fechar Krita), devolve foco ao foot só se nada mapear a tempo.
    pub fn tick_x11_foot_refocus(&mut self) {
        let Some(deadline) = self.x11_foot_refocus_at else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }
        self.x11_foot_refocus_at = None;
        if !self.x11_apps.is_empty() {
            return;
        }
        tracing::info!("X11 encerrado — voltando ao terminal");
        self.focused_is_x11 = false;
        self.x11_input_wanted = false;
        self.x11_focus_pending = false;
        if !self.running_apps.is_empty() {
            self.focused_app = self.focused_app.min(self.running_apps.len() - 1);
        }
        self.apply_focus();
    }

    /// Foca app recém-aberta; adia se menu/overlay WM estiver aberto.
    pub fn autofocus_new_unified(&mut self, unified_idx: usize, name: &str) {
        if self.wm_ui_blocks_focus() {
            self.pending_autofocus = Some(unified_idx);
            self.deferred_focus = true;
            tracing::info!("Nova app {name} — foco adiado (UI WM aberta)");
            return;
        }
        self.pending_autofocus = None;
        tracing::info!("Nova app {name} — foco automático");
        self.focus_unified(unified_idx);
    }

    fn release_keyboard_grab(&mut self) {
        if !self.keyboard.is_grabbed() {
            return;
        }
        let keyboard = self.keyboard.clone();
        keyboard.unset_grab(self);
    }

    /// Evita tecla repetindo no cliente após Alt+Tab/menu WM (release perdido).
    pub fn release_all_keys(&mut self) {
        use smithay::backend::input::KeyState;
        use smithay::input::keyboard::FilterResult;

        let keyboard = self.keyboard.clone();
        let keys: Vec<_> = keyboard.pressed_keys().into_iter().collect();
        if keys.is_empty() {
            return;
        }
        tracing::debug!("Liberando {} tecla(s) presa(s) no compositor", keys.len());
        for key in keys {
            let serial = self.next_serial();
            keyboard.input::<(), _>(self, key, KeyState::Released, serial, 0, |_, _, _| {
                FilterResult::Forward
            });
        }
    }

    fn sync_keyboard_enter(&mut self, surface: &WlSurface) {
        let keyboard = self.keyboard.clone();
        let seat = self.seat.clone();
        let mods = keyboard.modifier_state();
        let enter_serial = self.next_serial();
        // Não reenviar teclas ainda pressionadas — duplica chars ao refocar (Enter/clique).
        KeyboardTarget::enter(surface, &seat, self, Vec::new(), enter_serial);
        let mod_serial = self.next_serial();
        KeyboardTarget::modifiers(surface, &seat, self, mods, mod_serial);
    }

    /// X11 (Kate/Krita): precisa de WM_TAKE_FOCUS, não só wl_keyboard.enter.
    pub(crate) fn sync_x11_keyboard_enter(&mut self, x11_surface: &X11Surface) {
        let keyboard = self.keyboard.clone();
        let seat = self.seat.clone();
        let mods = keyboard.modifier_state();
        let enter_serial = self.next_serial();
        KeyboardTarget::enter(x11_surface, &seat, self, Vec::new(), enter_serial);
        let mod_serial = self.next_serial();
        KeyboardTarget::modifiers(x11_surface, &seat, self, mods, mod_serial);
    }

    fn push_modifiers_to_focus(&mut self) {
        let mods = self.keyboard.modifier_state();
        let seat = self.seat.clone();
        let serial = self.next_serial();
        let Some((surface, _)) = self.pointer_focus() else {
            return;
        };
        KeyboardTarget::modifiers(&surface, &seat, self, mods, serial);
    }

    pub fn unified_focus_index(&self) -> usize {
        let wl = self.running_apps.len();
        match self.active_target() {
            Some(ActiveTarget::Wayland(i)) => i,
            Some(ActiveTarget::X11(i)) => wl + i,
            None => 0,
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

    /// Toplevel Wayland que recebe input (popups, ponteiro quando X11 não está ativo).
    pub fn focused_toplevel(&self) -> Option<&ToplevelSurface> {
        if self.x11_input_active() {
            return None;
        }
        self.running_apps.get(self.focused_app).map(|a| &a.surface)
    }

    pub fn focus_app(&mut self, index: usize) {
        if index >= self.running_apps.len() {
            return;
        }
        if !self.focused_is_x11 && self.focused_app == index && self.x11_input_active() == false {
            self.apply_focus();
            return;
        }
        if !self.focused_is_x11
            && self.focused_app != index
            && self.focused_app < self.running_apps.len()
        {
            let old = self.running_apps[self.focused_app].surface.clone();
            self.configure_toplevel(&old, false);
        }
        if self.focused_is_x11 {
            self.release_all_keys();
            self.leave_x11_input();
        }
        let surface = self.running_apps[index].surface.clone();
        self.focused_is_x11 = false;
        self.x11_input_wanted = false;
        self.x11_focus_pending = false;
        self.x11_autofocus_idx = None;
        self.focused_app = index;
        self.dismiss_popup_grab();
        self.configure_kiosk(&surface);
        self.touch_mru(self.unified_focus_index());
        self.apply_focus();
        self.refresh_active_surface();
        crate::context_menu::invalidate_cache(self);
    }

    pub fn focus_x11(&mut self, index: usize) {
        if index >= self.x11_apps.len() {
            return;
        }
        if self.focused_is_x11
            && self.focused_x11 == index
            && self.x11_input_active()
        {
            self.apply_focus();
            return;
        }

        if self.focused_is_x11 && self.x11_input_active() && self.focused_x11 != index {
            self.release_all_keys();
            self.leave_x11_input();
        } else if !self.x11_input_active() {
            self.leave_wayland_input();
        }

        self.focused_is_x11 = true;
        self.focused_x11 = index;
        self.x11_input_wanted = true;
        self.x11_foot_refocus_at = None;
        self.dismiss_popup_grab();

        let wl_ready = self.x11_apps[index].surface.wl_surface().is_some();
        if !wl_ready {
            self.x11_focus_pending = true;
            tracing::info!(
                "X11 {}: aguardando wl_surface",
                self.x11_apps[index].display_name
            );
            self.apply_wayland_input(false);
            self.note_full_damage();
            self.request_render();
            crate::context_menu::invalidate_cache(self);
            return;
        }

        self.x11_focus_pending = false;
        self.touch_mru(self.unified_focus_index());
        self.apply_focus();
        self.refresh_active_surface();
        crate::context_menu::invalidate_cache(self);
    }

    fn refresh_active_surface(&mut self) {
        use smithay::utils::{Point, Rectangle, Size};
        let target = self.active_target();
        match target {
            Some(ActiveTarget::Wayland(i)) => {
                let surface = self.running_apps.get(i).map(|a| a.surface.clone());
                if let Some(surface) = surface {
                    self.configure_kiosk(&surface);
                }
            }
            Some(ActiveTarget::X11(i)) => {
                let x11 = self.x11_apps.get(i).map(|a| a.surface.clone());
                if let Some(x11) = x11 {
                    let rect = Rectangle::new(Point::from((0, 0)), Size::from(self.output_size));
                    let _ = x11.configure(rect);
                    x11.refresh();
                }
            }
            None => {}
        }
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
        if self.wm_ui_blocks_focus() {
            self.deferred_focus = true;
            return;
        }
        self.deferred_focus = false;
        self.dispatch_input_focus();
        self.note_full_damage();
        self.request_render();
    }

    pub fn flush_deferred_focus(&mut self) {
        if !self.deferred_focus {
            return;
        }
        self.deferred_focus = false;
        if self.wm_ui_blocks_focus() {
            return;
        }
        if let Some(idx) = self.pending_autofocus.take() {
            self.focus_unified(idx);
        }
        self.dispatch_input_focus();
    }

    fn dispatch_input_focus(&mut self) {
        self.dismiss_popup_grab();

        if self.keyboard.is_grabbed() {
            if self.focused_is_x11 && self.x11_input_wanted {
                if let Some(app) = self.x11_apps.get(self.focused_x11) {
                    let surface = app.surface.clone();
                    self.sync_x11_keyboard_enter(&surface);
                }
                self.sync_x11_pointer_after_grab();
            } else if !self.focused_is_x11 {
                self.apply_wayland_input(true);
            }
            return;
        }

        if self.focused_is_x11 && self.x11_input_wanted && self.x11_apps.is_empty() {
            self.focused_is_x11 = false;
            self.x11_input_wanted = false;
            self.x11_focus_pending = false;
            self.x11_foot_refocus_at = None;
        }

        if self.focused_is_x11 && self.x11_input_wanted {
            if self.try_apply_x11_focus() {
                return;
            }
            // X11 carregando — foot responde (não zerar ponteiro).
            self.apply_wayland_input(false);
            return;
        }

        self.apply_wayland_input(true);
    }

    fn try_apply_x11_focus(&mut self) -> bool {
        let focused = self.focused_x11;
        let Some(x11_surface) = self.x11_apps.get(focused).map(|a| a.surface.clone()) else {
            self.x11_focus_pending = false;
            return false;
        };
        let name = self.x11_apps[focused].display_name.clone();
        let Some(wl_surface) = x11_surface.wl_surface() else {
            self.x11_focus_pending = true;
            tracing::info!("X11 {name}: aguardando wl_surface");
            return false;
        };

        self.x11_focus_pending = false;
        self.dismiss_popup_grab();
        let _ = x11_surface.set_activated(true);
        x11_surface.refresh();
        let overlap = smithay::utils::Rectangle::from_size(self.output_size);
        x11_surface.output_enter(&self.output, overlap);

        let keyboard = self.keyboard.clone();
        let pointer = self.pointer.clone();
        let focus_serial = self.next_serial();
        keyboard.set_focus(self, Some(wl_surface.clone()), focus_serial);
        // WM_TAKE_FOCUS + wl_keyboard.enter (Kate precisa disto para cliques/teclado X11).
        self.sync_x11_keyboard_enter(&x11_surface);
        let motion = MotionEvent {
            location: self.pointer_pos,
            serial: self.next_serial(),
            time: 0,
        };
        pointer.motion(
            self,
            Some((wl_surface.clone(), Point::from((0.0, 0.0)))),
            &motion,
        );
        pointer.frame(self);
        tracing::info!("Foco ativo → X11 {name} (wl={})", wl_surface.id());
        true
    }

    /// Entrega movimento do ponteiro ao cliente focado (Wayland ou X11).
    pub fn deliver_pointer_motion(&mut self, time: u32) {
        let pointer = self.pointer.clone();
        let location = self.pointer_pos;
        let serial = self.next_serial();
        let motion = MotionEvent {
            location,
            serial,
            time,
        };
        let focus = self.pointer_focus();
        pointer.motion(self, focus, &motion);
        pointer.frame(self);
    }

    /// Entrega clique do ponteiro — X11 recebe WM_TAKE_FOCUS antes do botão.
    pub fn deliver_pointer_button(&mut self, button: u32, button_state: ButtonState, time: u32) {
        if button_state == ButtonState::Pressed {
            if let Some(x11) = self.pointer_x11_surface() {
                if !x11.is_override_redirect() {
                    self.sync_x11_keyboard_enter(&x11);
                }
            }
        }
        self.deliver_pointer_motion(time);

        let x11 = self.pointer_x11_surface().or_else(|| {
            // Menu GTK (OR): release do botão direito no frame do map às vezes
            // não passa no hit-test — manda pro overlay mais recente.
            const BTN_RIGHT: u32 = 0x111;
            if button != BTN_RIGHT || button_state != ButtonState::Released {
                return None;
            }
            self.x11_overlays.last().and_then(|o| {
                let geo = o.geometry();
                (geo.size.w > 0 && geo.size.h > 0 && o.wl_surface().is_some()).then(|| o.clone())
            })
        });

        if let Some(x11) = x11 {
            let seat = self.seat.clone();
            let serial = self.next_serial();
            let event = ButtonEvent {
                serial,
                time,
                button,
                state: button_state,
            };
            PointerTarget::button(&x11, &seat, self, &event);
            PointerTarget::frame(&x11, &seat, self);
            self.pointer.clone().frame(self);
            return;
        }

        let pointer = self.pointer.clone();
        let serial = self.next_serial();
        pointer.button(
            self,
            &ButtonEvent {
                serial,
                time,
                button,
                state: button_state,
            },
        );
        pointer.frame(self);
    }

    /// Ponteiro para X11 quando o cliente pediu keyboard grab (não soltar o grab).
    pub fn sync_x11_pointer_after_grab(&mut self) {
        let idx = self.focused_x11;
        let Some(x11_surface) = self.x11_apps.get(idx).map(|a| a.surface.clone()) else {
            return;
        };
        let Some(wl_surface) = x11_surface.wl_surface() else {
            return;
        };
        let pointer = self.pointer.clone();
        let motion = MotionEvent {
            location: self.pointer_pos,
            serial: self.next_serial(),
            time: 0,
        };
        pointer.motion(
            self,
            Some((wl_surface, Point::from((0.0, 0.0)))),
            &motion,
        );
        pointer.frame(self);
    }

    /// `clear_x11_intent`: false enquanto X11 está carregando (mantém intenção de foco).
    fn apply_wayland_input(&mut self, clear_x11_intent: bool) {
        if clear_x11_intent {
            self.x11_focus_pending = false;
            self.x11_input_wanted = false;
            self.focused_is_x11 = false;
        }
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
        let motion = MotionEvent {
            location: self.pointer_pos,
            serial: self.next_serial(),
            time: 0,
        };
        pointer.motion(self, Some((wl_surface, origin)), &motion);
        pointer.frame(self);
        tracing::info!("Foco ativo → Wayland {name}");
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

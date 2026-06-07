//! XWayland — apps X11 (Krita snap, etc.) como no COSMIC/anvil.

use std::process::Stdio;

use smithay::{
    delegate_xwayland_shell,
    desktop::space::SpaceElement,
    reexports::calloop::{self, EventLoop},
    utils::{Logical, Point, Rectangle, Size},
    wayland::{
        compositor::CompositorHandler,
        xwayland_shell::{XWaylandShellHandler, XWaylandShellState},
    },
    xwayland::{
        xwm::{Reorder, ResizeEdge, XwmId},
        X11Surface, X11Wm, XWayland, XWaylandEvent, XwmHandler,
    },
};
use wayland_server::{protocol::wl_surface::WlSurface, DisplayHandle};

use crate::{
    apps::X11App,
    spawn,
    state::State,
};

impl XWaylandShellHandler for State {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell_state
    }

    fn surface_associated(
        &mut self,
        _xwm_id: XwmId,
        _wl: WlSurface,
        window: X11Surface,
    ) {
        let Some(idx) = self.x11_apps.iter().position(|a| a.surface == window) else {
            return;
        };
        tracing::info!("X11 wl_surface associada: {}", self.x11_apps[idx].display_name);
        if self.focused_is_x11 && self.focused_x11 == idx && self.x11_input_wanted {
            self.x11_focus_pending = false;
            if !self.context_menu.open && !self.overlay_open && !self.alt_tab.open {
                self.apply_focus();
            } else {
                self.deferred_focus = true;
            }
        }
        crate::context_menu::invalidate_cache(self);
    }
}

impl XwmHandler for State {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        self.xwm.as_mut().expect("X11Wm ativo")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let name = window_title(&window);
        tracing::info!("X11 map request: {name}");
        // Não chamar raise_window/set_activated aqui — usam XGrabServer e travam o loop
        // principal enquanto o cliente (Kate) ainda está mapeando.
        let _ = window.set_mapped(true);
        self.purge_xwayland_from_running_apps();
        let is_new = if self.x11_apps.iter().any(|a| a.surface == window) {
            false
        } else {
            let idx = self.x11_apps.len();
            self.x11_apps.push(X11App {
                surface: window,
                display_name: name.clone(),
            });
            self.x11_autofocus_idx = Some(idx);
            crate::context_menu::invalidate_cache(self);
            true
        };
        if is_new {
            self.sync_app_mru();
            let foco = self.unified_app_name(self.unified_focus_index());
            tracing::info!("X11 {name}: registrada em background — foco mantém {foco}");
        }
        crate::alt_tab::invalidate_cache(self);
    }

    fn map_window_notify(&mut self, _xwm: XwmId, window: X11Surface) {
        let name = window_title(&window);
        tracing::info!("X11 mapped: {name}");
        let geo = Rectangle::new(
            Point::from((0, 0)),
            Size::from(self.output_size),
        );
        let _ = window.configure(geo);
        let overlap = Rectangle::from_size(self.output_size);
        window.output_enter(&self.output, overlap);
        window.refresh();
        if let Some(idx) = self.x11_apps.iter().position(|a| a.surface == window) {
            if self.x11_focus_pending
                && self.focused_is_x11
                && self.focused_x11 == idx
                && self.x11_input_wanted
            {
                self.x11_focus_pending = false;
                self.apply_focus();
            } else if self.x11_autofocus_idx == Some(idx)
                && !self.context_menu.open
                && !self.overlay_open
                && !self.alt_tab.open
            {
                self.x11_autofocus_idx = None;
                tracing::info!("X11 {name}: primeira map — ativando foco e input");
                self.focus_x11(idx);
            }
        }
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        self.x11_apps.retain(|a| a.surface != window);
        if self.x11_apps.is_empty() {
            if self.focused_is_x11 {
                self.focused_is_x11 = false;
                self.x11_input_wanted = false;
                self.x11_focus_pending = false;
                if !self.running_apps.is_empty() {
                    self.focused_app = self.focused_app.min(self.running_apps.len() - 1);
                }
                self.apply_focus();
            }
            return;
        }
        if self.focused_x11 >= self.x11_apps.len() {
            self.focused_x11 = self.x11_apps.len() - 1;
        }
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        let before = self.x11_apps.len();
        self.x11_apps.retain(|a| a.surface != window);
        if self.x11_apps.len() < before {
            tracing::info!("X11 fechada");
        }
        if self.x11_apps.is_empty() && self.running_apps.is_empty() {
            crate::state::request_exit(&self.exit_requested);
        }
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        _x: Option<i32>,
        _y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        let mut size = self.output_size;
        if let Some(w) = w {
            size.w = w as i32;
        }
        if let Some(h) = h {
            size.h = h as i32;
        }
        let geo = Rectangle::new(Point::from((0, 0)), Size::from(size));
        let _ = window.configure(geo);
    }

    fn configure_notify(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _geometry: Rectangle<i32, Logical>,
        _above: Option<smithay::xwayland::xwm::X11Window>,
    ) {
    }

    fn property_notify(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        property: smithay::xwayland::xwm::WmWindowProperty,
    ) {
        use smithay::xwayland::xwm::WmWindowProperty;
        match property {
            WmWindowProperty::Title => {
                let name = window_title(&window);
                if let Some(app) = self.x11_apps.iter_mut().find(|a| a.surface == window) {
                    if app.display_name != name {
                        app.display_name = name;
                        crate::context_menu::invalidate_cache(self);
                    }
                }
            }
            WmWindowProperty::Hints | WmWindowProperty::Protocols => {
                if let Some(idx) = self.x11_apps.iter().position(|a| a.surface == window) {
                    tracing::info!(
                        "X11 {} propriedade {:?}",
                        self.x11_apps[idx].display_name,
                        property,
                    );
                    if self.focused_is_x11
                        && self.focused_x11 == idx
                        && self.x11_input_wanted
                        && self.x11_focus_pending
                    {
                        self.x11_focus_pending = false;
                        self.apply_focus();
                    }
                }
            }
            _ => {}
        }
    }

    fn fullscreen_request(&mut self, _xwm: XwmId, window: X11Surface) {
        let geo = Rectangle::new(
            Point::from((0, 0)),
            Size::from(self.output_size),
        );
        let _ = window.configure(geo);
        if let Some(idx) = self.x11_apps.iter().position(|a| a.surface == window) {
            self.focus_x11(idx);
        }
    }

    fn resize_request(
        &mut self,
        _xwm: XwmId,
        _window: X11Surface,
        _button: u32,
        _resize_edge: ResizeEdge,
    ) {
    }

    fn move_request(&mut self, _xwm: XwmId, _window: X11Surface, _button: u32) {}
}

delegate_xwayland_shell!(State);

fn window_title(window: &X11Surface) -> String {
    crate::apps::short_name(&window.title())
}

pub fn start(handle: &calloop::LoopHandle<'static, State>, dh: &DisplayHandle) {
    let loop_handle = handle.clone();
    let (xwayland, client) = match XWayland::spawn(
        dh,
        None,
        std::iter::empty::<(String, String)>(),
        true,
        Stdio::null(),
        Stdio::null(),
        |_| (),
    ) {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!("XWayland indisponível: {err}");
            return;
        }
    };

    if let Err(err) = handle.insert_source(xwayland, move |event, _, data| match event {
        XWaylandEvent::Ready {
            x11_socket,
            display_number,
        } => {
            data.client_compositor_state(&client).set_client_scale(1.0);
            match X11Wm::start_wm(loop_handle.clone(), x11_socket, client.clone()) {
                Ok(wm) => {
                    data.xwm = Some(wm);
                    data.x11_display = Some(display_number);
                    data.xwayland_client = Some(client.id());
                    spawn::set_x11_display(display_number);
                    tracing::info!("XWayland :{display_number} pronto (apps snap/xcb)");
                }
                Err(err) => tracing::error!("X11Wm falhou: {err}"),
            }
        }
        XWaylandEvent::Error => tracing::error!("XWayland falhou ao iniciar"),
    }) {
        tracing::error!("Event loop XWayland: {err}");
    }
}

pub fn make_event_loop() -> EventLoop<'static, State> {
    EventLoop::try_new().expect("calloop")
}

/// Processa eventos do XWayland (chamar a cada frame no TTY e no winit).
pub fn dispatch(loop_: &mut EventLoop<'static, State>, state: &mut State) {
    use std::time::Duration;
    // Limita trabalho X11 por frame — evita starvation do libinput/DRM.
    const MAX_ROUNDS: usize = 8;
    for _ in 0..MAX_ROUNDS {
        if loop_.dispatch(Duration::ZERO, state).is_err() {
            break;
        }
    }
    if state.x11_focus_pending && state.x11_input_wanted {
        let pending = state.focused_x11;
        let ready = state
            .x11_apps
            .get(pending)
            .and_then(|a| a.surface.wl_surface())
            .is_some();
        if ready {
            state.x11_focus_pending = false;
            state.apply_focus();
        }
    }
}

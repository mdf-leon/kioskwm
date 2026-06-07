use std::{
    os::unix::io::OwnedFd,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use smithay::backend::renderer::element::memory::MemoryRenderBuffer;

pub struct SettingsPanelCache {
    pub buffer: MemoryRenderBuffer,
    pub key: u64,
}

pub struct ContextMenuCache {
    pub buffer: MemoryRenderBuffer,
    pub key: u64,
}

pub struct AltTabCache {
    pub buffer: MemoryRenderBuffer,
    pub key: u64,
}

use smithay::{
    backend::{
        allocator::{dmabuf::Dmabuf, format::FormatSet, Buffer},
        renderer::{buffer_dimensions, buffer_type},
    },
    delegate_compositor, delegate_data_device, delegate_dmabuf, delegate_output,
    delegate_primary_selection, delegate_seat, delegate_shm, delegate_xdg_shell,
    delegate_xwayland_keyboard_grab,
    desktop::{PopupKind, PopupManager, PopupPointerGrab},
    input::{
        keyboard::KeyboardHandle,
        pointer::{Focus, MotionEvent, PointerHandle},
        Seat, SeatHandler, SeatState,
    },
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::wayland_server::{protocol::wl_seat, Display, DisplayHandle},
    utils::{Logical, Point, Rectangle, Serial, Size, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            with_states, with_surface_tree_downward, BufferAssignment, CompositorClientState,
            CompositorHandler, CompositorState, SurfaceAttributes, TraversalAction,
        },
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        output::{OutputHandler, OutputManagerState},
        selection::{
            data_device::{
                set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
                ServerDndGrabHandler,
            },
            primary_selection::{
                set_primary_focus, PrimarySelectionHandler, PrimarySelectionState,
            },
            SelectionHandler,
        },
        shell::xdg::{
            PopupSurface, PositionerState, SurfaceCachedState, ToplevelSurface,
            XdgShellHandler, XdgShellState, XDG_POPUP_ROLE,
        },
        shm::{ShmHandler, ShmState},
        xwayland_keyboard_grab::{XWaylandKeyboardGrabHandler, XWaylandKeyboardGrabState},
        xwayland_shell::XWaylandShellState,
    },
    xwayland::{xwm::X11Wm, XWaylandClientData},
};
use wayland_protocols::xdg::shell::server::xdg_toplevel;
use wayland_server::{
    backend::{ClientData, ClientId, DisconnectReason},
    protocol::{
        wl_buffer,
        wl_surface::WlSurface,
    },
    Client, ListeningSocket, Resource,
};

pub struct State {
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub dmabuf_state: DmabufState,
    pub _dmabuf_global: Option<DmabufGlobal>,
    pub _output_state: OutputManagerState,
    pub seat_state: SeatState<Self>,
    pub seat: Seat<Self>,
    pub data_device_state: DataDeviceState,
    pub primary_selection_state: PrimarySelectionState,
    pub display_handle: DisplayHandle,
    pub output: Output,
    pub keyboard: KeyboardHandle<Self>,
    pub pointer: PointerHandle<Self>,
    pub running_apps: Vec<crate::apps::RunningApp>,
    pub x11_apps: Vec<crate::apps::X11App>,
    pub focused_app: usize,
    pub focused_x11: usize,
    pub focused_is_x11: bool,
    /// Foco X11 pedido mas wl_surface ainda não associada — reaplicar em dispatch.
    pub x11_focus_pending: bool,
    /// Input deve ir ao X11 (false ao mapear: só sobe z-order, Konsole segue respondendo).
    pub x11_input_wanted: bool,
    /// Índice X11 recém-aberto — auto-foco só no primeiro map, não em remaps.
    pub x11_autofocus_idx: Option<usize>,
    /// apply_focus adiado enquanto menu/painel WM estão abertos.
    pub deferred_focus: bool,
    pub xwayland_shell_state: XWaylandShellState,
    pub xwayland_keyboard_grab_state: XWaylandKeyboardGrabState,
    pub xwm: Option<X11Wm>,
    pub x11_display: Option<u32>,
    /// Cliente Wayland do XWayland — toplevels dele não vão para running_apps.
    pub xwayland_client: Option<wayland_server::backend::ClientId>,
    pub active_popup: Option<PopupSurface>,
    pub popup_manager: PopupManager,
    pub pointer_pos: Point<f64, Logical>,
    pub output_size: Size<i32, Logical>,
    /// TTY=Normal; winit aninhado=Flipped180.
    pub output_transform: Transform,
    /// TTY desenha cursor do compositor; winit deixa o cliente/pai cuidar.
    pub draw_compositor_cursor: bool,
    pub exit_requested: Arc<AtomicBool>,
    pub overlay_open: bool,
    /// Multiplicador de movimento do ponteiro (1.0 = padrao).
    pub pointer_speed: f64,
    pub settings: crate::settings::SettingsState,
    pub settings_cache: Mutex<Option<SettingsPanelCache>>,
    pub context_menu: crate::context_menu::ContextMenuState,
    pub context_menu_cache: Mutex<Option<ContextMenuCache>>,
    pub alt_tab: crate::alt_tab::AltTabState,
    pub alt_tab_cache: Mutex<Option<AltTabCache>>,
    /// Apps recentes (índices unificados) para Alt+Tab.
    pub app_mru: Vec<usize>,
    pub i18n: crate::i18n::I18n,
    pub mod_tracker: std::sync::Arc<crate::modifiers::ModifierTracker>,
    /// Super keys seen by the compositor keyboard (KDE may not deliver them).
    pub local_super_keys: u8,
    /// Render agendado — evita busy-loop desenhando só quando necessário.
    render_pending: bool,
    render_deadline: Option<Instant>,
    force_full_damage: bool,
    last_cursor_pos: Option<Point<f64, Logical>>,
    pointer_flush_pending: bool,
    pending_pointer_time: u32,
    serial: u32,
}

pub fn new_exit_flag() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

pub fn request_exit(flag: &AtomicBool) {
    flag.store(true, Ordering::SeqCst);
}

pub fn should_exit(flag: &AtomicBool) -> bool {
    flag.load(Ordering::SeqCst)
}

impl State {
    pub fn next_serial(&mut self) -> Serial {
        self.serial += 1;
        self.serial.into()
    }

    pub fn request_render(&mut self) {
        self.render_pending = true;
        self.render_deadline = None;
    }

    pub fn request_render_debounced(&mut self, delay: Duration) {
        self.render_pending = true;
        let deadline = Instant::now() + delay;
        self.render_deadline = Some(match self.render_deadline {
            Some(current) => current.min(deadline),
            None => deadline,
        });
    }

    pub fn take_render_pending(&mut self) -> bool {
        if !self.render_pending {
            return false;
        }
        if let Some(deadline) = self.render_deadline {
            if Instant::now() < deadline {
                return false;
            }
        }
        self.render_pending = false;
        self.render_deadline = None;
        true
    }

    pub fn note_full_damage(&mut self) {
        self.force_full_damage = true;
    }

    pub fn wants_full_damage(&self) -> bool {
        self.force_full_damage
            || self.overlay_open
            || self.context_menu.open
            || self.alt_tab.open
            || self.active_popup.is_some()
    }

    /// Overlay WM aberto — não compor apps por baixo (só painel/menu).
    pub fn wm_ui_obscures_apps(&self) -> bool {
        self.overlay_open || self.context_menu.open || self.alt_tab.open
    }

    pub fn last_cursor_pos(&self) -> Option<Point<f64, Logical>> {
        self.last_cursor_pos
    }

    pub fn set_last_cursor_pos(&mut self, pos: Point<f64, Logical>) {
        self.last_cursor_pos = Some(pos);
    }

    pub fn finish_frame_damage_state(&mut self, pointer: Option<Point<f64, Logical>>) {
        self.force_full_damage = false;
        if let Some(pos) = pointer {
            self.last_cursor_pos = Some(pos);
        }
    }

    pub fn mark_pointer_moved(&mut self, time: u32) {
        self.pointer_flush_pending = true;
        self.pending_pointer_time = time;
        crate::perf::record_pointer_move();
    }

    pub fn flush_pointer_to_client(&mut self) {
        if !self.pointer_flush_pending {
            return;
        }
        if self.overlay_open || self.context_menu.open || self.alt_tab.open {
            self.pointer_flush_pending = false;
            return;
        }
        self.pointer_flush_pending = false;
        let pointer = self.pointer.clone();
        let serial = self.next_serial();
        let focus = self.pointer_focus();
        pointer.motion(
            self,
            focus,
            &MotionEvent {
                location: self.pointer_pos,
                serial,
                time: self.pending_pointer_time,
            },
        );
        pointer.frame(self);
    }

    pub fn needs_x11_dispatch(&self) -> bool {
        !self.x11_apps.is_empty() || self.x11_focus_pending
    }

    pub fn wayland_dispatch_rounds(&self) -> usize {
        if self.overlay_open {
            return 1;
        }
        if self.active_popup.is_some() {
            return 10;
        }
        if self.context_menu.open || self.alt_tab.open {
            return 4;
        }
        1
    }

    pub fn wayland_post_frame_rounds(&self) -> usize {
        if self.active_popup.is_some() {
            return 10;
        }
        if self.context_menu.open || self.alt_tab.open || self.overlay_open {
            return 4;
        }
        2
    }

    fn note_surface_commit(&mut self, surface: &WlSurface) {
        if self.wm_ui_obscures_apps() {
            return;
        }
        let has_buffer = with_states(surface, |states| {
            states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .buffer
                .is_some()
        });
        if !has_buffer {
            return;
        }
        self.note_full_damage();
        self.request_render();
    }

    fn is_xwayland_client(&self, surface: &WlSurface) -> bool {
        let Some(client) = surface.client() else {
            return false;
        };
        if client.get_data::<XWaylandClientData>().is_some() {
            return true;
        }
        self.xwayland_client
            .as_ref()
            .is_some_and(|xwl| client.id() == *xwl)
    }

    /// Posição de desenho no compositor.
    /// `popup_offset` vem do PopupManager (soma dos configure.loc na árvore).
    /// Qt/Konsole desenha buffers na origem da wl_surface — não subtrair window_geometry.
    pub fn popup_render_offset(
        &self,
        popup: &PopupKind,
        popup_offset: smithay::utils::Point<i32, Logical>,
    ) -> (i32, i32) {
        let _ = popup;
        let tg = self.toplevel_window_geometry();
        (tg.loc.x + popup_offset.x, tg.loc.y + popup_offset.y)
    }

    pub(crate) fn popup_render_offset_for(&self, wl: &WlSurface) -> Option<(i32, i32)> {
        for app in &self.running_apps {
            for (popup, popup_offset) in PopupManager::popups_for_surface(app.surface.wl_surface()) {
                if popup.wl_surface() == wl {
                    return Some(self.popup_render_offset(&popup, popup_offset));
                }
            }
        }
        None
    }

    fn topmost_popup(&self) -> Option<PopupKind> {
        let wl = if self.x11_input_active() {
            self.x11_apps
                .get(self.focused_x11)
                .and_then(|a| a.surface.wl_surface())?
        } else {
            self.focused_toplevel()?.wl_surface().clone()
        };
        PopupManager::popups_for_surface(&wl).next().map(|(p, _)| p)
    }

    pub fn toplevel_window_geometry(&self) -> Rectangle<i32, Logical> {
        let Some(toplevel) = self.focused_toplevel() else {
            return Rectangle::from_size(self.output_size);
        };
        with_states(toplevel.wl_surface(), |states| {
            states
                .cached_state
                .get::<SurfaceCachedState>()
                .current()
                .geometry
                .filter(|g| g.size.w > 0 && g.size.h > 0)
                .unwrap_or_else(|| Rectangle::from_size(self.output_size))
        })
    }

    /// Retângulo de constraint para o positioner (coords da superfície do pai).
    /// No kiosk o toplevel cobre o output; usar o tamanho do output evita que
    /// submenus sejam deslizados para dentro do popup pai (slide_x).
    fn parent_constraint_rect(&self, _popup: &PopupSurface) -> Rectangle<i32, Logical> {
        Rectangle::from_size(self.output_size)
    }

    pub fn x11_input_active(&self) -> bool {
        self.focused_is_x11
            && self.x11_input_wanted
            && !self.x11_focus_pending
            && self
                .x11_apps
                .get(self.focused_x11)
                .and_then(|a| a.surface.wl_surface())
                .is_some()
    }

    pub fn pointer_focus(&self) -> Option<(WlSurface, Point<f64, Logical>)> {
        if let Some(topmost) = self.topmost_popup() {
            let surface = topmost.wl_surface().clone();
            let origin = self.surface_origin_for(&surface);
            return Some((surface, origin));
        }
        match self.active_target() {
            Some(crate::apps::ActiveTarget::X11(i)) => {
                let surface = self.x11_apps.get(i)?.surface.wl_surface()?;
                Some((surface, Point::from((0.0, 0.0))))
            }
            Some(crate::apps::ActiveTarget::Wayland(i)) => {
                let surface = self.running_apps.get(i)?.surface.wl_surface().clone();
                let origin = self.surface_origin_for(&surface);
                Some((surface, origin))
            }
            None => None,
        }
    }

    fn restore_pointer_to_toplevel(&mut self) {
        if let Some(toplevel) = self.focused_toplevel() {
            let wl_surface = toplevel.wl_surface().clone();
            let pointer = self.pointer.clone();
            let origin = self.surface_origin_for(&wl_surface);
            let serial = self.next_serial();
            pointer.motion(
                self,
                Some((wl_surface, origin)),
                &MotionEvent {
                    location: self.pointer_pos,
                    serial,
                    time: 0,
                },
            );
            pointer.frame(self);
        }
    }

    fn restore_toplevel_focus(&mut self) {
        self.apply_focus();
    }

    fn configure_popup(&mut self, surface: &PopupSurface, positioner: PositionerState) {
        let parent_rect = self.parent_constraint_rect(surface);
        let geometry = positioner.get_unconstrained_geometry(parent_rect);
        let nested = surface
            .get_parent_surface()
            .is_some_and(|p| smithay::wayland::compositor::get_role(&p) == Some(XDG_POPUP_ROLE));
        let kind = PopupKind::Xdg(surface.clone());
        let tree_offset = smithay::desktop::get_popup_toplevel_coords(&kind) + geometry.loc;
        let tg = self.toplevel_window_geometry();
        tracing::info!(
            "popup {} configure rel=({}, {}) compositor~=({}, {}) size={}x{} parent_rect={}x{}",
            if nested { "submenu" } else { "menu" },
            geometry.loc.x,
            geometry.loc.y,
            tg.loc.x + tree_offset.x,
            tg.loc.y + tree_offset.y,
            geometry.size.w,
            geometry.size.h,
            parent_rect.size.w,
            parent_rect.size.h,
        );
        surface.with_pending_state(|state| {
            state.geometry = geometry;
        });
        match surface.send_configure() {
            Ok(_) => tracing::debug!("popup configurado"),
            Err(err) => tracing::warn!("popup configure: {:?}", err),
        }
    }

    pub fn maintain_popups(&mut self) {
        self.popup_manager.cleanup();
    }

    fn log_popup_tree(surface: &WlSurface) {
        let mut nodes = 0u32;
        let mut mapped = 0u32;
        with_surface_tree_downward(
            surface,
            (),
            |_, _, &()| TraversalAction::DoChildren(()),
            |_s, states, &()| {
                nodes += 1;
                let mut guard = states.cached_state.get::<SurfaceAttributes>();
                let attrs = guard.current();
                let Some(BufferAssignment::NewBuffer(buf)) = &attrs.buffer else {
                    return;
                };
                let Some(dims) = buffer_dimensions(buf) else {
                    tracing::warn!("popup commit: buffer tipo desconhecido");
                    return;
                };
                mapped += 1;
                tracing::debug!(
                    "popup commit: {:?} {}x{}",
                    buffer_type(buf),
                    dims.w,
                    dims.h
                );
            },
            |_, _, &()| true,
        );
        if mapped == 0 {
            tracing::trace!("popup commit sem buffer ({nodes} superfície(s))");
        }
    }

    pub fn register_dmabuf_formats(&mut self, formats: FormatSet) {
        if self._dmabuf_global.is_some() {
            return;
        }
        let formats: Vec<_> = formats.into_iter().collect();
        tracing::info!("Registrando linux-dmabuf ({} formatos)", formats.len());
        self._dmabuf_global = Some(
            self.dmabuf_state
                .create_global::<Self>(&self.display_handle, formats),
        );
    }

    pub fn configure_kiosk(&mut self, surface: &ToplevelSurface) {
        self.configure_toplevel(surface, true);
    }

    fn configure_toplevel(&mut self, surface: &ToplevelSurface, activated: bool) {
        let size = self.output_size;
        surface.with_pending_state(|state| {
            state.size = Some(size);
            state.bounds = Some(size);
            state.states.set(xdg_toplevel::State::Maximized);
            if activated {
                state.states.set(xdg_toplevel::State::Activated);
            } else {
                state.states.unset(xdg_toplevel::State::Activated);
            }
        });
        surface.send_configure();
    }

    fn reconfigure_all_toplevels(&mut self) {
        let surfaces = self
            .xdg_shell_state
            .toplevel_surfaces()
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        for surface in surfaces {
            self.configure_kiosk(&surface);
        }
    }

    pub fn update_output_mode(&mut self, physical: smithay::utils::Size<i32, smithay::utils::Physical>) {
        self.output_size = physical.to_logical(1);
        let mode = Mode {
            size: physical,
            refresh: 60_000,
        };
        self.output.change_current_state(
            Some(mode),
            None,
            Some(Scale::Integer(1)),
            Some((0, 0).into()),
        );
        self.note_full_damage();
        self.request_render();
        self.reconfigure_all_toplevels();
    }
}

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, dmabuf: Dmabuf, notifier: ImportNotifier) {
        tracing::debug!(
            "dmabuf import {}x{} fmt={:?}",
            dmabuf.width(),
            dmabuf.height(),
            dmabuf.format()
        );
        if let Err(err) = notifier.successful::<Self>() {
            tracing::warn!("dmabuf import falhou: {:?}", err);
        }
    }
}

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        if self.is_xwayland_client(surface.wl_surface()) {
            tracing::debug!("Ignorando toplevel XWayland (gerido por x11_apps)");
            self.purge_xwayland_from_running_apps();
            return;
        }
        self.purge_xwayland_from_running_apps();
        let name = crate::apps::display_name(&surface);
        let index = self.running_apps.len();
        let is_first = index == 0;
        tracing::info!(
            "Nova app: {name} (índice {index}, foco={})",
            if is_first { "sim" } else { "background" }
        );
        if is_first {
            self.configure_kiosk(&surface);
        } else {
            self.configure_toplevel(&surface, false);
        }
        self.running_apps.push(crate::apps::RunningApp {
            surface: surface.clone(),
            display_name: name.clone(),
        });
        if is_first {
            self.focused_is_x11 = false;
            self.focused_app = index;
            self.touch_mru(self.unified_focus_index());
            self.apply_focus();
        } else {
            let foco = self.unified_app_name(self.unified_focus_index());
            tracing::info!("Nova app em background: {name} — foco mantém {foco}");
        }
        self.sync_app_mru();
        crate::alt_tab::invalidate_cache(self);
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        self.sync_app_name(&surface);
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        self.sync_app_name(&surface);
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        if let Err(err) = self
            .popup_manager
            .track_popup(PopupKind::Xdg(surface.clone()))
        {
            tracing::warn!("track_popup: {:?}", err);
        }
        self.configure_popup(&surface, positioner);
    }

    fn grab(&mut self, surface: PopupSurface, _seat: wl_seat::WlSeat, serial: Serial) {
        let Some(toplevel) = self
            .focused_toplevel()
            .cloned()
            .or_else(|| {
                self.running_apps
                    .first()
                    .map(|a| a.surface.clone())
            })
        else {
            tracing::warn!("popup grab sem toplevel");
            return;
        };

        let pointer = self.pointer.clone();

        let root = toplevel.wl_surface().clone();
        let popup = PopupKind::Xdg(surface.clone());
        let popup_grab = match self.popup_manager.grab_popup(root, popup, &self.seat, serial) {
            Ok(grab) => {
                tracing::info!("popup grab ok (serial={serial:?})");
                grab
            }
            Err(err) => {
                tracing::warn!("grab_popup falhou: {:?}", err);
                return;
            }
        };

        // Grab de ponteiro (sem mudar foco do teclado — evita piscar o Konsole).
        let pointer_grab = PopupPointerGrab::new(&popup_grab);
        pointer.set_grab(self, pointer_grab, serial, Focus::Keep);

        self.active_popup = Some(surface.clone());
        let wl_surface = surface.wl_surface().clone();
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
    }

    fn show_window_menu(
        &mut self,
        surface: ToplevelSurface,
        _seat: wl_seat::WlSeat,
        _serial: Serial,
        location: Point<i32, Logical>,
    ) {
        if let Some(idx) = self
            .running_apps
            .iter()
            .position(|a| a.surface.wl_surface() == surface.wl_surface())
        {
            self.focus_app(idx);
        }
        crate::context_menu::handlers::open_at_logical(
            self,
            location.x as f64,
            location.y as f64,
        );
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        let parent_rect = self.parent_constraint_rect(&surface);
        let geometry = positioner.get_unconstrained_geometry(parent_rect);
        surface.with_pending_state(|state| {
            state.geometry = geometry;
        });
        let _ = surface.send_repositioned(token);
        let _ = surface.send_configure();
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let Some(pos) = self
            .running_apps
            .iter()
            .position(|a| a.surface.wl_surface() == surface.wl_surface())
        else {
            return;
        };
        let name = self.running_apps[pos].display_name.clone();
        tracing::info!("App fechada: {name}");
        self.running_apps.remove(pos);
        if self.running_apps.is_empty() && self.x11_apps.is_empty() {
            tracing::info!("Nenhuma app restante — fechando compositor");
            request_exit(&self.exit_requested);
            return;
        }
        if self.focused_app >= self.running_apps.len() {
            self.focused_app = self.running_apps.len() - 1;
        } else if self.focused_app > pos {
            self.focused_app -= 1;
        }
        self.dismiss_popup_grab();
        self.apply_focus();
    }

    fn popup_destroyed(&mut self, surface: PopupSurface) {
        tracing::debug!("popup destruído");
        let was_active = self
            .active_popup
            .as_ref()
            .is_some_and(|p| p.wl_surface() == surface.wl_surface());

        if !was_active {
            return;
        }

        // Submenu fechou: volta ao popup pai, não derruba o menu inteiro.
        if let Some(topmost) = self.topmost_popup() {
            if let PopupKind::Xdg(parent) = topmost {
                self.active_popup = Some(parent.clone());
                let wl = parent.wl_surface().clone();
                let origin = self.surface_origin_for(&wl);
                let pointer = self.pointer.clone();
                let serial = self.next_serial();
                pointer.motion(
                    self,
                    Some((wl, origin)),
                    &MotionEvent {
                        location: self.pointer_pos,
                        serial,
                        time: 0,
                    },
                );
                pointer.frame(self);
                return;
            }
        }

        self.active_popup = None;
        if self.pointer.is_grabbed() {
            let pointer = self.pointer.clone();
            let serial = self.next_serial();
            pointer.unset_grab(self, serial, 0);
        }
        self.restore_pointer_to_toplevel();
    }
}

impl SelectionHandler for State {
    type SelectionUserData = ();
}

impl PrimarySelectionHandler for State {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary_selection_state
    }
}

impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for State {}
impl ServerDndGrabHandler for State {
    fn send(&mut self, _mime_type: String, _fd: OwnedFd, _seat: Seat<Self>) {}
}

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        if let Some(data) = client.get_data::<ClientState>() {
            return &data.compositor_state;
        }
        &client
            .get_data::<XWaylandClientData>()
            .expect("cliente sem ClientState")
            .compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        let is_popup = smithay::wayland::compositor::get_role(surface) == Some(XDG_POPUP_ROLE);
        if is_popup {
            Self::log_popup_tree(surface);
        }
        smithay::backend::renderer::utils::on_commit_buffer_handler::<Self>(surface);
        self.popup_manager.commit(surface);
        self.note_surface_commit(surface);
    }
}

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl OutputHandler for State {}

impl XWaylandKeyboardGrabHandler for State {
    fn grab(
        &mut self,
        surface: WlSurface,
        seat: Seat<Self>,
        grab: smithay::wayland::xwayland_keyboard_grab::XWaylandKeyboardGrab<Self>,
    ) {
        tracing::info!("XWayland pediu keyboard grab");
        if let Some(idx) = self
            .x11_apps
            .iter()
            .position(|a| a.surface.wl_surface().as_ref() == Some(&surface))
        {
            self.focused_is_x11 = true;
            self.focused_x11 = idx;
            self.x11_input_wanted = true;
            self.x11_focus_pending = false;
        }
        if let Some(keyboard) = seat.get_keyboard() {
            let serial = self.next_serial();
            keyboard.set_grab(self, grab, serial);
            if !self.context_menu.open && !self.overlay_open && !self.alt_tab.open {
                self.apply_focus();
            } else {
                self.deferred_focus = true;
            }
        }
    }

    fn keyboard_focus_for_xsurface(&self, surface: &WlSurface) -> Option<WlSurface> {
        Some(surface.clone())
    }
}

impl SeatHandler for State {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let client = focused.and_then(|s| s.client());
        set_data_device_focus(&self.display_handle, seat, client.clone());
        set_primary_focus(&self.display_handle, seat, client);
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        _image: smithay::input::pointer::CursorImageStatus,
    ) {
    }
}

delegate_xdg_shell!(State);
delegate_compositor!(State);
delegate_dmabuf!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_data_device!(State);
delegate_primary_selection!(State);
delegate_output!(State);
delegate_xwayland_keyboard_grab!(State);

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {
        tracing::debug!("cliente conectado");
    }

    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {
        tracing::debug!("cliente desconectado: {:?}", _reason);
    }
}

pub fn init_state(
    dh: &DisplayHandle,
    output_name: &str,
    output_model: &str,
    physical_size: smithay::utils::Size<i32, smithay::utils::Physical>,
    monitor_mm: (i32, i32),
    exit_requested: Arc<AtomicBool>,
    i18n: crate::i18n::I18n,
    mod_tracker: std::sync::Arc<crate::modifiers::ModifierTracker>,
) -> Result<State, Box<dyn std::error::Error>> {
    let compositor_state = CompositorState::new::<State>(dh);
    let shm_state = ShmState::new::<State>(dh, vec![]);
    let output_state = OutputManagerState::new();
    let mut seat_state = SeatState::new();
    let mut seat = seat_state.new_wl_seat(dh, "kioskwm-seat");

    let output = Output::new(
        output_name.into(),
        PhysicalProperties {
            size: monitor_mm.into(),
            subpixel: Subpixel::Unknown,
            make: "kioskwm".into(),
            model: output_model.into(),
        },
    );
    let mode = Mode {
        size: physical_size,
        refresh: 60_000,
    };
    output.set_preferred(mode);
    output.change_current_state(
        Some(mode),
        None,
        Some(Scale::Integer(1)),
        Some((0, 0).into()),
    );
    let _global = output.create_global::<State>(dh);

    let keyboard = seat.add_keyboard(Default::default(), 200, 200)?;
    let pointer = seat.add_pointer();
    let logical_size = physical_size.to_logical(1);

    Ok(State {
        compositor_state,
        xdg_shell_state: XdgShellState::new::<State>(dh),
        shm_state,
        dmabuf_state: DmabufState::new(),
        _dmabuf_global: None,
        _output_state: output_state,
        seat_state,
        seat,
        data_device_state: DataDeviceState::new::<State>(dh),
        primary_selection_state: PrimarySelectionState::new::<State>(dh),
        display_handle: dh.clone(),
        output,
        keyboard,
        pointer,
        running_apps: Vec::new(),
        x11_apps: Vec::new(),
        focused_app: 0,
        focused_x11: 0,
        focused_is_x11: false,
        x11_focus_pending: false,
        x11_input_wanted: false,
        x11_autofocus_idx: None,
        deferred_focus: false,
        xwayland_shell_state: XWaylandShellState::new::<State>(dh),
        xwayland_keyboard_grab_state: XWaylandKeyboardGrabState::new::<State>(dh),
        xwm: None,
        x11_display: None,
        xwayland_client: None,
        active_popup: None,
        popup_manager: PopupManager::default(),
        pointer_pos: Point::from((logical_size.w as f64 / 2.0, logical_size.h as f64 / 2.0)),
        output_size: logical_size,
        output_transform: Transform::Normal,
        draw_compositor_cursor: output_model == "tty",
        exit_requested,
        overlay_open: false,
        pointer_speed: 1.0,
        settings: crate::settings::SettingsState::default(),
        settings_cache: Mutex::new(None),
        context_menu: crate::context_menu::ContextMenuState::default(),
        context_menu_cache: Mutex::new(None),
        alt_tab: crate::alt_tab::AltTabState::default(),
        alt_tab_cache: Mutex::new(None),
        app_mru: Vec::new(),
        i18n,
        mod_tracker,
        local_super_keys: 0,
        render_pending: true,
        render_deadline: None,
        force_full_damage: true,
        last_cursor_pos: None,
        pointer_flush_pending: false,
        pending_pointer_time: 0,
        serial: 0,
    })
}

pub fn accept_clients(
    display: &mut Display<State>,
    state: &mut State,
    listener: &ListeningSocket,
) -> Result<(), Box<dyn std::error::Error>> {
    accept_clients_rounds(display, state, listener, state.wayland_dispatch_rounds())
}

pub fn accept_clients_rounds(
    display: &mut Display<State>,
    state: &mut State,
    listener: &ListeningSocket,
    rounds: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(stream) = listener.accept()? {
        display
            .handle()
            .insert_client(stream, Arc::new(ClientState::default()))?;
    }
    for _ in 0..rounds {
        display.dispatch_clients(state)?;
        display.flush_clients()?;
    }
    Ok(())
}

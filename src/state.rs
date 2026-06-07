use std::{
    os::unix::io::OwnedFd,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use smithay::{
    backend::{
        allocator::{dmabuf::Dmabuf, format::FormatSet, Buffer},
        renderer::{buffer_dimensions, buffer_type},
    },
    delegate_compositor, delegate_data_device, delegate_dmabuf, delegate_output,
    delegate_primary_selection, delegate_seat, delegate_shm, delegate_xdg_shell,
    desktop::{PopupKind, PopupManager, PopupPointerGrab},
    input::{
        keyboard::KeyboardHandle,
        pointer::{Focus, MotionEvent, PointerHandle},
        Seat, SeatHandler, SeatState,
    },
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::wayland_server::{protocol::wl_seat, Display, DisplayHandle},
    utils::{Logical, Point, Rectangle, Serial, Size},
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
    },
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
    pub primary_toplevel: Option<ToplevelSurface>,
    pub active_popup: Option<PopupSurface>,
    pub popup_manager: PopupManager,
    pub pointer_pos: Point<f64, Logical>,
    pub output_size: Size<i32, Logical>,
    pub exit_requested: Arc<AtomicBool>,
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
        let toplevel = self.primary_toplevel.as_ref()?.wl_surface();
        for (popup, popup_offset) in PopupManager::popups_for_surface(toplevel) {
            if popup.wl_surface() == wl {
                return Some(self.popup_render_offset(&popup, popup_offset));
            }
        }
        None
    }

    fn topmost_popup(&self) -> Option<PopupKind> {
        let toplevel = self.primary_toplevel.as_ref()?.wl_surface();
        // DFS: filhos antes do pai — o primeiro é o popup mais profundo (submenu ativo).
        PopupManager::popups_for_surface(toplevel).next().map(|(p, _)| p)
    }

    pub fn toplevel_window_geometry(&self) -> Rectangle<i32, Logical> {
        let Some(toplevel) = self.primary_toplevel.as_ref() else {
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

    /// Origem da superfície focada no espaço do compositor (não a posição do cursor).
    fn surface_origin_for(&self, surface: &WlSurface) -> Point<f64, Logical> {
        if let Some((ox, oy)) = self.popup_render_offset_for(surface) {
            return Point::from((ox as f64, oy as f64));
        }
        Point::from((0.0, 0.0))
    }

    /// Retângulo de constraint para o positioner (coords da superfície do pai).
    /// No kiosk o toplevel cobre o output; usar o tamanho do output evita que
    /// submenus sejam deslizados para dentro do popup pai (slide_x).
    fn parent_constraint_rect(&self, _popup: &PopupSurface) -> Rectangle<i32, Logical> {
        Rectangle::from_size(self.output_size)
    }

    pub fn pointer_focus(&self) -> Option<(WlSurface, Point<f64, Logical>)> {
        let surface = if let Some(topmost) = self.topmost_popup() {
            topmost.wl_surface().clone()
        } else {
            self.primary_toplevel.as_ref()?.wl_surface().clone()
        };
        let origin = self.surface_origin_for(&surface);
        Some((surface, origin))
    }

    fn restore_pointer_to_toplevel(&mut self) {
        if let Some(toplevel) = &self.primary_toplevel {
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
        if let Some(toplevel) = &self.primary_toplevel {
            let wl_surface = toplevel.wl_surface().clone();
            let pos = self.pointer_pos;
            let keyboard = self.keyboard.clone();
            let pointer = self.pointer.clone();
            let focus_serial = self.next_serial();
            let motion_serial = self.next_serial();
            keyboard.set_focus(self, Some(wl_surface.clone()), focus_serial);
            let origin = self.surface_origin_for(&wl_surface);
            pointer.motion(
                self,
                Some((wl_surface, origin)),
                &MotionEvent {
                    location: pos,
                    serial: motion_serial,
                    time: 0,
                },
            );
            pointer.frame(self);
        }
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
        let size = self.output_size;
        surface.with_pending_state(|state| {
            state.size = Some(size);
            state.bounds = Some(size);
            state.states.set(xdg_toplevel::State::Activated);
            state.states.set(xdg_toplevel::State::Maximized);
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
        if self.primary_toplevel.is_some() {
            tracing::warn!("Janela extra ignorada (modo kiosk)");
            return;
        }

        tracing::info!("Terminal detectado, aplicando modo kiosk");
        self.configure_kiosk(&surface);
        self.primary_toplevel = Some(surface.clone());

        let wl_surface = surface.wl_surface().clone();
        let keyboard = self.keyboard.clone();
        let pointer = self.pointer.clone();
        let focus_serial = self.next_serial();
        let motion_serial = self.next_serial();
        keyboard.set_focus(self, Some(wl_surface.clone()), focus_serial);
        let origin = self.surface_origin_for(&wl_surface);
        pointer.motion(
            self,
            Some((wl_surface.clone(), origin)),
            &MotionEvent {
                location: self.pointer_pos,
                serial: motion_serial,
                time: 0,
            },
        );
        pointer.frame(self);
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
        let Some(toplevel) = self.primary_toplevel.clone() else {
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
        let was_primary = self
            .primary_toplevel
            .as_ref()
            .is_some_and(|p| p.wl_surface() == surface.wl_surface());

        if was_primary {
            tracing::info!("Terminal encerrado (Ctrl+D) — fechando compositor");
            self.primary_toplevel = None;
            request_exit(&self.exit_requested);
        }
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
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        let is_popup = smithay::wayland::compositor::get_role(surface) == Some(XDG_POPUP_ROLE);
        if is_popup {
            Self::log_popup_tree(surface);
        }
        smithay::backend::renderer::utils::on_commit_buffer_handler::<Self>(surface);
        self.popup_manager.commit(surface);
    }
}

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl OutputHandler for State {}

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
        primary_toplevel: None,
        active_popup: None,
        popup_manager: PopupManager::default(),
        pointer_pos: Point::from((logical_size.w as f64 / 2.0, logical_size.h as f64 / 2.0)),
        output_size: logical_size,
        exit_requested,
        serial: 0,
    })
}

pub fn accept_clients(
    display: &mut Display<State>,
    state: &mut State,
    listener: &ListeningSocket,
) -> Result<(), Box<dyn std::error::Error>> {
    accept_clients_rounds(display, state, listener, 2)
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

use std::{
    os::unix::io::OwnedFd,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use smithay::{
    delegate_compositor, delegate_data_device, delegate_output, delegate_seat, delegate_shm,
    delegate_xdg_shell,
    input::{
        keyboard::KeyboardHandle,
        pointer::{MotionEvent, PointerHandle},
        Seat, SeatHandler, SeatState,
    },
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::wayland_server::{protocol::wl_seat, Display, DisplayHandle},
    utils::{Logical, Point, Serial, Size},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState,
        },
        output::{OutputHandler, OutputManagerState},
        selection::{
            data_device::{ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler},
            SelectionHandler,
        },
        shell::xdg::{PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState},
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
    Client, ListeningSocket,
};

pub struct State {
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub _output_state: OutputManagerState,
    pub seat_state: SeatState<Self>,
    pub data_device_state: DataDeviceState,
    pub output: Output,
    pub keyboard: KeyboardHandle<Self>,
    pub pointer: PointerHandle<Self>,
    pub primary_toplevel: Option<ToplevelSurface>,
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

    pub fn pointer_focus(&self) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.primary_toplevel
            .as_ref()
            .map(|toplevel| (toplevel.wl_surface().clone(), Point::from((0.0, 0.0))))
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
        pointer.motion(
            self,
            Some((wl_surface, Point::from((0.0, 0.0)))),
            &MotionEvent {
                location: Point::from((0.0, 0.0)),
                serial: motion_serial,
                time: 0,
            },
        );
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {}
    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}
    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
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
}

impl SelectionHandler for State {
    type SelectionUserData = ();
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
        smithay::backend::renderer::utils::on_commit_buffer_handler::<Self>(surface);
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

    fn focus_changed(&mut self, _seat: &Seat<Self>, _focused: Option<&WlSurface>) {}
    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        _image: smithay::input::pointer::CursorImageStatus,
    ) {
    }
}

delegate_xdg_shell!(State);
delegate_compositor!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_data_device!(State);
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

    Ok(State {
        compositor_state,
        xdg_shell_state: XdgShellState::new::<State>(dh),
        shm_state,
        _output_state: output_state,
        seat_state,
        data_device_state: DataDeviceState::new::<State>(dh),
        output,
        keyboard,
        pointer,
        primary_toplevel: None,
        output_size: physical_size.to_logical(1),
        exit_requested,
        serial: 0,
    })
}

pub fn accept_clients(
    display: &mut Display<State>,
    state: &mut State,
    listener: &ListeningSocket,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(stream) = listener.accept()? {
        display
            .handle()
            .insert_client(stream, Arc::new(ClientState::default()))?;
    }
    display.dispatch_clients(state)?;
    display.flush_clients()?;
    Ok(())
}

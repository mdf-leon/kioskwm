use std::{
    path::Path,
    time::{Duration, Instant},
};

use calloop::{
    signals::{Signal, Signals},
    timer::{TimeoutAction, Timer},
    EventLoop, LoopHandle, LoopSignal,
};
use smithay::{
    backend::{
        allocator::{
            format::FormatSet,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
            Fourcc,
        },
        drm::{DrmDevice, DrmDeviceFd, DrmEvent, GbmBufferedSurface},
        egl::{context::ContextPriority, EGLContext, EGLDisplay},
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{gles::GlesRenderer, Bind, ImportDma},
        session::{
            libseat::LibSeatSession,
            Event as SessionEvent, Session as LibSeatSessionTrait,
        },
        udev::primary_gpu,
    },
    reexports::{
        drm::control::{connector, crtc, Device as ControlDevice, ModeTypeFlags},
        input::Libinput,
        rustix::fs::OFlags,
        wayland_server::Display,
    },
    utils::{DeviceFd, Rectangle, Transform},
};

use crate::{
    cursor::PointerCursor,
    input::{handle_input, PointerTracker},
    context_menu::ContextMenuControl,
    emergency::EmergencyContext,
    hardware_bridge::HardwareBridge,
    kill_switch,
    overlay::OverlayControl,
    render::{render_kiosk_frame, send_frame_callbacks},
    spawn::{
        bind_wayland_socket, log_bound_socket, prepare_runtime_files, resolve_spawn,
        schedule_spawn,
    },
    state::{accept_clients, accept_clients_rounds, init_state, new_exit_flag, should_exit, State},
};
use crate::Args;
use wayland_server::ListeningSocket;

const COLOR_FORMATS: &[Fourcc] = &[Fourcc::Abgr8888, Fourcc::Argb8888];

struct OutputConfig {
    connector: connector::Handle,
    crtc: crtc::Handle,
    mode: smithay::reexports::drm::control::Mode,
    physical_size: smithay::utils::Size<i32, smithay::utils::Physical>,
    monitor_mm: (i32, i32),
}

struct TtyLoop {
    state: State,
    x11_loop: smithay::reexports::calloop::EventLoop<'static, State>,
    display: Display<State>,
    listener: ListeningSocket,
    device: DrmDevice,
    gbm_surface: GbmBufferedSurface<GbmAllocator<DrmDeviceFd>, ()>,
    renderer: GlesRenderer,
    libinput: Libinput,
    session: LibSeatSession,
    pointer_tracker: PointerTracker,
    pointer_cursor: PointerCursor,
    overlay: std::sync::Arc<OverlayControl>,
    emergency: std::sync::Arc<EmergencyContext>,
    hardware: std::sync::Arc<HardwareBridge>,
    loop_signal: LoopSignal,
    start_time: Instant,
}

fn stop_loop(data: &mut TtyLoop) {
    if should_exit(&data.state.exit_requested) {
        tracing::info!("Encerrando compositor…");
        data.loop_signal.stop();
    }
}

fn find_output(device: &DrmDevice) -> Result<OutputConfig, Box<dyn std::error::Error>> {
    let resources = device.resource_handles()?;

    for &conn_handle in resources.connectors() {
        let info = device.get_connector(conn_handle, false)?;
        if info.state() != connector::State::Connected || info.modes().is_empty() {
            continue;
        }

        let mode = info
            .modes()
            .iter()
            .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
            .or_else(|| info.modes().first())
            .cloned()
            .ok_or("monitor sem modo válido")?;

        let mut selected_crtc = None;
        'enc: for enc_handle in info.encoders() {
            let enc = device.get_encoder(*enc_handle)?;
            for crtc_handle in resources.filter_crtcs(enc.possible_crtcs()) {
                selected_crtc = Some(crtc_handle);
                break 'enc;
            }
        }

        let crtc = selected_crtc.ok_or("nenhum CRTC compatível com o monitor")?;
        let monitor_mm = info
            .size()
            .map(|(w, h)| (w as i32, h as i32))
            .unwrap_or((480, 270));
        let physical_size =
            smithay::utils::Size::from((mode.size().0 as i32, mode.size().1 as i32));

        tracing::info!(
            "Monitor: {}x{}",
            mode.size().0,
            mode.size().1
        );

        return Ok(OutputConfig {
            connector: conn_handle,
            crtc,
            mode,
            physical_size,
            monitor_mm,
        });
    }

    Err("nenhum monitor conectado encontrado".into())
}

fn request_render(handle: &LoopHandle<'_, TtyLoop>) {
    let _ = handle.insert_source(Timer::from_duration(Duration::from_millis(8)), |_, _, data| {
        if data.state.take_render_pending() {
            data.render_frame();
        }
        TimeoutAction::Drop
    });
}

fn request_render_immediate(handle: &LoopHandle<'_, TtyLoop>) {
    let _ = handle.insert_source(Timer::immediate(), |_, _, data| {
        if data.state.take_render_pending() {
            data.render_frame();
        }
        TimeoutAction::Drop
    });
}

impl TtyLoop {
    fn render_frame(&mut self) {
        self.emergency.menu.poll(&mut self.state);

        if !self.device.is_active() {
            return;
        }

        let size = self
            .state
            .output
            .current_mode()
            .map(|m| m.size)
            .unwrap_or_default();
        if size.w == 0 || size.h == 0 {
            return;
        }

        let Ok((mut dmabuf, _age)) = self.gbm_surface.next_buffer() else {
            tracing::warn!("next_buffer falhou");
            return;
        };

        if let Err(err) = accept_clients(&mut self.display, &mut self.state, &self.listener) {
            tracing::warn!("wayland dispatch: {}", err);
        }
        crate::x11::dispatch_if_needed(&mut self.x11_loop, &mut self.state);

        let time_ms = self.start_time.elapsed().as_millis() as u32;
        let damage = (|| -> Result<Vec<Rectangle<i32, smithay::utils::Physical>>, Box<dyn std::error::Error>> {
            let mut target = self.renderer.bind(&mut dmabuf)?;
            render_kiosk_frame(
                &mut self.renderer,
                &mut target,
                &mut self.state,
                size,
                Transform::Normal,
                Some(self.pointer_tracker.pos),
                Some(&self.pointer_cursor),
                time_ms,
            )
        })();

        let damage = match damage {
            Ok(d) => d,
            Err(err) => {
                tracing::warn!("render falhou: {}", err);
                return;
            }
        };

        if let Err(err) = self.gbm_surface.queue_buffer(None, Some(damage.clone()), ()) {
            tracing::warn!("queue_buffer falhou: {}", err);
        }

        send_frame_callbacks(&mut self.state, time_ms);

        let post_rounds = self.state.wayland_post_frame_rounds();
        if let Err(err) = accept_clients_rounds(
            &mut self.display,
            &mut self.state,
            &self.listener,
            post_rounds,
        ) {
            tracing::warn!("wayland dispatch pós-frame: {}", err);
        }

        stop_loop(self);
    }
}

pub fn run(args: Args, i18n: crate::i18n::I18n) -> Result<(), Box<dyn std::error::Error>> {
    ensure_tty_env()?;

    let spawn_plan = resolve_spawn(&args);

    let (session, session_notifier) = LibSeatSession::new().map_err(|err| {
        format!(
            "Falha ao abrir sessão libseat: {err}\n\
             Verifique se seatd está rodando e se você está no grupo 'seat'."
        )
    })?;

    let seat_name = session.seat();
    tracing::info!("Sessão libseat: seat={}", seat_name);

    let gpu_path = primary_gpu(&seat_name)?.ok_or("nenhuma GPU encontrada para este seat")?;
    tracing::info!("GPU primária: {}", gpu_path.display());

    let mut session = session;
    let owned_fd = session.open(Path::new(&gpu_path), OFlags::RDWR | OFlags::CLOEXEC)?;
    let device_fd = DrmDeviceFd::new(DeviceFd::from(owned_fd));

    let (mut device, drm_notifier) = DrmDevice::new(device_fd.clone(), false)?;
    let output = find_output(&device)?;

    let drm_surface = device.create_surface(output.crtc, output.mode, &[output.connector])?;

    let gbm_device = GbmDevice::new(device_fd.clone())?;
    let allocator = GbmAllocator::new(
        gbm_device.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );

    let display_egl = unsafe { EGLDisplay::new(gbm_device.clone())? };
    let context = EGLContext::new_with_priority(&display_egl, ContextPriority::High)?;
    let renderer = unsafe { GlesRenderer::new(context)? };
    let renderer_formats: FormatSet = renderer.dmabuf_formats();

    let gbm_surface = GbmBufferedSurface::new(
        drm_surface,
        allocator,
        COLOR_FORMATS,
        renderer_formats.clone(),
    )?;

    let display: Display<State> = Display::new()?;
    let dh = display.handle();
    let exit_requested = new_exit_flag();
    let mod_tracker = crate::modifiers::ModifierTracker::new_arc();
    let mut state = init_state(
        &dh,
        "kioskwm",
        "tty",
        output.physical_size,
        output.monitor_mm,
        exit_requested,
        i18n,
        mod_tracker.clone(),
    )?;
    state.register_dmabuf_formats(renderer_formats.clone());

    let mut x11_loop = crate::x11::make_event_loop();
    crate::x11::start(&x11_loop.handle(), &dh);

    let listener = bind_wayland_socket()?;
    let socket_name = listener
        .socket_name()
        .expect("socket wayland sem nome")
        .to_string_lossy()
        .into_owned();

    log_bound_socket(&socket_name);
    prepare_runtime_files(&socket_name);

    let overlay = OverlayControl::with_loop_wake(true);
    let menu = ContextMenuControl::with_loop_wake(true);
    let hardware = HardwareBridge::new_arc();
    let emergency = std::sync::Arc::new(EmergencyContext::new(
        state.exit_requested.clone(),
        overlay.clone(),
        menu,
    ));
    kill_switch::spawn(emergency.clone(), mod_tracker, hardware.clone());

    schedule_spawn(spawn_plan, socket_name, args.spawn_delay_ms);

    let session_for_loop = session.clone();
    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.into());
    libinput
        .udev_assign_seat(&seat_name)
        .map_err(|()| "falha ao associar libinput ao seat")?;
    let libinput_backend = LibinputInputBackend::new(libinput.clone());

    let mut event_loop: EventLoop<TtyLoop> = EventLoop::try_new()?;
    let handle = event_loop.handle();
    let loop_signal = event_loop.get_signal();

    let mut data = TtyLoop {
        pointer_tracker: PointerTracker::new(state.output_size),
        pointer_cursor: PointerCursor::load(),
        loop_signal: loop_signal.clone(),
        overlay,
        emergency,
        hardware,
        state,
        x11_loop,
        display,
        listener,
        device,
        gbm_surface,
        renderer,
        libinput,
        session: session_for_loop,
        start_time: Instant::now(),
    };

    let signal_handle = handle.clone();
    handle.insert_source(
        Signals::new(&[Signal::SIGINT, Signal::SIGTERM, Signal::SIGUSR1, Signal::SIGUSR2])?,
        {
            let loop_signal = loop_signal.clone();
            move |event, _, data| {
                if event.signal() == Signal::SIGUSR2 {
                    data.overlay.poll(&mut data.state);
                    data.emergency.menu.poll(&mut data.state);
                    data.state.request_render();
                    request_render_immediate(&signal_handle);
                } else {
                    tracing::info!("Sinal {:?} — fechando compositor", event.signal());
                    crate::state::request_exit(&data.state.exit_requested);
                    loop_signal.stop();
                }
            }
        },
    )?;

    handle.insert_source(libinput_backend, {
        let handle = handle.clone();
        move |event, _, data| {
            let mut tty_vt = crate::emergency::TtyVtControl {
                libinput: &mut data.libinput,
                device: &mut data.device,
                session: &mut data.session,
            };
            handle_input(
                &mut data.state,
                &mut data.pointer_tracker,
                &data.overlay,
                &data.emergency,
                &data.hardware,
                event,
                Some(&mut tty_vt),
            );
            data.state.request_render_debounced(Duration::from_millis(8));
            request_render(&handle);
        }
    })?;

    handle.insert_source(session_notifier, {
        let handle = handle.clone();
        move |event, _, data| match event {
            SessionEvent::PauseSession => {
                tracing::info!("Sessão pausada (troca de VT)");
                data.libinput.suspend();
                data.device.pause();
            }
            SessionEvent::ActivateSession => {
                tracing::info!("Sessão retomada");
                if data.libinput.resume().is_err() {
                    tracing::error!("libinput resume falhou");
                }
                if let Err(err) = data.device.activate(false) {
                    tracing::error!("drm activate: {}", err);
                }
                data.state.request_render();
                request_render_immediate(&handle);
            }
        }
    })?;

    handle.insert_source(drm_notifier, {
        let handle = handle.clone();
        move |event, _, data| {
            if let DrmEvent::VBlank(_) = event {
                if let Err(err) = data.gbm_surface.frame_submitted() {
                    tracing::warn!("frame_submitted: {}", err);
                }
                data.state.request_render();
                request_render(&handle);
            }
        }
    })?;

    request_render_immediate(&handle);

    event_loop.run(None, &mut data, |_| {})?;

    data.device.pause();
    tracing::info!("Compositor encerrado — VT liberado");

    Ok(())
}

fn ensure_tty_env() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        return Err(
            "XDG_RUNTIME_DIR não está definido.\n\
             Faça login no tty (ex.: tty2) — o pam_systemd cria o diretório automaticamente."
                .into(),
        );
    }
    Ok(())
}

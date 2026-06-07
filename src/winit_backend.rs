use std::time::{Duration, Instant};

use smithay::{
    backend::{
        renderer::{gles::GlesRenderer, ImportDma},
        winit::{self, WinitEvent},
    },
    reexports::wayland_server::Display,
    utils::Transform,
};
use crate::{
    input::{debug_right_click, handle_input, PointerTracker},
    context_menu::ContextMenuControl,
    emergency::EmergencyContext,
    hardware_bridge::HardwareBridge,
    kill_switch,
    overlay::OverlayControl,
    parent_shortcuts::ParentShortcutGuard,
    perf,
    render::{render_kiosk_frame, send_frame_callbacks},
    spawn::{
        bind_wayland_socket, log_bound_socket, prepare_runtime_files, resolve_spawn,
        schedule_spawn,
    },
    state::{accept_clients, accept_clients_rounds, init_state, new_exit_flag, should_exit, State},
};
use crate::{env_detect, i18n::I18n, Args};

const IDLE_SLEEP: Duration = Duration::from_millis(16);

pub fn run(args: Args, i18n: I18n) -> Result<(), Box<dyn std::error::Error>> {
    ensure_desktop_env()?;

    let spawn_plan = resolve_spawn(&args);

    let mut display: Display<State> = Display::new()?;
    let dh = display.handle();

    let (mut backend, mut winit) = winit::init::<GlesRenderer>()?;
    let physical_size = backend.window_size();

    let mod_tracker = crate::modifiers::ModifierTracker::new_arc();
    let mut state = init_state(
        &dh,
        "kioskwm",
        "winit",
        physical_size,
        (480, 270),
        new_exit_flag(),
        i18n,
        mod_tracker.clone(),
    )?;
    state.register_dmabuf_formats(backend.renderer().dmabuf_formats());
    state.output_transform = Transform::Flipped180;

    let mut event_loop = crate::x11::make_event_loop();
    let loop_handle = event_loop.handle();
    crate::x11::start(&loop_handle, &dh);

    let listener = bind_wayland_socket()?;
    let socket_name = listener
        .socket_name()
        .expect("socket wayland sem nome")
        .to_string_lossy()
        .into_owned();

    tracing::info!(
        "Sessão pai: WAYLAND_DISPLAY={:?} DISPLAY={:?}",
        std::env::var_os("WAYLAND_DISPLAY"),
        std::env::var_os("DISPLAY")
    );
    log_bound_socket(&socket_name);
    prepare_runtime_files(&socket_name);

    let no_auto_spawn = spawn_plan.command.is_none();
    schedule_spawn(spawn_plan, socket_name.clone(), args.spawn_delay_ms);
    if args.no_spawn || no_auto_spawn {
        tracing::info!(
            "Modo no-spawn — conecte clientes em WAYLAND_DISPLAY={}",
            socket_name
        );
    }

    tracing::info!("Janela do compositor aberta — feche-a para sair");
    crate::parent_shortcuts::log_workaround();

    let overlay = OverlayControl::new();
    let menu = ContextMenuControl::new();
    let hardware = HardwareBridge::new_arc();
    let emergency = std::sync::Arc::new(EmergencyContext::new(
        state.exit_requested.clone(),
        overlay.clone(),
        menu,
    ));
    kill_switch::spawn(emergency.clone(), mod_tracker, hardware.clone());

    let mut shortcut_guard = ParentShortcutGuard::try_new(backend.window());
    if env_detect::parent_steals_global_shortcuts() {
        backend.window().focus_window();
    }

    let start_time = Instant::now();
    let mut pointer_tracker = PointerTracker::new(state.output_size);
    let auto_rclick = std::env::var_os("KIOSKWM_DEBUG_RCLICK").is_some();
    let mut auto_rclick_done = false;
    let mut frame_count: u64 = 0;

    loop {
        let status = winit.dispatch_new_events(|event| match event {
            WinitEvent::Resized { size, .. } => {
                state.update_output_mode(size);
                pointer_tracker.clamp(state.output_size);
            }
            WinitEvent::Focus(focused) => {
                if let Some(guard) = shortcut_guard.as_mut() {
                    guard.on_focus(focused);
                }
            }
            WinitEvent::CloseRequested => {
                tracing::info!("Fechar janela solicitado (X / menu KDE)");
                state.handle_close_request();
            }
            WinitEvent::Input(event) => {
                handle_input(
                    &mut state,
                    &mut pointer_tracker,
                    &overlay,
                    &emergency,
                    &hardware,
                    event,
                    None,
                )
            }
            _ => {}
        });

        overlay.poll(&mut state);
        emergency.menu.poll(&mut state);

        if let Some(guard) = shortcut_guard.as_mut() {
            guard.poll();
        }

        if matches!(status, ::winit::platform::pump_events::PumpStatus::Exit(_))
            || should_exit(&state.exit_requested)
        {
            break;
        }

        accept_clients(&mut display, &mut state, &listener)?;
        if state.needs_x11_dispatch() {
            crate::x11::dispatch(&mut event_loop, &mut state);
        }

        if !state.take_render_pending() {
            perf::record_frame_skipped();
            std::thread::sleep(IDLE_SLEEP);
            continue;
        }

        let size = backend.window_size();
        let time_ms = start_time.elapsed().as_millis() as u32;

        let damage = {
            let (renderer, mut framebuffer) = backend.bind()?;
            render_kiosk_frame(
                renderer,
                &mut framebuffer,
                &mut state,
                size,
                Transform::Flipped180,
                None,
                None,
                time_ms,
            )?
        };

        send_frame_callbacks(&mut state, time_ms);

        let post_rounds = state.wayland_post_frame_rounds();
        accept_clients_rounds(
            &mut display,
            &mut state,
            &listener,
            post_rounds,
        )?;

        frame_count += 1;
        if auto_rclick
            && !auto_rclick_done
            && !state.running_apps.is_empty()
            && frame_count > 120
        {
            debug_right_click(&mut state, &mut pointer_tracker);
            auto_rclick_done = true;
        }

        backend.submit(Some(&damage))?;
    }

    Ok(())
}

fn ensure_desktop_env() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        return Err(
            "XDG_RUNTIME_DIR não está definido.\n\
             Abra o Konsole ou Alacritty numa sessão gráfica e rode o kioskwm de lá."
                .into(),
        );
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        return Err(format!(
            "Nenhum display gráfico (WAYLAND_DISPLAY/DISPLAY) e VT não detectado.\n\
             {}\n\
             Se você está num tty, isso é um bug — reporte o log acima.",
            crate::env_detect::detection_debug()
        )
        .into());
    }
    Ok(())
}

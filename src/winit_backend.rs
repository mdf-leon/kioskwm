use std::time::{Duration, Instant};

use smithay::{
    backend::{
        renderer::{gles::GlesRenderer, ImportDma},
        winit::{self, WinitEvent},
    },
    reexports::wayland_server::Display,
    utils::{Rectangle, Transform},
};
use wayland_server::ListeningSocket;

use crate::{
    input::{debug_right_click, handle_input, PointerTracker},
    emergency::EmergencyContext,
    kill_switch,
    overlay::OverlayControl,
    parent_shortcuts::ParentShortcutGuard,
    render::{render_kiosk_frame, send_frame_callbacks},
    spawn::{command_exists, resolve_terminal, schedule_spawn},
    state::{accept_clients, accept_clients_rounds, init_state, new_exit_flag, should_exit, State},
};
use crate::Args;

pub fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    ensure_desktop_env()?;

    let terminal = resolve_terminal(&args.terminal);

    if !args.no_spawn && !command_exists(&terminal) {
        return Err(format!(
            "Terminal '{terminal}' não encontrado no PATH.\n\
             Instale alacritty/konsole ou passe outro com -t NOME"
        )
        .into());
    }

    let mut display: Display<State> = Display::new()?;
    let dh = display.handle();

    let (mut backend, mut winit) = winit::init::<GlesRenderer>()?;
    let physical_size = backend.window_size();

    let mut state = init_state(
        &dh,
        "kioskwm",
        "winit",
        physical_size,
        (480, 270),
        new_exit_flag(),
    )?;
    state.register_dmabuf_formats(backend.renderer().dmabuf_formats());

    let listener = ListeningSocket::bind_auto("kioskwm", 0..32)?;
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
    tracing::info!("Socket Wayland do kioskwm: {}", socket_name);

    if !args.no_spawn {
        schedule_spawn(terminal, socket_name.clone(), args.spawn_delay_ms);
    } else {
        tracing::info!(
            "Modo manual: WAYLAND_DISPLAY={} {} &",
            socket_name, terminal
        );
    }

    tracing::info!("Janela do compositor aberta — feche-a para sair");
    crate::parent_shortcuts::log_workaround();

    let overlay = OverlayControl::new();
    let emergency = std::sync::Arc::new(EmergencyContext::new(
        state.exit_requested.clone(),
        overlay.clone(),
    ));
    kill_switch::spawn(emergency.clone());

    let mut shortcut_guard = ParentShortcutGuard::try_new(backend.window());

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
            WinitEvent::Input(event) => {
                handle_input(
                    &mut state,
                    &mut pointer_tracker,
                    &overlay,
                    &emergency,
                    event,
                )
            }
            _ => {}
        });

        overlay.poll(&mut state);

        if matches!(status, ::winit::platform::pump_events::PumpStatus::Exit(_))
            || should_exit(&state.exit_requested)
        {
            break;
        }

        let size = backend.window_size();
        let damage = Rectangle::from_size(size);

        {
            accept_clients(&mut display, &mut state, &listener)?;

            let (renderer, mut framebuffer) = backend.bind()?;
            render_kiosk_frame(
                renderer,
                &mut framebuffer,
                &mut state,
                size,
                Transform::Flipped180,
                Some(pointer_tracker.pos),
                None,
            )?;

            send_frame_callbacks(
                &mut state,
                start_time.elapsed().as_millis() as u32,
            );

            // O buffer do menu chega após o frame callback — drena a resposta do cliente.
            let rounds = if state.active_popup.is_some() { 10 } else { 2 };
            accept_clients_rounds(&mut display, &mut state, &listener, rounds)?;
        }

        frame_count += 1;
        if auto_rclick
            && !auto_rclick_done
            && state.primary_toplevel.is_some()
            && frame_count > 120
        {
            debug_right_click(&mut state, &mut pointer_tracker);
            auto_rclick_done = true;
        }

        backend.submit(Some(&[damage]))?;
        std::thread::sleep(Duration::from_millis(1));
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

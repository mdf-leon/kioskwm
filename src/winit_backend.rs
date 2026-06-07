use std::time::{Duration, Instant};

use smithay::{
    backend::{
        renderer::{
            element::{
                surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
                Kind,
            },
            gles::GlesRenderer,
            utils::draw_render_elements,
            Color32F, Frame, Renderer,
        },
        winit::{self, WinitEvent},
    },
    reexports::wayland_server::Display,
    utils::{Rectangle, Transform},
};
use wayland_server::ListeningSocket;

use crate::{
    input::{handle_input, PointerTracker},
    render::send_frames_surface_tree,
    spawn::{command_exists, resolve_terminal, schedule_spawn},
    state::{accept_clients, init_state, new_exit_flag, should_exit, State},
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

    let start_time = Instant::now();
    let mut pointer_tracker = PointerTracker::new(state.output_size);

    loop {
        let status = winit.dispatch_new_events(|event| match event {
            WinitEvent::Resized { size, .. } => {
                state.update_output_mode(size);
                pointer_tracker.clamp(state.output_size);
            }
            WinitEvent::Input(event) => handle_input(&mut state, &mut pointer_tracker, event),
            _ => {}
        });

        if matches!(status, ::winit::platform::pump_events::PumpStatus::Exit(_))
            || should_exit(&state.exit_requested)
        {
            break;
        }

        let size = backend.window_size();
        let damage = Rectangle::from_size(size);

        {
            let (renderer, mut framebuffer) = backend.bind()?;
            let elements = state
                .xdg_shell_state
                .toplevel_surfaces()
                .iter()
                .flat_map(|surface| {
                    render_elements_from_surface_tree(
                        renderer,
                        surface.wl_surface(),
                        (0, 0),
                        1.0,
                        1.0,
                        Kind::Unspecified,
                    )
                })
                .collect::<Vec<WaylandSurfaceRenderElement<GlesRenderer>>>();

            let mut frame = renderer.render(&mut framebuffer, size, Transform::Flipped180)?;
            frame.clear(Color32F::new(0.08, 0.08, 0.08, 1.0), &[damage])?;
            draw_render_elements(&mut frame, 1.0, &elements, &[damage])?;
            let _ = frame.finish()?;

            for surface in state.xdg_shell_state.toplevel_surfaces() {
                send_frames_surface_tree(
                    surface.wl_surface(),
                    start_time.elapsed().as_millis() as u32,
                );
            }

            accept_clients(&mut display, &mut state, &listener)?;
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

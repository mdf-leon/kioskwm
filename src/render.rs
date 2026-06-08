use smithay::{
    backend::renderer::{
        element::{
            surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
            Kind,
        },
        gles::GlesRenderer,
        utils::{
            draw_render_elements, import_surface, import_surface_tree, RendererSurfaceStateUserData,
        },
        Color32F, Frame, Renderer,
    },
    desktop::PopupManager,
    utils::{Logical, Physical, Point, Rectangle, Scale, Transform},
    wayland::compositor::{with_surface_tree_downward, SurfaceAttributes, TraversalAction},
};
use wayland_server::{protocol::wl_surface, Resource};

use crate::{
    apps::ActiveTarget,
    backdrop,
    cursor::{self, PointerCursor},
    perf::{self, FrameTimer},
    state::State,
};

pub fn send_frame_callbacks(state: &mut State, time: u32) {
    if state.uses_wm_backdrop() || state.alt_tab.open {
        state.maintain_popups();
        return;
    }
    match state.active_target() {
        Some(ActiveTarget::Wayland(i)) => {
            if let Some(app) = state.running_apps.get(i) {
                let wl = app.surface.wl_surface();
                send_frames_surface_tree(wl, time);
                for (popup, _) in PopupManager::popups_for_surface(wl) {
                    send_frames_surface_tree(popup.wl_surface(), time);
                }
            }
        }
        Some(ActiveTarget::X11(i)) => {
            if let Some(wl) = state
                .x11_apps
                .get(i)
                .and_then(|a| a.surface.wl_surface())
            {
                send_frames_surface_tree(&wl, time);
            }
            for overlay in &state.x11_overlays {
                if let Some(wl) = overlay.wl_surface() {
                    send_frames_surface_tree(&wl, time);
                }
            }
        }
        None => {}
    }
    state.maintain_popups();
}

pub fn compute_frame_damage(
    state: &State,
    size: smithay::utils::Size<i32, Physical>,
) -> Vec<Rectangle<i32, Physical>> {
    let _ = state;
    vec![Rectangle::from_size(size)]
}

fn render_popup_surface_tree(
    renderer: &mut GlesRenderer,
    surface: &wl_surface::WlSurface,
    location: impl Into<Point<i32, Physical>>,
    scale: Scale<f64>,
) -> Vec<WaylandSurfaceRenderElement<GlesRenderer>> {
    let location = location.into().to_f64();
    let mut elements = Vec::new();

    with_surface_tree_downward(
        surface,
        location,
        |_, states, location| {
            let mut location = *location;
            if let Some(data) = states.data_map.get::<RendererSurfaceStateUserData>() {
                if let Some(view) = data.lock().unwrap().view() {
                    location += view.offset.to_f64().to_physical(scale);
                }
            }
            TraversalAction::DoChildren(location)
        },
        |surface, states, location| {
            let _ = import_surface(renderer, states);
            if let Ok(Some(elem)) = WaylandSurfaceRenderElement::from_surface(
                renderer,
                surface,
                states,
                *location,
                1.0,
                Kind::Unspecified,
            ) {
                elements.push(elem);
            }
        },
        |_, _, _| true,
    );

    elements
}

pub fn send_frames_surface_tree(surface: &wl_surface::WlSurface, time: u32) {
    with_surface_tree_downward(
        surface,
        (),
        |_, _, &()| TraversalAction::DoChildren(()),
        |_surf, states, &()| {
            for callback in states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .frame_callbacks
                .drain(..)
            {
                callback.done(time);
            }
        },
        |_, _, &()| true,
    );
}

pub fn render_kiosk_frame(
    renderer: &mut GlesRenderer,
    target: &mut smithay::backend::renderer::gles::GlesTarget<'_>,
    state: &mut State,
    size: smithay::utils::Size<i32, Physical>,
    transform: Transform,
    pointer: Option<Point<f64, Logical>>,
    cursor: Option<&PointerCursor>,
    time_ms: u32,
) -> Result<Vec<Rectangle<i32, Physical>>, Box<dyn std::error::Error>> {
    let timer = FrameTimer::start();
    state.flush_pointer_to_client();

    let damage = compute_frame_damage(state, size);
    let full_damage = true;
    // TTY/QEMU: sem backdrop GPU (fence virtio trava o loop). Com overlay aberto,
    // não compõe apps por baixo — scrim + painel ficam visíveis sobre fundo escuro.
    let skip_live_apps = state.wm_ui_obscures_apps()
        || state.uses_wm_backdrop()
        || (state.overlay_open && state.draw_compositor_cursor);

    let mut toplevel_elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = Vec::new();
    let mut popup_elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = Vec::new();

    if !skip_live_apps {
        collect_app_elements(state, renderer, &mut toplevel_elements, &mut popup_elements);
    } else if state.uses_wm_backdrop() {
        let mut capture_popups = Vec::new();
        let mut capture_toplevels = Vec::new();
        collect_app_elements(
            state,
            renderer,
            &mut capture_toplevels,
            &mut capture_popups,
        );
        let mut capture_elements = capture_popups;
        capture_elements.extend(capture_toplevels);
        backdrop::capture_if_needed(renderer, state, size, transform, &capture_elements)?;
    }

    let mut elements = popup_elements;
    elements.extend(toplevel_elements);

    let cursor_elem = match (pointer, cursor) {
        (Some(pos), Some(cur)) if state.draw_compositor_cursor => {
            Some(cursor::cursor_element(renderer, cur, pos)?)
        }
        _ => None,
    };

    let scale = state.output.current_scale().fractional_scale();
    let context_menu = if state.context_menu.open {
        crate::context_menu::prepare_menu(renderer, state, size, scale)?
    } else {
        None
    };

    let settings_panel = if state.overlay_open {
        crate::settings::prepare_panel(renderer, state, size, scale)?
    } else {
        None
    };

    let overlay_scrim = if state.overlay_open {
        crate::settings::prepare_scrim(renderer, state, size)?
    } else {
        None
    };

    let backdrop_elem = if state.uses_wm_backdrop() {
        backdrop::prepare_element(renderer, state)?
    } else {
        None
    };

    let alt_tab_overlay = if state.alt_tab.open {
        crate::alt_tab::prepare_overlay(renderer, state, size, scale, time_ms)?
    } else {
        None
    };

    let console_bg = if crate::console_backdrop::wants(state) {
        crate::console_backdrop::prepare_element(renderer, state, size)?
    } else {
        None
    };

    let clear_color = Color32F::new(0.08, 0.08, 0.08, 1.0);

    let mut frame = renderer.render(target, size, transform)?;
    if let Some(elem) = backdrop_elem {
        draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &[elem], &damage)?;
        if let Some(scrim) = overlay_scrim {
            draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &[scrim], &damage)?;
        }
    } else if let Some(bg) = console_bg {
        draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &[bg], &damage)?;
        draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &elements, &damage)?;
        if let Some(scrim) = overlay_scrim {
            draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &[scrim], &damage)?;
        }
    } else {
        frame.clear(clear_color, &damage)?;
        draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &elements, &damage)?;
        if let Some(scrim) = overlay_scrim {
            draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &[scrim], &damage)?;
        }
    }

    if let Some(overlay) = alt_tab_overlay {
        draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &[overlay.elem], &damage)?;
    }

    if let Some(menu) = context_menu {
        draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &[menu.elem], &damage)?;
    }

    if let Some(panel) = settings_panel {
        draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &[panel.elem], &damage)?;
        crate::settings::draw_overlay_extras(&mut frame, state, size, scale);
    }

    if let Some(elem) = cursor_elem {
        draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &[elem], &damage)?;
    }

    let _ = frame.finish()?;
    state.finish_frame_damage_state(pointer);
    perf::record_frame_rendered(timer.elapsed_ms(), damage.len(), full_damage);
    Ok(damage)
}

fn collect_app_elements(
    state: &State,
    renderer: &mut GlesRenderer,
    toplevel_elements: &mut Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
    popup_elements: &mut Vec<WaylandSurfaceRenderElement<GlesRenderer>>,
) {
    let mut rendered_popups = std::collections::HashSet::new();

    match state.active_target() {
        Some(ActiveTarget::Wayland(i)) => {
            if let Some(app) = state.running_apps.get(i) {
                let surface = &app.surface;
                toplevel_elements.extend(render_elements_from_surface_tree(
                    renderer,
                    surface.wl_surface(),
                    (0, 0),
                    1.0,
                    1.0,
                    Kind::Unspecified,
                ));

                for (popup, popup_offset) in PopupManager::popups_for_surface(surface.wl_surface()) {
                    rendered_popups.insert(popup.wl_surface().id());
                    let wl = popup.wl_surface();
                    let (ox, oy) = state.popup_render_offset(&popup, popup_offset);
                    let _ = import_surface_tree(renderer, wl);

                    let mut elems =
                        render_popup_surface_tree(renderer, wl, (ox, oy), Scale::from(1.0));
                    if elems.is_empty() {
                        elems = render_elements_from_surface_tree(
                            renderer,
                            wl,
                            (ox, oy),
                            1.0,
                            1.0,
                            Kind::Unspecified,
                        );
                    }

                    if elems.is_empty() {
                        tracing::trace!("popup aguardando buffer em ({ox}, {oy})");
                    }
                    popup_elements.extend(elems);
                }
            }
        }
        Some(ActiveTarget::X11(i)) => {
            if let Some(app) = state.x11_apps.get(i) {
                if let Some(wl) = app.surface.wl_surface() {
                    let _ = import_surface_tree(renderer, &wl);
                    toplevel_elements.extend(render_elements_from_surface_tree(
                        renderer,
                        &wl,
                        (0, 0),
                        1.0,
                        1.0,
                        Kind::Unspecified,
                    ));
                }
            }
            for overlay in &state.x11_overlays {
                if let Some(wl) = overlay.wl_surface() {
                    let geo = overlay.geometry();
                    let _ = import_surface_tree(renderer, &wl);
                    popup_elements.extend(render_elements_from_surface_tree(
                        renderer,
                        &wl,
                        (geo.loc.x, geo.loc.y),
                        1.0,
                        1.0,
                        Kind::Unspecified,
                    ));
                }
            }
        }
        None => {}
    }

    for popup in state.xdg_shell_state.popup_surfaces() {
        let wl = popup.wl_surface();
        if rendered_popups.contains(&wl.id()) {
            continue;
        }
        let Some((ox, oy)) = state.popup_render_offset_for(wl) else {
            continue;
        };
        let _ = import_surface_tree(renderer, wl);
        let mut elems = render_popup_surface_tree(renderer, wl, (ox, oy), Scale::from(1.0));
        if elems.is_empty() {
            elems = render_elements_from_surface_tree(renderer, wl, (ox, oy), 1.0, 1.0, Kind::Unspecified);
        }
        popup_elements.extend(elems);
    }
}

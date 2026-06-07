use smithay::{
    backend::renderer::{
        element::{
            surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
            Kind,
        },
        gles::GlesRenderer,
        utils::draw_render_elements,
        Color32F, Frame, Renderer,
    },
    utils::{Logical, Point, Rectangle, Transform},
    wayland::compositor::{
        with_surface_tree_downward, SurfaceAttributes, TraversalAction,
    },
};
use wayland_server::protocol::wl_surface;

use crate::{cursor::PointerCursor, state::State};

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
    state: &State,
    size: smithay::utils::Size<i32, smithay::utils::Physical>,
    transform: Transform,
    pointer: Option<Point<f64, Logical>>,
    cursor: Option<&PointerCursor>,
) -> Result<(), Box<dyn std::error::Error>> {
    let damage = Rectangle::from_size(size);

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

    let cursor_elem = match (pointer, cursor) {
        (Some(pos), Some(cursor)) => Some(crate::cursor::cursor_element(renderer, cursor, pos)?),
        _ => None,
    };

    let mut frame = renderer.render(target, size, transform)?;
    frame.clear(Color32F::new(0.08, 0.08, 0.08, 1.0), &[damage])?;
    draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &elements, &[damage])?;

    if let Some(elem) = cursor_elem {
        draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, &[elem], &[damage])?;
    }

    let _ = frame.finish()?;
    Ok(())
}

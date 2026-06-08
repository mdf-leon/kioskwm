use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                surface::WaylandSurfaceRenderElement,
                Kind,
            },
            gles::{GlesRenderbuffer, GlesRenderer},
            utils::draw_render_elements,
            Bind, Color32F, ExportMem, Frame, Offscreen, Renderer,
        },
    },
    utils::{Buffer, Physical, Rectangle, Size, Transform},
};

use crate::state::State;

pub fn capture_if_needed(
    renderer: &mut GlesRenderer,
    state: &mut State,
    size: Size<i32, Physical>,
    transform: Transform,
    elements: &[WaylandSurfaceRenderElement<GlesRenderer>],
) -> Result<(), Box<dyn std::error::Error>> {
    if !state.uses_wm_backdrop() {
        return Ok(());
    }

    let needs_capture = {
        let guard = state.wm_backdrop.lock().unwrap();
        state.wm_backdrop_stale
            || guard
                .as_ref()
                .is_none_or(|cache| cache.size.0 != size.w || cache.size.1 != size.h)
    };

    if !needs_capture {
        return Ok(());
    }

    let buffer_size = Size::<i32, Buffer>::from((size.w, size.h));
    let mut rb =
        Offscreen::<GlesRenderbuffer>::create_buffer(renderer, Fourcc::Argb8888, buffer_size)?;
    let mut target = renderer.bind(&mut rb)?;
    let damage = vec![Rectangle::from_size(size)];
    let mut frame = renderer.render(&mut target, size, transform)?;
    frame.clear(Color32F::new(0.08, 0.08, 0.08, 1.0), &damage)?;
    draw_render_elements::<GlesRenderer, _, _>(&mut frame, 1.0, elements, &damage)?;
    frame.finish()?;
    // Sem .wait() — em virtio/QEMU o fence pode bloquear o loop inteiro (mouse/CAD congelam).

    let mapping = renderer.copy_framebuffer(
        &target,
        Rectangle::from_size(buffer_size),
        Fourcc::Argb8888,
    )?;
    let pixels = renderer.map_texture(&mapping)?;
    let mem = MemoryRenderBuffer::from_slice(
        pixels,
        Fourcc::Argb8888,
        (size.w, size.h),
        1,
        transform,
        None,
    );

    *state.wm_backdrop.lock().unwrap() = Some(crate::state::WmBackdropCache {
        buffer: mem,
        size: (size.w, size.h),
    });
    state.wm_backdrop_stale = false;
    Ok(())
}

pub fn prepare_element(
    renderer: &mut GlesRenderer,
    state: &State,
) -> Result<Option<MemoryRenderBufferRenderElement<GlesRenderer>>, Box<dyn std::error::Error>> {
    let guard = state.wm_backdrop.lock().unwrap();
    let Some(cache) = guard.as_ref() else {
        return Ok(None);
    };

    let elem = MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        (0.0, 0.0),
        &cache.buffer,
        None,
        None,
        None,
        Kind::Unspecified,
    )?;

    Ok(Some(elem))
}

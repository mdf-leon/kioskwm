//! Idle screen on TTY when no Wayland/X11 client is open.

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                Kind,
            },
            gles::GlesRenderer,
        },
    },
    utils::{Rectangle, Size, Transform},
};

use crate::{
    env_detect,
    settings::{raster::Canvas, text, theme::Rgba},
    spawn,
    state::State,
};

const BG: Rgba = Rgba::new(0, 0, 0, 255);
const TITLE: Rgba = Rgba::new(204, 204, 204, 255);
const HINT: Rgba = Rgba::new(140, 140, 140, 255);

pub fn wants(state: &State) -> bool {
    state.console_backdrop_enabled
        && state.app_count() == 0
        && !state.wm_ui_obscures_apps()
}

pub fn prepare_element(
    renderer: &mut GlesRenderer,
    state: &mut State,
    output: Size<i32, smithay::utils::Physical>,
) -> Result<Option<MemoryRenderBufferRenderElement<GlesRenderer>>, Box<dyn std::error::Error>> {
    if !wants(state) {
        return Ok(None);
    }

    let key = (output.w, output.h);
    {
        let guard = state.console_backdrop_cache.lock().unwrap();
        if guard.as_ref().is_some_and(|c| c.size == key) {
            let buf = &guard.as_ref().unwrap().buffer;
            let elem = MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                (0.0, 0.0),
                buf,
                None,
                None,
                None,
                Kind::Unspecified,
            )?;
            return Ok(Some(elem));
        }
    }

    let font_size = (output.h as f32 * 0.022).clamp(13.0, 20.0);
    let title_size = (font_size * 1.15).min(22.0);
    let margin = (font_size * 0.85) as i32;
    let line_h = (font_size * 1.35) as i32;
    let char_w = text::width("M", font_size, false).max(1);
    let cols = ((output.w - margin * 2) / char_w).max(1) as usize;

    let mut canvas = Canvas::new(output.w, output.h);
    canvas.fill(BG);

    let mut y = margin + line_h;
    for line in idle_lines(cols) {
        let (size, color) = if y == margin + line_h {
            (title_size, TITLE)
        } else {
            (font_size, HINT)
        };
        text::draw(&mut canvas, margin, y, size, &line, color);
        y += line_h;
        if y + line_h >= output.h {
            break;
        }
    }

    let mut buffer = MemoryRenderBuffer::new(
        Fourcc::Argb8888,
        (output.w, output.h),
        1,
        Transform::Normal,
        None,
    );
    {
        let mut ctx = buffer.render();
        let _ = ctx.draw(|buf| {
            buf.copy_from_slice(&canvas.pixels);
            Ok::<_, std::convert::Infallible>(vec![Rectangle::from_size(Size::from((
                output.w,
                output.h,
            )))])
        });
    }

    let elem = MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        (0.0, 0.0),
        &buffer,
        None,
        None,
        None,
        Kind::Unspecified,
    )?;

    *state.console_backdrop_cache.lock().unwrap() =
        Some(crate::state::ConsoleBackdropCache { buffer, size: key });

    Ok(Some(elem))
}

fn idle_lines(cols: usize) -> Vec<String> {
    let host = read_hostname();
    let tty = env_detect::controlling_tty().unwrap_or_else(|| "tty1".into());

    let mut lines = vec![
        format!("kioskwm on {host} ({tty})"),
        String::new(),
        "No application is open.".into(),
        String::new(),
        "Ctrl+Alt+F1-F12 switches VT (real shell on another TTY).".into(),
    ];

    if spawn::detect_terminal().is_none() {
        lines.push(String::new());
        lines.push("Install a Wayland terminal for auto-start, e.g.:".into());
        lines.push("sudo apt install foot".into());
    }

    lines
        .into_iter()
        .flat_map(|line| wrap_line(&line, cols))
        .collect()
}

fn wrap_line(line: &str, cols: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }
    if line.chars().count() <= cols {
        return vec![line.to_string()];
    }
    let mut out = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        if rest.chars().count() <= cols {
            out.push(rest.to_string());
            break;
        }
        let split = rest
            .char_indices()
            .nth(cols)
            .map(|(i, _)| rest[..i].rfind(' ').unwrap_or(i))
            .unwrap_or(rest.len());
        let split = if split == 0 {
            rest.char_indices().nth(cols).map(|(i, _)| i).unwrap_or(rest.len())
        } else {
            split
        };
        out.push(rest[..split].trim_end().to_string());
        rest = rest[split..].trim_start();
    }
    out
}

fn read_hostname() -> String {
    if let Ok(s) = std::fs::read_to_string("/etc/hostname") {
        let s = s.trim();
        if !s.is_empty() {
            return s.to_string();
        }
    }
    if let Ok(o) = std::process::Command::new("hostname").output() {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    "localhost".into()
}

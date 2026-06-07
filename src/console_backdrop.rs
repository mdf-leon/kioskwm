//! Fundo estilo console Linux quando nenhuma app Wayland/X11 está aberta (modo TTY).

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
    font8x8,
    settings::{raster::Canvas, theme::Rgba},
    state::State,
};

const BG: Rgba = Rgba::new(0, 0, 0, 255);
const TEXT: Rgba = Rgba::new(204, 204, 204, 255);
const PROMPT: Rgba = Rgba::new(0, 204, 0, 255);
const CURSOR: Rgba = Rgba::new(204, 204, 204, 255);

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

    let scale = (output.h / 400).clamp(2, 3);
    let char_w = 8 * scale;
    let char_h = 8 * scale;
    let cols = (output.w / char_w).max(1) as usize;

    let mut canvas = Canvas::new(output.w, output.h);
    canvas.fill(BG);

    let mut y = char_h;
    let lines = console_lines(cols);
    for line in &lines {
        let color = if line.ends_with("$ ") {
            PROMPT
        } else {
            TEXT
        };
        draw_text8(&mut canvas, char_h, y, scale, line, color);
        y += char_h;
        if y + char_h >= output.h {
            break;
        }
    }

    if let Some(prompt) = lines.last() {
        let cursor_x = char_h + prompt.len() as i32 * char_w;
        let cursor_y = y - char_h;
        if cursor_y >= 0 {
            canvas.fill_rect(cursor_x, cursor_y + char_h - scale, char_w, scale, CURSOR);
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

fn draw_text8(c: &mut Canvas, mut x: i32, y: i32, scale: i32, text: &str, color: Rgba) {
    for ch in text.chars() {
        draw_glyph8(c, x, y, scale, ch, color);
        x += 8 * scale;
    }
}

fn draw_glyph8(c: &mut Canvas, x: i32, y: i32, scale: i32, ch: char, color: Rgba) {
    let glyph = font8x8::glyph(ch);
    for (row, &bits) in glyph.iter().enumerate() {
        for col in 0..8 {
            if bits & (1 << col) != 0 {
                c.fill_rect(x + col * scale, y + row as i32 * scale, scale, scale, color);
            }
        }
    }
}

fn console_lines(cols: usize) -> Vec<String> {
    let host = read_hostname();
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "kiosk".into());
    let tty = env_detect::controlling_tty().unwrap_or_else(|| "tty1".into());

    let mut lines = vec![format!("{host} {tty}"), String::new()];

    for path in ["/run/motd.dynamic", "/etc/motd"] {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                let t = line.trim_end();
                if t.is_empty() {
                    lines.push(String::new());
                    continue;
                }
                for wrapped in wrap_line(t, cols) {
                    lines.push(wrapped);
                }
            }
        }
    }

    if lines.last().is_some_and(|l| !l.is_empty()) {
        lines.push(String::new());
    }
    lines.push(format!("{user}@{host}:~$ "));
    lines
}

fn wrap_line(line: &str, cols: usize) -> Vec<String> {
    if line.len() <= cols {
        return vec![line.to_string()];
    }
    let mut out = Vec::new();
    let mut rest = line;
    while !rest.is_empty() {
        if rest.len() <= cols {
            out.push(rest.to_string());
            break;
        }
        let split = rest[..cols].rfind(' ').unwrap_or(cols);
        let split = if split == 0 { cols } else { split };
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

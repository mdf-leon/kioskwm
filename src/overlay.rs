//! Painel Ctrl+Alt+Del (max 500x500). Rasterizado em um unico buffer GPU.

use std::{
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                Kind,
            },
            gles::{GlesFrame, GlesRenderer},
            Color32F,
        },
    },
    utils::{Logical, Point, Rectangle, Size, Transform},
};

use crate::{
    env_detect,
    font8x8::glyph,
    state::State,
};

pub const PANEL_W: i32 = 480;
pub const PANEL_H: i32 = 400;
const CHAR_W: i32 = 8;
const LINE_H: i32 = 11;

pub struct OverlayControl {
    toggle_requested: AtomicBool,
}

impl OverlayControl {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            toggle_requested: AtomicBool::new(false),
        })
    }

    pub fn request_toggle(&self) {
        self.toggle_requested.store(true, Ordering::SeqCst);
        self.notify_changed();
    }

    pub fn notify_changed(&self) {
        unsafe {
            libc::raise(libc::SIGUSR2);
        }
    }

    pub fn poll(&self, state: &mut State) {
        if self.toggle_requested.swap(false, Ordering::SeqCst) {
            state.overlay_open = !state.overlay_open;
            tracing::info!(
                "Painel P1 {}",
                if state.overlay_open { "aberto" } else { "fechado" }
            );
            if state.overlay_open {
                crate::emergency::seize_for_overlay(state);
                invalidate_panel_cache(state);
            }
        }
    }
}

#[derive(Clone)]
pub struct DiscoveredTool {
    pub label: String,
    pub command: String,
    pub args: Vec<String>,
    pub note: String,
}

pub struct OverlayDraw {
    pub elem: MemoryRenderBufferRenderElement<GlesRenderer>,
    pub panel_damage: Rectangle<i32, smithay::utils::Physical>,
}

pub fn prepare_overlay(
    renderer: &mut GlesRenderer,
    state: &mut State,
    output: Size<i32, smithay::utils::Physical>,
) -> Result<Option<OverlayDraw>, Box<dyn std::error::Error>> {
    if !state.overlay_open {
        return Ok(None);
    }

    let scale = state.output.current_scale().fractional_scale();
    let out_w = (output.w as f64 / scale).round() as i32;
    let out_h = (output.h as f64 / scale).round() as i32;
    let panel_x = (out_w - PANEL_W) / 2;
    let panel_y = (out_h - PANEL_H) / 2;

    rebuild_panel_if_needed(state)?;
    let cache = state.overlay_panel.lock().unwrap();
    let Some(cache) = cache.as_ref() else {
        return Ok(None);
    };

    let loc = Point::<i32, Logical>::from((panel_x, panel_y)).to_physical_precise_round(scale);
    let elem = MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        loc,
        &cache.buffer,
        None,
        None,
        None,
        Kind::Unspecified,
    )?;
    let panel_damage = Rectangle::from_size(Size::from((
        (PANEL_W as f64 * scale).round() as i32,
        (PANEL_H as f64 * scale).round() as i32,
    )));

    Ok(Some(OverlayDraw {
        elem,
        panel_damage,
    }))
}

pub fn draw_dim(frame: &mut GlesFrame<'_, '_>, output: Size<i32, smithay::utils::Physical>, scale: f64) {
    let out_w = (output.w as f64 / scale).round() as i32;
    let out_h = (output.h as f64 / scale).round() as i32;
    let dim = Rectangle::from_size(Size::from((out_w, out_h))).to_physical_precise_round(scale);
    use smithay::backend::renderer::Frame as _;
    let _ = frame.draw_solid(dim, &[dim], Color32F::new(0.0, 0.0, 0.0, 0.2));
}

pub fn discover_tools() -> Vec<DiscoveredTool> {
    let on_tty = env_detect::on_hardware_tty();
    let kde = env_detect::is_kde_session();

    let candidates: &[(&str, &str, &[&str], &str)] = &[
        ("KDE System Settings", "systemsettings6", &[], "Nao controla kioskwm no TTY"),
        ("KDE System Settings (5)", "systemsettings5", &[], "Nao controla kioskwm no TTY"),
        ("KDE modulo mouse", "kcmshell6", &["mouse"], "Mouse so no Plasma"),
        ("libinput-gui", "libinput-gui", &[], "Config libinput global"),
    ];

    let mut out = Vec::new();
    for (label, bin, args, note) in candidates {
        if !command_exists(bin) {
            continue;
        }
        let mut note = (*note).to_string();
        if on_tty && label.starts_with("KDE") {
            note.push_str(" [TTY: use este painel]");
        } else if kde && label.starts_with("KDE") {
            note.push_str(" [KDE detectado]");
        }
        out.push(DiscoveredTool {
            label: (*label).to_string(),
            command: (*bin).to_string(),
            args: args.iter().map(|s| (*s).to_string()).collect(),
            note,
        });
    }
    out
}

pub fn launch_tool(tool: &DiscoveredTool) -> bool {
    tracing::info!("Abrindo ferramenta: {} {:?}", tool.label, tool.args);
    Command::new(&tool.command)
        .args(&tool.args)
        .spawn()
        .is_ok()
}

fn command_exists(name: &str) -> bool {
    crate::spawn::command_exists(name)
}

pub fn invalidate_panel_cache(state: &mut State) {
    *state.overlay_panel.lock().unwrap() = None;
}

fn rebuild_panel_if_needed(state: &mut State) -> Result<(), Box<dyn std::error::Error>> {
    let speed_bits = state.pointer_speed.to_bits();
    let needs = state
        .overlay_panel
        .lock()
        .unwrap()
        .as_ref()
        .map(|c| c.speed_bits != speed_bits)
        .unwrap_or(true);
    if !needs {
        return Ok(());
    }

    let tools = discover_tools();
    let lines = build_lines(state, &tools);
    let pixels = rasterize_panel(&lines, state.pointer_speed);

    let mut buffer =
        MemoryRenderBuffer::new(Fourcc::Argb8888, (PANEL_W, PANEL_H), 1, Transform::Normal, None);
    {
        let mut ctx = buffer.render();
        let _ = ctx.draw(|buf| {
            buf.copy_from_slice(&pixels);
            Ok::<_, std::convert::Infallible>(vec![Rectangle::from_size(Size::from((
                PANEL_W, PANEL_H,
            )))])
        });
    }

    *state.overlay_panel.lock().unwrap() = Some(crate::state::OverlayPanelCache {
        buffer,
        speed_bits,
    });
    Ok(())
}

fn rasterize_panel(lines: &[String], speed: f64) -> Vec<u8> {
    let w = PANEL_W as usize;
    let h = PANEL_H as usize;
    let mut px = vec![0u8; w * h * 4];

    for y in 0..h {
        for x in 0..w {
            put_pixel(&mut px, w, x, y, 38, 44, 58, 255);
        }
    }
    for y in 0..3 {
        for x in 0..w {
            put_pixel(&mut px, w, x, y, 90, 140, 240, 255);
        }
    }

    let mut y = 14i32;
    for line in lines {
        if line.starts_with("@@slider") {
            draw_slider_rgba(&mut px, w, 16, y + 6, PANEL_W - 32, speed);
            y += 28;
            continue;
        }
        draw_text_rgba(&mut px, w, 16, y, line, 235, 240, 250);
        y += LINE_H;
        if y > PANEL_H - 20 {
            break;
        }
    }

    px
}

fn put_pixel(buf: &mut [u8], stride: usize, x: usize, y: usize, r: u8, g: u8, b: u8, a: u8) {
    if x >= stride || y >= PANEL_H as usize {
        return;
    }
    let i = (y * stride + x) * 4;
    if i + 3 < buf.len() {
        buf[i] = b;
        buf[i + 1] = g;
        buf[i + 2] = r;
        buf[i + 3] = a;
    }
}

fn draw_text_rgba(buf: &mut [u8], w: usize, x0: i32, y0: i32, text: &str, r: u8, g: u8, b: u8) {
    let mut x = x0;
    for ch in text.chars() {
        let bitmap = glyph(ch);
        for (row, bits) in bitmap.iter().enumerate() {
            for col in 0..8 {
                if bits & (1 << col) != 0 {
                    put_pixel(
                        buf,
                        w,
                        (x + col) as usize,
                        (y0 + row as i32) as usize,
                        r,
                        g,
                        b,
                        255,
                    );
                }
            }
        }
        x += CHAR_W;
    }
}

fn draw_slider_rgba(buf: &mut [u8], w: usize, x: i32, y: i32, bar_w: i32, speed: f64) {
    let bar_h = 12;
    for dy in 0..bar_h {
        for dx in 0..bar_w {
            put_pixel(buf, w, (x + dx) as usize, (y + dy) as usize, 20, 22, 30, 255);
        }
    }
    let t = ((speed - 0.25) / (4.0 - 0.25)).clamp(0.0, 1.0);
    let fill_w = ((bar_w as f64) * t).round() as i32;
    for dy in 0..bar_h {
        for dx in 0..fill_w {
            put_pixel(buf, w, (x + dx) as usize, (y + dy) as usize, 90, 140, 240, 255);
        }
    }
}

fn build_lines(state: &State, tools: &[DiscoveredTool]) -> Vec<String> {
    let mut lines = vec![
        "kioskwm — painel (Ctrl+Alt+Del)".to_string(),
        String::new(),
        "KDE Settings NAO altera este WM no TTY.".to_string(),
        "Velocidade do cursor:".to_string(),
        "@@slider".to_string(),
        format!("{:.2}x  (+/- ou setas)", state.pointer_speed),
        String::new(),
        "Ferramentas no PATH:".to_string(),
    ];

    if tools.is_empty() {
        lines.push("  (nenhuma)".to_string());
    } else {
        for (i, t) in tools.iter().take(3).enumerate() {
            lines.push(format!("{}. {} — {}", i + 1, t.label, t.note));
        }
    }

    lines.push(String::new());
    lines.push("[O] abrir  [Esc] fechar".to_string());
    lines.push("Ctrl+Alt+Shift+Del = sair".to_string());
    lines
}

pub fn adjust_speed(state: &mut State, delta: f64) {
    state.pointer_speed = (state.pointer_speed + delta).clamp(0.25, 4.0);
    invalidate_panel_cache(state);
    tracing::info!("Velocidade do cursor: {:.2}x", state.pointer_speed);
}

pub fn first_tool() -> Option<DiscoveredTool> {
    discover_tools().into_iter().next()
}

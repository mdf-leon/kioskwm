use super::{
    slider::speed_from_slider_x,
    theme::{self, PANEL_H, PANEL_W},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Main,
    Mouse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmAction {
    QuitWm,
    Shutdown,
    Reboot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    None,
    AppletMouse,
    FooterQuit,
    FooterShutdown,
    FooterReboot,
    MouseBack,
    Slider,
    ConfirmCancel,
    ConfirmOk,
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

/// Botao estilo KDE Quick Settings (linha com icone + texto).
pub const APPLET_MOUSE: Rect = Rect {
    x: 24,
    y: 128,
    w: 452,
    h: 48,
};

pub fn footer_buttons() -> [Rect; 3] {
    let y = PANEL_H - theme::FOOTER_H + 12;
    let h = 36;
    let gap = 8;
    let w = (PANEL_W - 48 - gap * 2) / 3;
    [
        Rect { x: 24, y, w, h },
        Rect {
            x: 24 + w + gap,
            y,
            w,
            h,
        },
        Rect {
            x: 24 + (w + gap) * 2,
            y,
            w,
            h,
        },
    ]
}

pub const MOUSE_BACK: Rect = Rect {
    x: 16,
    y: 62,
    w: 108,
    h: 32,
};

pub fn mouse_slider() -> Rect {
    Rect {
        x: 48,
        y: 220,
        w: PANEL_W - 96,
        h: 6,
    }
}

pub fn confirm_modal() -> Rect {
    Rect {
        x: 60,
        y: 150,
        w: 380,
        h: 200,
    }
}

pub fn confirm_buttons(modal: Rect) -> (Rect, Rect) {
    let bw = 140;
    let bh = 36;
    let y = modal.y + modal.h - bh - 20;
    (
        Rect {
            x: modal.x + 20,
            y,
            w: bw,
            h: bh,
        },
        Rect {
            x: modal.x + modal.w - bw - 20,
            y,
            w: bw,
            h: bh,
        },
    )
}

pub fn hit_test(screen: Screen, confirm: Option<ConfirmAction>, x: i32, y: i32) -> Hit {
    if confirm.is_some() {
        let modal = confirm_modal();
        let (cancel, ok) = confirm_buttons(modal);
        if cancel.contains(x, y) {
            return Hit::ConfirmCancel;
        }
        if ok.contains(x, y) {
            return Hit::ConfirmOk;
        }
        return Hit::None;
    }

    match screen {
        Screen::Main => {
            if APPLET_MOUSE.contains(x, y) {
                return Hit::AppletMouse;
            }
            let footers = footer_buttons();
            if footers[0].contains(x, y) {
                return Hit::FooterQuit;
            }
            if footers[1].contains(x, y) {
                return Hit::FooterShutdown;
            }
            if footers[2].contains(x, y) {
                return Hit::FooterReboot;
            }
        }
        Screen::Mouse => {
            if MOUSE_BACK.contains(x, y) {
                return Hit::MouseBack;
            }
            let track = mouse_slider();
            let hit = Rect {
                x: track.x - 12,
                y: track.y - 24,
                w: track.w + 24,
                h: track.h + 48,
            };
            if hit.contains(x, y) {
                return Hit::Slider;
            }
        }
    }
    Hit::None
}

pub fn slider_value_from_x(x: i32) -> f64 {
    let r = mouse_slider();
    speed_from_slider_x(x, r.x, r.w)
}

pub fn panel_origin_logical(output_w: i32, output_h: i32) -> (i32, i32) {
    ((output_w - PANEL_W) / 2, (output_h - PANEL_H) / 2)
}

pub fn pointer_to_panel_local(
    pos_x: f64,
    pos_y: f64,
    output_w: i32,
    output_h: i32,
) -> Option<(i32, i32)> {
    let (ox, oy) = panel_origin_logical(output_w, output_h);
    let lx = pos_x - ox as f64;
    let ly = pos_y - oy as f64;
    if lx >= 0.0 && ly >= 0.0 && lx < PANEL_W as f64 && ly < PANEL_H as f64 {
        Some((lx as i32, ly as i32))
    } else {
        None
    }
}

pub const SLIDER_LABEL_LEFT: &str = "0.01x";
pub const SLIDER_LABEL_CENTER: &str = "1x";
pub const SLIDER_LABEL_RIGHT: &str = "4x";

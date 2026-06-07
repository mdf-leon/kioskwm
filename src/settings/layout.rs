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
    Close,
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

/// Linha estilo KDE Quick Settings — icone Breeze + texto.
pub const APPLET_MOUSE: Rect = Rect {
    x: 20,
    y: 78,
    w: 200,
    h: 36,
};

pub fn footer_buttons() -> [Rect; 3] {
    let y = PANEL_H - theme::FOOTER_H + 8;
    let h = 32;
    let gap = 10;
    let w = (PANEL_W - 40 - gap * 2) / 3;
    [
        Rect { x: 20, y, w, h },
        Rect {
            x: 20 + w + gap,
            y,
            w,
            h,
        },
        Rect {
            x: 20 + (w + gap) * 2,
            y,
            w,
            h,
        },
    ]
}

pub const HEADER_CLOSE: Rect = Rect {
    x: PANEL_W - 40,
    y: 8,
    w: 32,
    h: 32,
};

pub const MOUSE_BACK: Rect = Rect {
    x: 12,
    y: 8,
    w: 96,
    h: 32,
};

pub fn mouse_slider() -> Rect {
    Rect {
        x: 32,
        y: 188,
        w: PANEL_W - 64,
        h: 4,
    }
}

pub fn confirm_modal() -> Rect {
    Rect {
        x: 50,
        y: 140,
        w: 340,
        h: 180,
    }
}

pub fn confirm_buttons(modal: Rect) -> (Rect, Rect) {
    let bw = 128;
    let bh = 32;
    let y = modal.y + modal.h - bh - 18;
    (
        Rect {
            x: modal.x + 18,
            y,
            w: bw,
            h: bh,
        },
        Rect {
            x: modal.x + modal.w - bw - 18,
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

    if HEADER_CLOSE.contains(x, y) {
        return Hit::Close;
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
                y: track.y - 28,
                w: track.w + 24,
                h: track.h + 56,
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

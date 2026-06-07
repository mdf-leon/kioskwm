//! Cores do Breeze Dark (Kubuntu) — /usr/share/color-schemes/BreezeDark.colors

#[derive(Clone, Copy)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

// [Colors:Window]
pub const WINDOW_BG: Rgba = Rgba::new(32, 35, 38, 255);
// [Colors:Button]
pub const BUTTON_BG: Rgba = Rgba::new(41, 44, 48, 255);
pub const BUTTON_HOVER: Rgba = Rgba::new(61, 65, 70, 255);
// [Colors:View]
pub const VIEW_BG: Rgba = Rgba::new(20, 22, 24, 255);
// [Colors:Header]
pub const HEADER_BG: Rgba = Rgba::new(41, 44, 48, 255);
// Texto
pub const TEXT: Rgba = Rgba::new(252, 252, 252, 255);
pub const TEXT_INACTIVE: Rgba = Rgba::new(161, 169, 177, 255);
// Accent / DecorationFocus
pub const ACCENT: Rgba = Rgba::new(61, 174, 233, 255);
pub const ACCENT_HOVER: Rgba = Rgba::new(41, 128, 185, 255);
// Negativo
pub const NEGATIVE: Rgba = Rgba::new(218, 68, 83, 255);
// Bordas (derivado do tema)
pub const BORDER: Rgba = Rgba::new(87, 91, 96, 255);
pub const BORDER_FOCUS: Rgba = Rgba::new(61, 174, 233, 255);
// Slider
pub const SLIDER_TRACK: Rgba = Rgba::new(49, 54, 59, 255);
pub const SLIDER_TICK: Rgba = Rgba::new(87, 91, 96, 255);
// Modal
pub const MODAL_SCRIM: Rgba = Rgba::new(0, 0, 0, 140);
pub const MODAL_BG: Rgba = Rgba::new(41, 44, 48, 255);

pub const HEADER_H: i32 = 56;
pub const FOOTER_H: i32 = 60;
pub const MOUSE_FOOTER_H: i32 = 76;
pub const PANEL_W: i32 = 500;
pub const PANEL_H: i32 = 500;
pub const PANEL_RADIUS: i32 = 8;

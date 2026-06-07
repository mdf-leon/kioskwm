//! Cores Breeze Dark — alinhado ao Quick Settings do Plasma.

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

/// Fundo uniforme do painel (igual janela KDE).
pub const WINDOW_BG: Rgba = Rgba::new(41, 44, 48, 255);
pub const TILE_BG: Rgba = Rgba::new(49, 54, 59, 255);
pub const TILE_ICON_BG: Rgba = Rgba::new(61, 174, 233, 55);
pub const BUTTON_BG: Rgba = Rgba::new(49, 54, 59, 255);
pub const BUTTON_HOVER: Rgba = Rgba::new(61, 65, 70, 255);
pub const TILE_HOVER: Rgba = Rgba::new(61, 65, 70, 255);
pub const CLOSE_HOVER: Rgba = Rgba::new(61, 65, 70, 255);
pub const NEGATIVE_HOVER: Rgba = Rgba::new(140, 48, 58, 255);
pub const TEXT: Rgba = Rgba::new(252, 252, 252, 255);
pub const TEXT_INACTIVE: Rgba = Rgba::new(161, 169, 177, 255);
pub const ACCENT: Rgba = Rgba::new(61, 174, 233, 255);
pub const NEGATIVE: Rgba = Rgba::new(218, 68, 83, 255);
pub const BORDER: Rgba = Rgba::new(70, 74, 79, 255);
pub const SLIDER_TRACK: Rgba = Rgba::new(61, 65, 70, 255);
pub const SLIDER_TICK: Rgba = Rgba::new(87, 91, 96, 255);
pub const MODAL_SCRIM: Rgba = Rgba::new(0, 0, 0, 120);
pub const MODAL_BG: Rgba = Rgba::new(49, 54, 59, 255);
pub const KNOB_FILL: Rgba = Rgba::new(239, 240, 241, 255);

pub const HEADER_H: i32 = 48;
pub const FOOTER_H: i32 = 52;
pub const MOUSE_FOOTER_H: i32 = 48;
pub const PANEL_W: i32 = 440;
pub const PANEL_H: i32 = 480;
pub const PANEL_RADIUS: i32 = 8;

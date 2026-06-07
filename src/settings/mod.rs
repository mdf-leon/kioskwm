mod actions;
mod icon;
mod layout;
mod raster;
mod render;
mod slider;
mod text;
pub mod theme;

pub mod input;

pub use layout::{ConfirmAction, Screen};
pub use render::{draw_overlay_extras, prepare_panel};

#[derive(Debug, Clone)]
pub struct SettingsState {
    pub screen: Screen,
    pub confirm: Option<ConfirmAction>,
    pub slider_drag: bool,
    pub hover: Option<layout::Hit>,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            screen: Screen::Main,
            confirm: None,
            slider_drag: false,
            hover: None,
        }
    }
}

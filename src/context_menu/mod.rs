mod control;
mod input;
mod layout;
mod render;

pub use control::ContextMenuControl;
pub use render::{invalidate_cache, prepare_menu};

#[derive(Debug, Clone)]
pub struct ContextMenuState {
    pub open: bool,
    pub origin_x: i32,
    pub origin_y: i32,
    pub hover: Option<layout::Hit>,
}

impl Default for ContextMenuState {
    fn default() -> Self {
        Self {
            open: false,
            origin_x: 0,
            origin_y: 0,
            hover: None,
        }
    }
}

pub mod handlers {
    pub use super::input::{
        handle_pointer_button, handle_pointer_motion, keyboard_filter, open_at,
        open_at_logical, super_held,
    };
}

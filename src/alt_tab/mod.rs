mod input;
mod layout;
mod render;

pub use render::{invalidate_cache, prepare_overlay};

#[derive(Debug, Clone, Default)]
pub struct AltTabState {
    pub open: bool,
    /// Índice na lista MRU rotacionada (0 = app focada agora).
    pub slot: usize,
}

pub mod handlers {
    pub use super::input::{close, keyboard_filter, try_open};
}

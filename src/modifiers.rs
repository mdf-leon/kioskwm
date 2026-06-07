//! Super/Meta tracking — compositor keyboard, evdev hardware, and xkb state.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Default)]
pub struct ModifierTracker {
    evdev_super: AtomicBool,
}

impl ModifierTracker {
    pub fn new_arc() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn set_evdev_super(&self, held: bool) {
        self.evdev_super.store(held, Ordering::Relaxed);
    }

    pub fn evdev_super(&self) -> bool {
        self.evdev_super.load(Ordering::Relaxed)
    }
}

pub fn super_held(
    keyboard_logo: bool,
    local_super_keys: u8,
    tracker: &ModifierTracker,
) -> bool {
    keyboard_logo || local_super_keys > 0 || tracker.evdev_super()
}

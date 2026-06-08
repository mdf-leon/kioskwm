//! Super/Meta and Right-Alt tracking — compositor keyboard, evdev, and xkb state.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Default)]
pub struct ModifierTracker {
    evdev_super: AtomicBool,
    evdev_right_alt: AtomicBool,
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

    pub fn set_evdev_right_alt(&self, held: bool) {
        self.evdev_right_alt.store(held, Ordering::Relaxed);
    }

    pub fn evdev_right_alt(&self) -> bool {
        self.evdev_right_alt.load(Ordering::Relaxed)
    }
}

pub fn super_held(
    keyboard_logo: bool,
    local_super_keys: u8,
    tracker: &ModifierTracker,
) -> bool {
    keyboard_logo || local_super_keys > 0 || tracker.evdev_super()
}

pub fn right_alt_held(local_right_alt_keys: u8, tracker: &ModifierTracker) -> bool {
    local_right_alt_keys > 0 || tracker.evdev_right_alt()
}

pub fn context_menu_modifier_held(
    keyboard_logo: bool,
    local_super_keys: u8,
    local_right_alt_keys: u8,
    tracker: &ModifierTracker,
) -> bool {
    super_held(keyboard_logo, local_super_keys, tracker)
        || right_alt_held(local_right_alt_keys, tracker)
}

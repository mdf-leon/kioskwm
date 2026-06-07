//! Posição do ponteiro compartilhada com o thread evdev.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct HardwareBridge {
    pointer_x: AtomicU64,
    pointer_y: AtomicU64,
}

impl HardwareBridge {
    pub fn new_arc() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self::default())
    }

    pub fn set_pointer(&self, x: f64, y: f64) {
        self.pointer_x.store(x.to_bits(), Ordering::Relaxed);
        self.pointer_y.store(y.to_bits(), Ordering::Relaxed);
    }

    pub fn pointer(&self) -> (f64, f64) {
        (
            f64::from_bits(self.pointer_x.load(Ordering::Relaxed)),
            f64::from_bits(self.pointer_y.load(Ordering::Relaxed)),
        )
    }
}

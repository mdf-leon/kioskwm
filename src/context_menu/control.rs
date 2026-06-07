//! Pedido evdev → main loop (igual OverlayControl).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use smithay::utils::{Logical, Point};

use crate::state::State;

use super::handlers::open_at;

pub struct ContextMenuControl {
    requested: AtomicBool,
    x: AtomicU64,
    y: AtomicU64,
    wake_loop: bool,
}

impl ContextMenuControl {
    pub fn new() -> std::sync::Arc<Self> {
        Self::with_loop_wake(false)
    }

    pub fn with_loop_wake(wake: bool) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            requested: AtomicBool::new(false),
            x: AtomicU64::new(0),
            y: AtomicU64::new(0),
            wake_loop: wake,
        })
    }

    pub fn request_open(&self, x: f64, y: f64) {
        self.x.store(x.to_bits(), Ordering::Relaxed);
        self.y.store(y.to_bits(), Ordering::Relaxed);
        self.requested.store(true, Ordering::SeqCst);
        if self.wake_loop {
            unsafe {
                libc::raise(libc::SIGUSR2);
            }
        }
    }

    pub fn poll(&self, state: &mut State) {
        if !self.requested.swap(false, Ordering::SeqCst) {
            return;
        }
        let x = f64::from_bits(self.x.load(Ordering::Relaxed));
        let y = f64::from_bits(self.y.load(Ordering::Relaxed));
        tracing::info!("Menu WM (evdev) em ({x:.0}, {y:.0})");
        open_at(state, Point::<f64, Logical>::from((x, y)));
    }
}

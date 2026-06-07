//! Métricas leves de performance — ative com `KIOSKWM_PERF=1`.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("KIOSKWM_PERF").is_some())
}

pub struct FrameTimer {
    start: Instant,
}

impl FrameTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
}

struct Counters {
    frames_rendered: AtomicU64,
    frames_skipped: AtomicU64,
    pointer_moves: AtomicU64,
    last_report: std::sync::Mutex<Instant>,
}

impl Counters {
    fn new() -> Self {
        Self {
            frames_rendered: AtomicU64::new(0),
            frames_skipped: AtomicU64::new(0),
            pointer_moves: AtomicU64::new(0),
            last_report: std::sync::Mutex::new(Instant::now()),
        }
    }
}

static COUNTERS: std::sync::OnceLock<Counters> = std::sync::OnceLock::new();

fn counters() -> &'static Counters {
    COUNTERS.get_or_init(Counters::new)
}

pub fn record_frame_rendered(render_ms: f64, damage_regions: usize, full_damage: bool) {
    if !enabled() {
        return;
    }
    let c = counters();
    c.frames_rendered.fetch_add(1, Ordering::Relaxed);
    maybe_report(c, render_ms, damage_regions, full_damage);
}

pub fn record_frame_skipped() {
    if !enabled() {
        return;
    }
    let c = counters();
    c.frames_skipped.fetch_add(1, Ordering::Relaxed);
    maybe_report(c, 0.0, 0, false);
}

pub fn record_pointer_move() {
    if !enabled() {
        return;
    }
    counters().pointer_moves.fetch_add(1, Ordering::Relaxed);
}

fn maybe_report(c: &Counters, render_ms: f64, damage_regions: usize, full_damage: bool) {
    let Ok(mut last) = c.last_report.lock() else {
        return;
    };
    if last.elapsed() < Duration::from_secs(1) {
        return;
    }
    *last = Instant::now();
    let rendered = c.frames_rendered.swap(0, Ordering::Relaxed);
    let skipped = c.frames_skipped.swap(0, Ordering::Relaxed);
    let moves = c.pointer_moves.swap(0, Ordering::Relaxed);
    tracing::info!(
        "perf: rendered={rendered}/s skipped={skipped}/s pointer_moves={moves}/s \
         last_render_ms={render_ms:.2} damage_regions={damage_regions} full={full_damage}"
    );
}

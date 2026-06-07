//! Slider hibrido: esquerda linear 0.01x–1x (centro), direita log 1x–SPEED_MAX.

pub const SPEED_MIN: f64 = 0.01;
pub const SPEED_MAX: f64 = 4.0;
pub const SPEED_CENTER: f64 = 1.0;

const LN_MAX: f64 = 1.386_294_361_119_890_6;

/// Posicao normalizada [0, 1] no trilho -> multiplicador.
pub fn speed_from_t(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.5 {
        let u = t / 0.5;
        SPEED_MIN + u * (SPEED_CENTER - SPEED_MIN)
    } else {
        let u = (t - 0.5) / 0.5;
        (u * LN_MAX).exp().clamp(SPEED_CENTER, SPEED_MAX)
    }
}

/// Multiplicador -> posicao normalizada no trilho.
pub fn t_from_speed(speed: f64) -> f64 {
    let speed = speed.clamp(SPEED_MIN, SPEED_MAX);
    if speed <= SPEED_CENTER {
        let u = (speed - SPEED_MIN) / (SPEED_CENTER - SPEED_MIN);
        0.5 * u
    } else {
        let u = speed.ln() / LN_MAX;
        0.5 + 0.5 * u
    }
}

pub fn speed_from_slider_x(x: i32, slider_x: i32, slider_w: i32) -> f64 {
    let t = ((x - slider_x) as f64 / slider_w as f64).clamp(0.0, 1.0);
    speed_from_t(t)
}

pub fn format_speed(speed: f64) -> String {
    if speed < 0.1 {
        format!("{:.2}x", speed)
    } else if speed < 10.0 {
        format!("{:.2}x", speed)
    } else {
        format!("{:.1}x", speed)
    }
}

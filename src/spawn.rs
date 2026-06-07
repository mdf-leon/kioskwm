use std::{process::Command, time::Duration};

use crate::env_detect;

pub fn command_exists(name: &str) -> bool {
    let binary = name.split_whitespace().next().unwrap_or(name);
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {binary}"))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn terminal_from_env() -> Option<String> {
    for var in ["KIOSKWM_TERMINAL", "TERMINAL"] {
        if let Ok(term) = std::env::var(var) {
            let term = term.trim().to_string();
            if !term.is_empty() {
                let binary = term.split_whitespace().next().unwrap_or(&term);
                if command_exists(binary) {
                    tracing::info!("Terminal via {var}: {term}");
                    return Some(term);
                }
                tracing::warn!("{var}={term} definido mas não encontrado no PATH");
            }
        }
    }
    None
}

pub fn detect_terminal(on_tty: bool) -> String {
    if let Some(term) = terminal_from_env() {
        return term;
    }

    let candidates: &[&str] = if on_tty {
        // Konsole precisa do Plasma — inútil num VT cru
        &["alacritty", "foot", "kitty", "wezterm", "ghostty"]
    } else {
        &["alacritty", "konsole", "foot", "kitty", "wezterm", "ghostty"]
    };

    for candidate in candidates {
        if command_exists(candidate) {
            return candidate.to_string();
        }
    }

    if on_tty {
        "alacritty".to_string()
    } else {
        "konsole".to_string()
    }
}

pub fn resolve_terminal(requested: &str) -> String {
    let on_tty = env_detect::on_hardware_tty();

    if requested != "auto" {
        return requested.to_string();
    }

    let found = detect_terminal(on_tty);
    tracing::info!(
        "Terminal auto-detectado: {} (ambiente: {})",
        found,
        if on_tty { "TTY" } else { "desktop" }
    );
    found
}

pub fn spawn_terminal(command: &str, wayland_display: &str) {
    tracing::info!("Lançando terminal: {} (WAYLAND_DISPLAY={})", command, wayland_display);

    let mut parts = command.split_whitespace();
    let binary = parts.next().unwrap_or(command);
    let extra_args: Vec<&str> = parts.collect();

    let mut cmd = match binary {
        "konsole" => {
            let mut c = Command::new("konsole");
            c.arg("--separate");
            c
        }
        "alacritty" => Command::new("alacritty"),
        "foot" => Command::new("foot"),
        "kitty" => Command::new("kitty"),
        other => Command::new(other),
    };

    cmd.args(extra_args)
        .env("WAYLAND_DISPLAY", wayland_display)
        .env("QT_QPA_PLATFORM", "wayland")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_SOCKET");

    match cmd.spawn() {
        Ok(_) => tracing::info!("Terminal iniciado"),
        Err(err) => tracing::error!("Falha ao iniciar {}: {}", command, err),
    }
}

pub fn schedule_spawn(command: String, wayland_display: String, delay_ms: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(delay_ms));
        spawn_terminal(&command, &wayland_display);
    });
}

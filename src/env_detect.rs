/// Estamos num VT de hardware (/dev/tty1, /dev/tty2, …).
///
/// Tem prioridade sobre WAYLAND_DISPLAY/DISPLAY no ambiente — o .bashrc do KDE
/// costuma exportar essas vars e quebraria a detecção no tty3.
pub fn on_hardware_tty() -> bool {
    controlling_tty()
        .map(|path| is_hardware_tty_path(&path))
        .unwrap_or(false)
}

pub fn has_graphical_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
}

/// Caminho do TTY controlador (ex.: `/dev/tty3`), se existir.
pub fn controlling_tty() -> Option<String> {
    for fd in 0..3 {
        if let Ok(path) = std::fs::read_link(format!("/proc/self/fd/{fd}")) {
            let path = path.to_string_lossy().into_owned();
            if is_tty_device(&path) {
                return Some(path);
            }
        }
    }

    std::process::Command::new("tty")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| is_tty_device(s))
}

fn is_tty_device(path: &str) -> bool {
    path.starts_with("/dev/tty") || path.starts_with("/dev/pts/")
}

fn is_hardware_tty_path(path: &str) -> bool {
    let path = path.trim();
    if !path.starts_with("/dev/tty") {
        return false;
    }
    let suffix = &path["/dev/tty".len()..];
    !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit())
}

/// Info de debug para logar quando a detecção falhar.
pub fn detection_debug() -> String {
    let fd0 = std::fs::read_link("/proc/self/fd/0")
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "?".into());
    format!(
        "fd0={fd0} hardware_tty={} graphical_env={} XDG_SESSION_TYPE={} WAYLAND_DISPLAY={} DISPLAY={}",
        on_hardware_tty(),
        has_graphical_session(),
        std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "?".into()),
        std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "-".into()),
        std::env::var("DISPLAY").unwrap_or_else(|_| "-".into()),
    )
}

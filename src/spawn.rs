use std::{
    fs,
    io::ErrorKind,
    os::unix::{
        fs::PermissionsExt,
        net::UnixStream,
    },
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use wayland_server::ListeningSocket;

use crate::Args;

pub struct SpawnPlan {
    pub command: Option<String>,
}

pub fn command_exists(name: &str) -> bool {
    let binary = name.split_whitespace().next().unwrap_or(name);
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {binary}"))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn app_from_env() -> Option<String> {
    for var in ["KIOSKWM_APP", "KIOSKWM_TERMINAL", "TERMINAL"] {
        if let Ok(app) = std::env::var(var) {
            let app = app.trim().to_string();
            if !app.is_empty() {
                let binary = app.split_whitespace().next().unwrap_or(&app);
                if command_exists(binary) {
                    tracing::info!("App via {var}: {app}");
                    return Some(app);
                }
                tracing::warn!("{var}={app} is set but not found in PATH");
            }
        }
    }
    None
}

pub fn detect_terminal() -> Option<String> {
    let candidates = &["foot", "alacritty", "konsole", "kitty", "wezterm", "ghostty"];
    for candidate in candidates {
        if command_exists(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Resolve o que lançar na subida: app explícita, auto-detect ou nada (no-spawn silencioso).
pub fn resolve_spawn(args: &Args) -> SpawnPlan {
    if args.no_spawn {
        tracing::info!("No-spawn mode (--no-spawn)");
        return SpawnPlan { command: None };
    }

    let requested = args.app.trim();

    if requested != "auto" {
        let binary = requested.split_whitespace().next().unwrap_or(requested);
        if command_exists(binary) {
            return SpawnPlan {
                command: Some(requested.to_string()),
            };
        }
        tracing::warn!(
            "App '{requested}' not found in PATH — continuing without auto-spawn"
        );
        return SpawnPlan { command: None };
    }

    if let Some(app) = app_from_env() {
        return SpawnPlan { command: Some(app) };
    }

    if let Some(term) = detect_terminal() {
        tracing::info!("Auto-spawn: terminal emulator '{term}'");
        return SpawnPlan {
            command: Some(term),
        };
    }

    tracing::info!(
        "No terminal emulator in PATH — no-spawn mode (waiting for Wayland clients)"
    );
    SpawnPlan { command: None }
}

fn user_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
}

fn runtime_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
}

fn escape_shell(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// Mata instância anterior (cargo run repetido) para não acumular wayland-N mortos.
pub fn ensure_single_instance() {
    let Some(dir) = runtime_dir() else {
        return;
    };
    let pid_file = dir.join("kioskwm.pid");
    let my_pid = std::process::id() as i32;
    if let Ok(contents) = fs::read_to_string(&pid_file) {
        if let Ok(old_pid) = contents.trim().parse::<i32>() {
            if old_pid != my_pid && Path::new(&format!("/proc/{old_pid}")).exists() {
                tracing::info!("Encerrando kioskwm anterior (pid={old_pid})");
                unsafe { libc::kill(old_pid, libc::SIGTERM) };
                std::thread::sleep(Duration::from_millis(400));
                if Path::new(&format!("/proc/{old_pid}")).exists() {
                    unsafe { libc::kill(old_pid, libc::SIGKILL) };
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
    let _ = fs::write(&pid_file, format!("{my_pid}\n"));
}

fn remove_dead_wayland_sockets() {
    let Some(dir) = runtime_dir() else {
        return;
    };
    for n in 1..32 {
        let path = dir.join(format!("wayland-{n}"));
        if !path.exists() || path.is_symlink() {
            continue;
        }
        let dead = matches!(
            UnixStream::connect(&path),
            Err(e) if e.kind() == ErrorKind::ConnectionRefused
                || e.raw_os_error() == Some(libc::ECONNREFUSED)
        );
        if dead {
            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(dir.join(format!("wayland-{n}.lock")));
            tracing::info!("Removido socket wayland-{n} morto");
        }
    }
}

/// Snaps só acessam sockets reais `wayland-N` (symlinks não funcionam).
pub fn bind_wayland_socket() -> Result<ListeningSocket, wayland_server::BindError> {
    ensure_single_instance();
    clear_stale_wayland_symlinks();
    clear_legacy_kioskwm_sockets();
    remove_dead_wayland_sockets();
    ListeningSocket::bind_auto("wayland", 1..32)
}

fn write_active_display(wayland_display: &str) {
    let Some(dir) = runtime_dir() else {
        return;
    };
    let path = dir.join("kioskwm-active");
    let _ = fs::write(&path, wayland_display);
}

fn read_active_wayland() -> Option<String> {
    let dir = runtime_dir()?;
    let s = fs::read_to_string(dir.join("kioskwm-active")).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn read_x11_display() -> Option<u32> {
    let dir = runtime_dir()?;
    let pid: u32 = fs::read_to_string(dir.join("kioskwm-x11-pid"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    if pid != std::process::id() {
        return None;
    }
    let s = fs::read_to_string(dir.join("kioskwm-x11-display")).ok()?;
    s.trim().parse().ok()
}

fn clear_stale_x11_display() {
    let Some(dir) = runtime_dir() else {
        return;
    };
    let _ = fs::remove_file(dir.join("kioskwm-x11-display"));
    let _ = fs::remove_file(dir.join("kioskwm-x11-pid"));
}

/// Atualiza env do terminal quando XWayland sobe (snap Krita usa DISPLAY=:N).
pub fn set_x11_display(display_num: u32) {
    let Some(dir) = runtime_dir() else {
        return;
    };
    let pid = std::process::id();
    let _ = fs::write(
        dir.join("kioskwm-x11-display"),
        format!("{display_num}\n"),
    );
    let _ = fs::write(dir.join("kioskwm-x11-pid"), format!("{pid}\n"));
    if let Some(wd) = read_active_wayland() {
        write_active_display(&wd);
        if let Some(scripts) = write_client_env(&wd) {
            let _ = write_app_wrappers();
            let _ = scripts;
        }
    }
    tracing::info!(
        "DISPLAY=:{display_num} — snap/xcb apps podem abrir no kioskwm"
    );
}

/// Gera env e wrappers antes do terminal (socket já definido).
pub fn prepare_runtime_files(wayland_display: &str) {
    clear_stale_x11_display();
    write_active_display(wayland_display);
    if let Some(scripts) = write_client_env(wayland_display) {
        let _ = write_app_wrappers();
        let _ = scripts;
    }
    tracing::info!(
        "Snap krita: aguarde log DISPLAY=:N (XWayland); apt: sudo apt install krita"
    );
}

pub fn log_bound_socket(socket_name: &str) {
    let Some(dir) = runtime_dir() else {
        return;
    };
    let path = dir.join(socket_name);
    let kind = if path.is_symlink() {
        "symlink"
    } else if path.exists() {
        "real"
    } else {
        "ausente"
    };
    tracing::info!(
        "Socket {socket_name} ({kind}, pid={})",
        std::process::id()
    );
}

fn clear_stale_wayland_symlinks() {
    let Some(dir) = runtime_dir() else {
        return;
    };
    for n in 1..32 {
        let path = dir.join(format!("wayland-{n}"));
        if path.is_symlink() {
            if fs::remove_file(&path).is_ok() {
                tracing::info!("Removido symlink stale: wayland-{n}");
            }
        }
    }
}

fn clear_legacy_kioskwm_sockets() {
    let Some(dir) = runtime_dir() else {
        return;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Só sockets Wayland legados (kioskwm-0), não scripts/env.
        let Some(suffix) = name.strip_prefix("kioskwm-") else {
            continue;
        };
        if suffix.chars().all(|c| c.is_ascii_digit()) {
            if fs::remove_file(entry.path()).is_ok() {
                tracing::info!("Removido socket legado: {name}");
            }
        }
    }
}

struct ClientEnvScripts {
    base: PathBuf,
    qt: PathBuf,
}

/// Env base: socket Wayland — seguro para snaps (sem QT_QPA_PLATFORM).
fn write_client_env(wayland_display: &str) -> Option<ClientEnvScripts> {
    let dir = runtime_dir()?;
    let base = dir.join("kioskwm-client-env.sh");
    let qt = dir.join("kioskwm-qt-env.sh");
    let wd = escape_shell(wayland_display);

    let display_line = read_x11_display()
        .map(|n| format!("export DISPLAY=':{n}'\n"))
        .unwrap_or_else(|| "unset DISPLAY\n".into());
    let base_content = format!(
        r#"# Gerado pelo kioskwm — env base (sem Qt/GDK — snaps quebram com wayland forçado).
export WAYLAND_DISPLAY='{wd}'
export XDG_SESSION_TYPE=wayland
{display_line}unset WAYLAND_SOCKET
"#
    );
    let qt_content = r#"# Gerado pelo kioskwm — env Qt/GTK (não usar em snaps).
export QT_QPA_PLATFORM=wayland
export GDK_BACKEND=wayland
export SDL_VIDEODRIVER=wayland
export MOZ_ENABLE_WAYLAND=1
"#
    .to_string();

    fs::write(&base, base_content).ok()?;
    fs::write(&qt, qt_content).ok()?;
    tracing::info!("Env base: {}", base.display());
    tracing::info!("Env Qt: {}", qt.display());
    Some(ClientEnvScripts { base, qt })
}

fn write_app_wrappers() -> Option<PathBuf> {
    let dir = runtime_dir()?.join("kioskwm-bin");
    fs::create_dir_all(&dir).ok()?;

    let kate = dir.join("kate");
    let kate_body = r#"#!/bin/sh
# kioskwm: Kate via XWayland (Wayland nativo congela no kiosk).
rt="${XDG_RUNTIME_DIR:?}"
wd="${WAYLAND_DISPLAY:-$(cat "$rt/kioskwm-active" 2>/dev/null)}"
xd="$(cat "$rt/kioskwm-x11-display" 2>/dev/null)"
xpid="$(cat "$rt/kioskwm-x11-pid" 2>/dev/null)"
if [ -n "$xpid" ] && ! kill -0 "$xpid" 2>/dev/null; then
  echo "kioskwm: XWayland obsoleto — reinicie o compositor" >&2
  exit 1
fi
if [ -z "$wd" ]; then
  echo "kioskwm: compositor não detectado — inicie o kioskwm primeiro" >&2
  exit 1
fi
if [ -x /usr/bin/kate ]; then
  if [ -z "$xd" ]; then
    echo "kioskwm: XWayland ainda não iniciou — aguarde o log DISPLAY=:N" >&2
    exit 1
  fi
  exec env -u WAYLAND_DISPLAY -u GDK_BACKEND -u MOZ_ENABLE_WAYLAND DISPLAY=":$xd" QT_QPA_PLATFORM=xcb /usr/bin/kate "$@"
fi
echo "kioskwm: kate não encontrado — sudo apt install kate" >&2
exit 1
"#;
    write_executable(&kate, kate_body)?;

    let krita = dir.join("krita");
    let krita_body = r#"#!/bin/sh
# kioskwm: deb krita → Wayland; snap krita → XWayland (DISPLAY=:N).
rt="${XDG_RUNTIME_DIR:?}"
wd="${WAYLAND_DISPLAY:-$(cat "$rt/kioskwm-active" 2>/dev/null)}"
xd="$(cat "$rt/kioskwm-x11-display" 2>/dev/null)"
xpid="$(cat "$rt/kioskwm-x11-pid" 2>/dev/null)"
if [ -n "$xpid" ] && ! kill -0 "$xpid" 2>/dev/null; then
  echo "kioskwm: XWayland obsoleto — reinicie o compositor" >&2
  exit 1
fi
if [ -z "$wd" ]; then
  echo "kioskwm: compositor não detectado — inicie o kioskwm primeiro" >&2
  exit 1
fi
krita_via_snap() {
  command -v snap >/dev/null 2>&1 && snap list krita 2>/dev/null | grep -q '^krita '
}
launch_snap_krita() {
  if [ -z "$xd" ]; then
    echo "kioskwm: XWayland ainda não iniciou — aguarde o log DISPLAY=:N no /tmp/kioskwm.log" >&2
    exit 1
  fi
  echo "kioskwm: krita via XWayland (DISPLAY=:$xd)" >&2
  exec env -u WAYLAND_DISPLAY -u GDK_BACKEND -u MOZ_ENABLE_WAYLAND -u QT_QPA_PLATFORM \
    -u SDL_VIDEODRIVER DISPLAY=":$xd" QT_QPA_PLATFORM=xcb XDG_SESSION_TYPE=x11 \
    snap run krita "$@"
}
launch_deb_krita() {
  echo "kioskwm: krita deb via Wayland ($wd)" >&2
  exec env -u DISPLAY WAYLAND_DISPLAY="$wd" QT_QPA_PLATFORM=wayland GDK_BACKEND=wayland \
    /usr/bin/krita "$@"
}
# Snap sempre XWayland — nunca tentar Wayland nativo no snap.
if krita_via_snap; then
  launch_snap_krita
fi
if [ -x /usr/bin/krita ]; then
  case "$(readlink -f /usr/bin/krita 2>/dev/null)" in
    *snap*) launch_snap_krita ;;
    *) launch_deb_krita ;;
  esac
fi
echo "kioskwm: krita não encontrado — sudo apt install krita  ou  snap install krita" >&2
exit 1
"#;
    write_executable(&krita, krita_body)?;

    let firefox = dir.join("firefox");
    let firefox_body = r#"#!/bin/sh
# kioskwm: Firefox — Wayland nativo; snap cai em XWayland (DISPLAY=:N).
rt="${XDG_RUNTIME_DIR:?}"
wd="${WAYLAND_DISPLAY:-$(cat "$rt/kioskwm-active" 2>/dev/null)}"
xd="$(cat "$rt/kioskwm-x11-display" 2>/dev/null)"
if [ -z "$wd" ]; then
  echo "kioskwm: compositor não detectado — inicie o kioskwm primeiro" >&2
  exit 1
fi
[ -f "$rt/kioskwm-client-env.sh" ] && . "$rt/kioskwm-client-env.sh"
[ -f "$rt/kioskwm-qt-env.sh" ] && . "$rt/kioskwm-qt-env.sh"
if command -v firefox >/dev/null 2>&1; then
  exec firefox "$@"
fi
if command -v firefox-esr >/dev/null 2>&1; then
  exec firefox-esr "$@"
fi
if [ -n "$xd" ] && command -v snap >/dev/null 2>&1; then
  exec env -u WAYLAND_DISPLAY DISPLAY=":$xd" MOZ_ENABLE_WAYLAND=0 snap run firefox "$@"
fi
echo "kioskwm: firefox não encontrado — sudo apt install firefox" >&2
exit 1
"#;
    write_executable(&firefox, firefox_body)?;
    Some(dir)
}

fn write_executable(path: &Path, content: &str) -> Option<()> {
    fs::write(path, content).ok()?;
    let mut perms = fs::metadata(path).ok()?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).ok()?;
    Some(())
}

fn write_zsh_wrapper(scripts: &ClientEnvScripts, bin_dir: Option<&Path>) -> Option<PathBuf> {
    let dir = runtime_dir()?.join("kioskwm-zsh");
    fs::create_dir_all(&dir).ok()?;
    let path = dir.join(".zshrc");
    let base = escape_shell(&scripts.base.display().to_string());
    let qt = escape_shell(&scripts.qt.display().to_string());
    let bin = bin_dir
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let app_fns = if bin.is_empty() {
        String::new()
    } else {
        let b = bin.trim_end_matches('/');
        format!(
            "kate() {{ \"{b}/kate\" \"$@\"; }}\n\
             krita() {{ \"{b}/krita\" \"$@\"; }}\n\
             firefox() {{ \"{b}/firefox\" \"$@\"; }}\n",
        )
    };
    let bin_path = kioskwm_bin_path_prefix().unwrap_or_default();
    let path_export = if bin_path.is_empty() {
        String::new()
    } else {
        format!("export PATH='{}'\n", escape_shell(&bin_path))
    };
    let content = format!(
        r#"# kioskwm — profile intacto; env base reaplicado sem Qt (snaps).
export KIOSKWM_ENV='{base}'
export KIOSKWM_QT_ENV='{qt}'
{path_export}unset ZDOTDIR
[ -f "$HOME/.zshenv" ] && source "$HOME/.zshenv"
[ -f "$HOME/.zshrc" ] && source "$HOME/.zshrc"
[ -f "$KIOSKWM_ENV" ] && source "$KIOSKWM_ENV"
{app_fns}
unalias kate krita firefox 2>/dev/null
kioskwm_reapply_env() {{ [ -f "$KIOSKWM_ENV" ] && source "$KIOSKWM_ENV"; }}
precmd_functions+=(kioskwm_reapply_env)
[[ -z "$KIOSKWM_BANNER" ]] && KIOSKWM_BANNER=1 && \
  echo "[kioskwm] socket=$WAYLAND_DISPLAY  kate=$(whence -w kate 2>/dev/null | head -1 || echo '?')  krita=$(whence -w krita 2>/dev/null | head -1 || echo '?')  DISPLAY=${{DISPLAY:-vazio}}"
"#,
        base = base,
        qt = qt,
        path_export = path_export,
        app_fns = app_fns,
    );
    fs::write(&path, content).ok()?;
    Some(dir)
}

fn write_bash_wrapper(scripts: &ClientEnvScripts, bin_dir: Option<&Path>) -> Option<PathBuf> {
    let dir = runtime_dir()?;
    let path = dir.join("kioskwm-bashrc");
    let base = escape_shell(&scripts.base.display().to_string());
    let qt = escape_shell(&scripts.qt.display().to_string());
    let app_fns = bin_dir
        .map(|p| {
            format!(
                "kate() {{ '{}/kate' \"$@\"; }}\n\
                 krita() {{ '{}/krita' \"$@\"; }}\n\
                 firefox() {{ '{}/firefox' \"$@\"; }}\n",
                p.display(),
                p.display(),
                p.display()
            )
        })
        .unwrap_or_default();
    let bin_path = kioskwm_bin_path_prefix().unwrap_or_default();
    let path_export = if bin_path.is_empty() {
        String::new()
    } else {
        format!("export PATH='{}'\n", escape_shell(&bin_path))
    };
    let content = format!(
        r#"# kioskwm — profile intacto; env base sem Qt para snaps.
export KIOSKWM_ENV='{base}'
export KIOSKWM_QT_ENV='{qt}'
{path_export}[ -f "$HOME/.bashrc" ] && . "$HOME/.bashrc"
[ -f "$KIOSKWM_ENV" ] && . "$KIOSKWM_ENV"
kioskwm_reapply_env() {{ [ -f "$KIOSKWM_ENV" ] && . "$KIOSKWM_ENV"; }}
PROMPT_COMMAND="kioskwm_reapply_env${{PROMPT_COMMAND:+; $PROMPT_COMMAND}}"
{app_fns}"#,
        base = base,
        qt = qt,
        app_fns = app_fns,
    );
    fs::write(&path, content).ok()?;
    Some(path)
}

fn write_shell_init(wayland_display: &str, scripts: &ClientEnvScripts) -> Option<PathBuf> {
    let dir = runtime_dir()?;
    let path = dir.join("kioskwm-shell-init.sh");
    let shell = user_shell();
    let name = shell.rsplit('/').next().unwrap_or("bash");
    let base = escape_shell(&scripts.base.display().to_string());

    let bins = write_app_wrappers();
    let body = match name {
        "zsh" => {
            let zdot = write_zsh_wrapper(scripts, bins.as_deref())?;
            format!(
                r#"export KIOSKWM_ENV='{base}'
export ZDOTDIR='{zdot}'
exec zsh -i
"#,
                zdot = zdot.display(),
            )
        }
        "bash" => {
            let rc = write_bash_wrapper(scripts, bins.as_deref())?;
            format!(
                r#"export KIOSKWM_ENV='{base}'
exec bash --rcfile '{rc}' -i
"#,
                rc = rc.display(),
            )
        }
        _ => format!(
            r#"export KIOSKWM_ENV='{base}'
. "$KIOSKWM_ENV"
exec '{shell}' -i
"#,
            shell = escape_shell(&shell),
        ),
    };

    let content = format!(
        "#!{shell}\n# Gerado pelo kioskwm — terminal com env Wayland.\n{body}",
        shell = shell,
    );
    fs::write(&path, content).ok()?;
    let mut perms = fs::metadata(&path).ok()?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).ok()?;
    tracing::info!(
        "Shell init: {} (WAYLAND_DISPLAY={wayland_display})",
        path.display()
    );
    Some(path)
}

fn prepend_kioskwm_bin_path(cmd: &mut Command) {
    if let Some(dir) = runtime_dir() {
        let bin = dir.join("kioskwm-bin");
        if bin.is_dir() {
            let path =
                std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
            cmd.env("PATH", format!("{}:{path}", bin.display()));
        }
    }
}

fn kioskwm_bin_path_prefix() -> Option<String> {
    let dir = runtime_dir()?.join("kioskwm-bin");
    if !dir.is_dir() {
        return None;
    }
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
    Some(format!("{}:{path}", dir.display()))
}

fn wrapper_binary(name: &str) -> Option<PathBuf> {
    let dir = runtime_dir()?.join("kioskwm-bin");
    let path = dir.join(name);
    path.is_file().then_some(path)
}

/// Só socket Wayland + PATH dos wrappers — sem Qt/GDK (snaps quebram com wayland forçado).
fn apply_wayland_env(cmd: &mut Command, wayland_display: &str) {
    cmd.env("WAYLAND_DISPLAY", wayland_display)
        .env("XDG_SESSION_TYPE", "wayland")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_SOCKET");
    prepend_kioskwm_bin_path(cmd);
}

fn command_for_spawn(binary: &str) -> Command {
    if let Some(wrapper) = wrapper_binary(binary) {
        tracing::info!("Usando wrapper kioskwm: {}", wrapper.display());
        Command::new(wrapper)
    } else {
        Command::new(binary)
    }
}

pub fn spawn_app(command: &str, wayland_display: &str) {
    tracing::info!("Launching app: {} (WAYLAND_DISPLAY={})", command, wayland_display);

    let scripts = write_client_env(wayland_display);
    let shell_init = scripts
        .as_ref()
        .and_then(|s| write_shell_init(wayland_display, s));

    let mut parts = command.split_whitespace();
    let binary = parts.next().unwrap_or(command);
    let extra_args: Vec<&str> = parts.collect();

    let mut cmd = match binary {
        "konsole" => {
            let mut c = Command::new("konsole");
            c.arg("--separate");
            c.arg("-p").arg("TabTitleFormat=kioskwm");
            if std::env::var_os("KIOSKWM_DEBUG_TERMINAL").is_some() {
                c.arg("--hold");
            }
            c.args(&extra_args);
            if let Some(init) = &shell_init {
                c.arg("-e").arg(init);
            } else if let Some(s) = &scripts {
                let shell = user_shell();
                let cmdline = format!(". '{}'; exec '{shell}' -i", s.base.display());
                c.arg("-e").arg(&shell).arg("-c").arg(cmdline);
            }
            c.env("QSG_RHI_BACKEND", "software");
            c.env("QT_QUICK_BACKEND", "software");
            c
        }
        "alacritty" => {
            let mut c = Command::new("alacritty");
            c.args(&extra_args);
            if let Some(init) = &shell_init {
                c.arg("-e").arg(init);
            }
            c
        }
        "foot" => {
            let mut c = Command::new("foot");
            c.args(&extra_args);
            if let Some(init) = &shell_init {
                c.arg("-e").arg(init);
            } else if let Some(s) = &scripts {
                let shell = user_shell();
                let cmdline = format!(". '{}'; exec '{shell}' -i", s.base.display());
                c.arg("-e").arg(&shell).arg("-c").arg(cmdline);
            }
            c
        }
        "kitty" => {
            let mut c = Command::new("kitty");
            c.args(&extra_args);
            c
        }
        other => {
            let mut c = command_for_spawn(other);
            c.args(&extra_args);
            c
        }
    };

    apply_wayland_env(&mut cmd, wayland_display);

    match cmd.spawn() {
        Ok(_) => tracing::info!("App iniciada: {command}"),
        Err(err) => tracing::error!("Failed to start {command}: {err}"),
    }
}

pub fn schedule_spawn(plan: SpawnPlan, wayland_display: String, delay_ms: u64) {
    let Some(command) = plan.command else {
        return;
    };
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(delay_ms));
        spawn_app(&command, &wayland_display);
    });
}
